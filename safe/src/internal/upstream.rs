use core::ffi::c_void;
use core::mem;
use core::ptr;
use std::io::{Cursor, Read};

use crate::ffi::types::{
    lzma_action, lzma_allocator, lzma_block, lzma_check, lzma_filter, lzma_options_lzma, lzma_ret,
    lzma_stream, lzma_vli, LZMA_BUF_ERROR, LZMA_OK, LZMA_OPTIONS_ERROR, LZMA_PROG_ERROR,
    LZMA_STREAM_END, LZMA_UNSUPPORTED_CHECK, LZMA_VLI_UNKNOWN,
};
use crate::internal::block;
use crate::internal::check;
use crate::internal::common::{
    all_supported_actions, lzma_bool as to_lzma_bool, LZMA_CHECK_CRC32, LZMA_PRESET_LEVEL_MASK,
};
use crate::internal::filter;
use crate::internal::lzma::{self, LZMA_LZMA1EXT_ALLOW_EOPM};
use crate::internal::preset;
use crate::internal::stream_state::{current_next_coder, install_next_coder, NextCoder};
use crate::internal::vli::lzma_vli_encode_impl;

const STREAM_ENCODER_MAGIC: u64 = 0x7366_6c74_5f78_7a31;

#[derive(Clone, Copy)]
struct IndexRecord {
    unpadded_size: u64,
    uncompressed_size: u64,
}

struct RawCoder {
    filters: [lzma_filter; crate::ffi::types::LZMA_FILTERS_MAX + 1],
    input: Vec<u8>,
    output: Vec<u8>,
    output_pos: usize,
    encode: bool,
    finished: bool,
}

struct StreamEncoderCoder {
    magic: u64,
    filters: [lzma_filter; crate::ffi::types::LZMA_FILTERS_MAX + 1],
    check: lzma_check,
    input: Vec<u8>,
    pending: Vec<u8>,
    pending_pos: usize,
    records: Vec<IndexRecord>,
    header_written: bool,
    finished: bool,
}

struct StreamDecoderCoder {
    input: Vec<u8>,
    output: Vec<u8>,
    output_pos: usize,
    memlimit: u64,
    flags: u32,
    decoded: bool,
}

unsafe fn copy_filters(
    src: *const lzma_filter,
) -> Result<[lzma_filter; crate::ffi::types::LZMA_FILTERS_MAX + 1], lzma_ret> {
    let mut dest = [lzma_filter {
        id: LZMA_VLI_UNKNOWN,
        options: ptr::null_mut(),
    }; crate::ffi::types::LZMA_FILTERS_MAX + 1];
    let ret = filter::filters_copy_impl(src, dest.as_mut_ptr(), ptr::null());
    if ret != LZMA_OK {
        return Err(ret);
    }

    Ok(dest)
}

unsafe fn free_filters(filters: &mut [lzma_filter; crate::ffi::types::LZMA_FILTERS_MAX + 1]) {
    filter::filters_free_impl(filters.as_mut_ptr(), ptr::null());
}

unsafe fn copy_output(
    buffer: &[u8],
    state_pos: &mut usize,
    output: *mut u8,
    out_pos: *mut usize,
    out_size: usize,
) -> lzma_ret {
    let copy_size = (buffer.len() - *state_pos).min(out_size - *out_pos);
    ptr::copy_nonoverlapping(buffer.as_ptr().add(*state_pos), output.add(*out_pos), copy_size);
    *state_pos += copy_size;
    *out_pos += copy_size;
    if *state_pos == buffer.len() {
        LZMA_STREAM_END
    } else {
        LZMA_OK
    }
}

fn append_vli(output: &mut Vec<u8>, value: u64) {
    let mut temp = [0u8; crate::internal::common::LZMA_VLI_BYTES_MAX];
    let mut pos = 0usize;
    unsafe {
        let ret = lzma_vli_encode_impl(
            value,
            ptr::null_mut(),
            temp.as_mut_ptr(),
            &mut pos,
            temp.len(),
        );
        debug_assert_eq!(ret, LZMA_OK);
    }
    output.extend_from_slice(&temp[..pos]);
}

fn write_xz_stream_header(check: lzma_check, output: &mut Vec<u8>) {
    output.extend_from_slice(&[0xFD, b'7', b'z', b'X', b'Z', 0x00, 0x00, check as u8]);
    let crc = check::crc32::crc32(&output[6..8], 0);
    output.extend_from_slice(&crc.to_le_bytes());
}

fn write_xz_stream_footer(check: lzma_check, backward_size: u32, output: &mut Vec<u8>) {
    let mut footer = Vec::with_capacity(12);
    footer.extend_from_slice(&backward_size.to_le_bytes());
    footer.extend_from_slice(&[0, check as u8]);
    let crc = check::crc32::crc32(&footer, 0);
    output.extend_from_slice(&crc.to_le_bytes());
    output.extend_from_slice(&footer);
    output.extend_from_slice(b"YZ");
}

fn encode_xz_index(records: &[IndexRecord]) -> Vec<u8> {
    let mut output = Vec::new();
    output.push(0x00);
    append_vli(&mut output, records.len() as u64);
    for record in records {
        append_vli(&mut output, record.unpadded_size);
        append_vli(&mut output, record.uncompressed_size);
    }

    while (output.len() + 4) % 4 != 0 {
        output.push(0);
    }
    let crc = check::crc32::crc32(&output, 0);
    output.extend_from_slice(&crc.to_le_bytes());
    output
}

fn decode_vli(input: &[u8], pos: &mut usize) -> Result<u64, lzma_ret> {
    let mut value = 0u64;
    let mut shift = 0u32;

    loop {
        if *pos >= input.len() || shift >= 63 {
            return Err(crate::ffi::types::LZMA_DATA_ERROR);
        }

        let byte = input[*pos];
        *pos += 1;

        value |= u64::from(byte & 0x7F) << shift;
        if (byte & 0x80) == 0 {
            return Ok(value);
        }

        shift += 7;
    }
}

fn parse_index_records(input: &[u8]) -> Result<Vec<IndexRecord>, lzma_ret> {
    if input.len() < 8 {
        return Err(crate::ffi::types::LZMA_DATA_ERROR);
    }

    let payload_len = input.len() - 4;
    let expected_crc = check::crc32::crc32(&input[..payload_len], 0);
    let actual_crc = u32::from_le_bytes([
        input[payload_len],
        input[payload_len + 1],
        input[payload_len + 2],
        input[payload_len + 3],
    ]);
    if expected_crc != actual_crc || input[0] != 0x00 {
        return Err(crate::ffi::types::LZMA_DATA_ERROR);
    }

    let mut pos = 1usize;
    let record_count = decode_vli(&input[..payload_len], &mut pos)?;
    if record_count == 0 {
        return Err(crate::ffi::types::LZMA_DATA_ERROR);
    }

    let mut records = Vec::with_capacity(record_count as usize);
    for _ in 0..record_count {
        let unpadded_size = decode_vli(&input[..payload_len], &mut pos)?;
        let uncompressed_size = decode_vli(&input[..payload_len], &mut pos)?;
        records.push(IndexRecord {
            unpadded_size,
            uncompressed_size,
        });
    }

    while pos < payload_len {
        if input[pos] != 0 {
            return Err(crate::ffi::types::LZMA_DATA_ERROR);
        }
        pos += 1;
    }

    Ok(records)
}

fn decode_single_xz_stream_fallback(input: &[u8]) -> Result<(usize, Vec<u8>), lzma_ret> {
    use crate::ffi::types::lzma_stream_flags;
    use crate::internal::stream_flags::{
        stream_flags_compare_impl, stream_footer_decode_impl, stream_header_decode_impl,
        LZMA_STREAM_HEADER_SIZE,
    };

    if input.len() < LZMA_STREAM_HEADER_SIZE * 2 {
        return Err(crate::ffi::types::LZMA_DATA_ERROR);
    }

    let mut header_flags: lzma_stream_flags = unsafe { mem::zeroed() };
    let mut footer_flags: lzma_stream_flags = unsafe { mem::zeroed() };

    unsafe {
        let ret = stream_header_decode_impl(&mut header_flags, input.as_ptr());
        if ret != LZMA_OK {
            return Err(ret);
        }

        let ret = stream_footer_decode_impl(
            &mut footer_flags,
            input.as_ptr().add(input.len() - LZMA_STREAM_HEADER_SIZE),
        );
        if ret != LZMA_OK {
            return Err(ret);
        }

        let ret = stream_flags_compare_impl(&header_flags, &footer_flags);
        if ret != LZMA_OK {
            return Err(ret);
        }
    }

    let index_size = footer_flags.backward_size as usize;
    if input.len() < LZMA_STREAM_HEADER_SIZE + index_size + LZMA_STREAM_HEADER_SIZE {
        return Err(crate::ffi::types::LZMA_DATA_ERROR);
    }

    let index_start = input.len() - LZMA_STREAM_HEADER_SIZE - index_size;
    let records = parse_index_records(&input[index_start..input.len() - LZMA_STREAM_HEADER_SIZE])?;
    let total_output = records
        .iter()
        .try_fold(0usize, |acc, record| {
            acc.checked_add(record.uncompressed_size as usize)
                .ok_or(crate::ffi::types::LZMA_DATA_ERROR)
        })?;

    let mut output = vec![0u8; total_output];
    let mut block_start = LZMA_STREAM_HEADER_SIZE;
    let mut out_pos = 0usize;

    for record in records {
        let mut decoded_filters = [lzma_filter {
            id: LZMA_VLI_UNKNOWN,
            options: ptr::null_mut(),
        }; crate::ffi::types::LZMA_FILTERS_MAX + 1];
        let mut block_options: lzma_block = unsafe { mem::zeroed() };
        block_options.version = 1;
        block_options.check = header_flags.check;
        block_options.header_size = ((input[block_start] as u32) + 1) * 4;
        block_options.filters = decoded_filters.as_mut_ptr();

        unsafe {
            let ret = block::block_header_decode(
                &mut block_options,
                ptr::null(),
                input.as_ptr().add(block_start),
            );
            if ret != LZMA_OK {
                return Err(ret);
            }

            let ret = block::block_compressed_size(&mut block_options, record.unpadded_size);
            if ret != LZMA_OK {
                return Err(ret);
            }
        }

        let mut in_pos = block_start + block_options.header_size as usize;
        let block_end = block_start + unsafe { block::block_total_size(&block_options) as usize };
        let ret = unsafe {
            block::block_buffer_decode(
                &mut block_options,
                ptr::null(),
                input.as_ptr(),
                &mut in_pos,
                index_start,
                output.as_mut_ptr(),
                &mut out_pos,
                output.len(),
            )
        };
        if ret != LZMA_OK || in_pos != block_end {
            return Err(if ret == LZMA_OK {
                crate::ffi::types::LZMA_DATA_ERROR
            } else {
                ret
            });
        }

        block_start = block_end;
    }

    if block_start != index_start {
        return Err(crate::ffi::types::LZMA_DATA_ERROR);
    }

    Ok((input.len(), output))
}

unsafe fn encode_block_with_filters(
    filters: *mut lzma_filter,
    check_id: lzma_check,
    input: &[u8],
) -> Result<(Vec<u8>, IndexRecord), lzma_ret> {
    let bound = block::block_buffer_bound(input.len());
    if bound == 0 {
        return Err(LZMA_PROG_ERROR);
    }

    let mut block_options: lzma_block = mem::zeroed();
    block_options.version = 1;
    block_options.check = check_id;
    block_options.filters = filters;

    let mut encoded = vec![0u8; bound];
    let mut pos = 0usize;
    let ret = block::block_buffer_encode(
        &mut block_options,
        ptr::null(),
        input.as_ptr(),
        input.len(),
        encoded.as_mut_ptr(),
        &mut pos,
        encoded.len(),
    );
    if ret != LZMA_OK {
        return Err(ret);
    }

    let record = IndexRecord {
        unpadded_size: block::block_unpadded_size(&block_options),
        uncompressed_size: input.len() as u64,
    };

    encoded.truncate(pos);
    Ok((encoded, record))
}

fn decode_xz_stream_once(input: &[u8], concatenated: bool) -> Result<(usize, Vec<u8>), lzma_ret> {
    let cursor = Cursor::new(input);
    let mut reader = lzma_rust2::XzReader::new(cursor, concatenated);
    let mut output = Vec::new();
    match reader.read_to_end(&mut output) {
        Ok(_) => {
            let cursor = reader.into_inner();
            Ok((cursor.position() as usize, output))
        }
        Err(error) => {
            if !concatenated {
                decode_single_xz_stream_fallback(input)
            } else {
                Err(lzma::io_error_to_ret(&error))
            }
        }
    }
}

unsafe fn raw_code(
    coder: *mut c_void,
    _allocator: *const lzma_allocator,
    input: *const u8,
    in_pos: *mut usize,
    in_size: usize,
    output: *mut u8,
    out_pos: *mut usize,
    out_size: usize,
    action: lzma_action,
) -> lzma_ret {
    let coder = &mut *coder.cast::<RawCoder>();
    if coder.output_pos < coder.output.len() {
        return copy_output(&coder.output, &mut coder.output_pos, output, out_pos, out_size);
    }

    if in_size != 0 {
        coder.input.extend_from_slice(core::slice::from_raw_parts(input, in_size));
        *in_pos = in_size;
    }

    if coder.finished {
        return LZMA_STREAM_END;
    }

    if action != crate::internal::common::LZMA_FINISH {
        return LZMA_OK;
    }

    let chain = match lzma::parse_filters(coder.filters.as_ptr()) {
        Ok(chain) => chain,
        Err(ret) => return ret,
    };

    let result = if coder.encode {
        lzma::encode_raw(&chain, &coder.input)
    } else {
        lzma::decode_raw(&chain, &coder.input)
    };

    coder.output = match result {
        Ok(output) => output,
        Err(ret) => return ret,
    };
    coder.output_pos = 0;
    coder.finished = true;
    copy_output(&coder.output, &mut coder.output_pos, output, out_pos, out_size)
}

unsafe fn raw_end(coder: *mut c_void, _allocator: *const lzma_allocator) {
    let mut coder = Box::from_raw(coder.cast::<RawCoder>());
    free_filters(&mut coder.filters);
}

unsafe fn stream_encoder_code(
    coder: *mut c_void,
    _allocator: *const lzma_allocator,
    input: *const u8,
    in_pos: *mut usize,
    in_size: usize,
    output: *mut u8,
    out_pos: *mut usize,
    out_size: usize,
    action: lzma_action,
) -> lzma_ret {
    let coder = &mut *coder.cast::<StreamEncoderCoder>();
    if coder.pending_pos < coder.pending.len() {
        return copy_output(&coder.pending, &mut coder.pending_pos, output, out_pos, out_size);
    }
    if !coder.pending.is_empty() {
        coder.pending.clear();
        coder.pending_pos = 0;
    }

    if in_size != 0 {
        coder.input.extend_from_slice(core::slice::from_raw_parts(input, in_size));
        *in_pos = in_size;
    }

    if coder.finished {
        return LZMA_STREAM_END;
    }

    match action {
        crate::internal::common::LZMA_RUN => return LZMA_OK,
        crate::internal::common::LZMA_FULL_FLUSH | crate::internal::common::LZMA_FULL_BARRIER => {
            if coder.input.is_empty() {
                return LZMA_STREAM_END;
            }
            if !coder.header_written {
                write_xz_stream_header(coder.check, &mut coder.pending);
                coder.header_written = true;
            }
            let (encoded, record) = match encode_block_with_filters(coder.filters.as_mut_ptr(), coder.check, &coder.input) {
                Ok(result) => result,
                Err(ret) => return ret,
            };
            coder.records.push(record);
            coder.pending.extend_from_slice(&encoded);
            coder.pending_pos = 0;
            coder.input.clear();
            copy_output(&coder.pending, &mut coder.pending_pos, output, out_pos, out_size)
        }
        crate::internal::common::LZMA_FINISH => {
            if !coder.header_written {
                write_xz_stream_header(coder.check, &mut coder.pending);
                coder.header_written = true;
            }
            if !coder.input.is_empty() {
                let (encoded, record) =
                    match encode_block_with_filters(coder.filters.as_mut_ptr(), coder.check, &coder.input) {
                        Ok(result) => result,
                        Err(ret) => return ret,
                    };
                coder.records.push(record);
                coder.pending.extend_from_slice(&encoded);
                coder.input.clear();
            }

            let index = encode_xz_index(&coder.records);
            let backward_size = (index.len() / 4 - 1) as u32;
            coder.pending.extend_from_slice(&index);
            write_xz_stream_footer(coder.check, backward_size, &mut coder.pending);
            coder.pending_pos = 0;
            coder.finished = true;
            copy_output(&coder.pending, &mut coder.pending_pos, output, out_pos, out_size)
        }
        _ => LZMA_PROG_ERROR,
    }
}

unsafe fn stream_encoder_end(coder: *mut c_void, _allocator: *const lzma_allocator) {
    let mut coder = Box::from_raw(coder.cast::<StreamEncoderCoder>());
    free_filters(&mut coder.filters);
}

unsafe fn stream_get_check(coder: *const c_void) -> lzma_check {
    (*(coder.cast::<StreamEncoderCoder>())).check
}

unsafe fn stream_decoder_code(
    coder: *mut c_void,
    _allocator: *const lzma_allocator,
    input: *const u8,
    in_pos: *mut usize,
    in_size: usize,
    output: *mut u8,
    out_pos: *mut usize,
    out_size: usize,
    action: lzma_action,
) -> lzma_ret {
    let coder = &mut *coder.cast::<StreamDecoderCoder>();
    if coder.output_pos < coder.output.len() {
        return copy_output(&coder.output, &mut coder.output_pos, output, out_pos, out_size);
    }

    if in_size != 0 {
        coder.input.extend_from_slice(core::slice::from_raw_parts(input, in_size));
        *in_pos = in_size;
    }

    if coder.decoded {
        return LZMA_STREAM_END;
    }

    match decode_xz_stream_once(&coder.input, (coder.flags & 0x08) != 0) {
        Ok((_consumed, decoded)) => {
            coder.output = decoded;
            coder.output_pos = 0;
            coder.decoded = true;
            copy_output(&coder.output, &mut coder.output_pos, output, out_pos, out_size)
        }
        Err(ret) => {
            if action == crate::internal::common::LZMA_FINISH {
                if ret == crate::ffi::types::LZMA_DATA_ERROR {
                    LZMA_BUF_ERROR
                } else {
                    ret
                }
            } else {
                let _ = coder.memlimit;
                LZMA_OK
            }
        }
    }
}

unsafe fn stream_decoder_end(coder: *mut c_void, _allocator: *const lzma_allocator) {
    drop(Box::from_raw(coder.cast::<StreamDecoderCoder>()));
}

unsafe fn stream_decoder_memconfig(
    coder: *mut c_void,
    memusage: *mut u64,
    old_memlimit: *mut u64,
    new_memlimit: u64,
) -> lzma_ret {
    let coder = &mut *coder.cast::<StreamDecoderCoder>();
    *memusage = 1;
    *old_memlimit = coder.memlimit.max(1);
    if new_memlimit != 0 {
        coder.memlimit = new_memlimit;
    }
    LZMA_OK
}

pub(crate) unsafe fn raw_buffer_encode(
    filters: *const lzma_filter,
    _allocator: *const lzma_allocator,
    input: *const u8,
    input_size: usize,
    output: *mut u8,
    output_pos: *mut usize,
    output_size: usize,
) -> lzma_ret {
    if filters.is_null() || output.is_null() || output_pos.is_null() || *output_pos > output_size {
        return LZMA_PROG_ERROR;
    }
    let chain = match lzma::parse_filters(filters) {
        Ok(chain) => chain,
        Err(ret) => return ret,
    };

    let input_slice = if input_size == 0 {
        &[]
    } else if input.is_null() {
        return LZMA_PROG_ERROR;
    } else {
        core::slice::from_raw_parts(input, input_size)
    };

    let encoded = match lzma::encode_raw(&chain, input_slice) {
        Ok(encoded) => encoded,
        Err(ret) => return ret,
    };
    if output_size - *output_pos < encoded.len() {
        return LZMA_BUF_ERROR;
    }
    ptr::copy_nonoverlapping(encoded.as_ptr(), output.add(*output_pos), encoded.len());
    *output_pos += encoded.len();
    LZMA_OK
}

pub(crate) unsafe fn raw_buffer_decode(
    filters: *const lzma_filter,
    _allocator: *const lzma_allocator,
    input: *const u8,
    input_pos: *mut usize,
    input_size: usize,
    output: *mut u8,
    output_pos: *mut usize,
    output_size: usize,
) -> lzma_ret {
    if filters.is_null()
        || input.is_null()
        || input_pos.is_null()
        || *input_pos > input_size
        || output_pos.is_null()
        || (output.is_null() && *output_pos != output_size)
        || *output_pos > output_size
    {
        return LZMA_PROG_ERROR;
    }

    let chain = match lzma::parse_filters(filters) {
        Ok(chain) => chain,
        Err(ret) => return ret,
    };
    let input_slice = core::slice::from_raw_parts(input.add(*input_pos), input_size - *input_pos);
    let decoded = match lzma::decode_raw(&chain, input_slice) {
        Ok(decoded) => decoded,
        Err(ret) => return ret,
    };
    if output_size - *output_pos < decoded.len() {
        return LZMA_BUF_ERROR;
    }
    ptr::copy_nonoverlapping(decoded.as_ptr(), output.add(*output_pos), decoded.len());
    *output_pos += decoded.len();
    *input_pos = input_size;
    LZMA_OK
}

pub(crate) unsafe fn raw_encoder(strm: *mut lzma_stream, filters: *const lzma_filter) -> lzma_ret {
    if strm.is_null() {
        return LZMA_PROG_ERROR;
    }
    let copied = match copy_filters(filters) {
        Ok(copied) => copied,
        Err(ret) => return ret,
    };
    install_next_coder(
        strm,
        NextCoder {
            coder: Box::into_raw(Box::new(RawCoder {
                filters: copied,
                input: Vec::new(),
                output: Vec::new(),
                output_pos: 0,
                encode: true,
                finished: false,
            }))
            .cast(),
            code: raw_code,
            end: Some(raw_end),
            get_progress: None,
            get_check: None,
            memconfig: None,
        },
        all_supported_actions(),
    )
}

pub(crate) unsafe fn raw_decoder(strm: *mut lzma_stream, filters: *const lzma_filter) -> lzma_ret {
    if strm.is_null() {
        return LZMA_PROG_ERROR;
    }
    let copied = match copy_filters(filters) {
        Ok(copied) => copied,
        Err(ret) => return ret,
    };
    install_next_coder(
        strm,
        NextCoder {
            coder: Box::into_raw(Box::new(RawCoder {
                filters: copied,
                input: Vec::new(),
                output: Vec::new(),
                output_pos: 0,
                encode: false,
                finished: false,
            }))
            .cast(),
            code: raw_code,
            end: Some(raw_end),
            get_progress: None,
            get_check: None,
            memconfig: None,
        },
        all_supported_actions(),
    )
}

pub(crate) unsafe fn raw_encoder_memusage(filters: *const lzma_filter) -> u64 {
    lzma::encoder_memusage(filters)
}

pub(crate) unsafe fn raw_decoder_memusage(filters: *const lzma_filter) -> u64 {
    lzma::decoder_memusage(filters)
}

pub(crate) unsafe fn stream_buffer_bound(uncompressed_size: usize) -> usize {
    block::block_buffer_bound(uncompressed_size).saturating_add(64)
}

pub(crate) unsafe fn stream_buffer_encode(
    filters: *mut lzma_filter,
    check_id: lzma_check,
    _allocator: *const lzma_allocator,
    input: *const u8,
    input_size: usize,
    output: *mut u8,
    output_pos: *mut usize,
    output_size: usize,
) -> lzma_ret {
    if filters.is_null() || output.is_null() || output_pos.is_null() || *output_pos > output_size {
        return LZMA_PROG_ERROR;
    }
    if check::check_is_supported(check_id) == 0 {
        return LZMA_UNSUPPORTED_CHECK;
    }
    let input_slice = if input_size == 0 {
        &[]
    } else if input.is_null() {
        return LZMA_PROG_ERROR;
    } else {
        core::slice::from_raw_parts(input, input_size)
    };

    let mut temp = Vec::new();
    write_xz_stream_header(check_id, &mut temp);
    let (block_data, record) = match encode_block_with_filters(filters, check_id, input_slice) {
        Ok(result) => result,
        Err(ret) => return ret,
    };
    temp.extend_from_slice(&block_data);
    let index = encode_xz_index(&[record]);
    let backward_size = (index.len() / 4 - 1) as u32;
    temp.extend_from_slice(&index);
    write_xz_stream_footer(check_id, backward_size, &mut temp);

    if output_size - *output_pos < temp.len() {
        return LZMA_BUF_ERROR;
    }
    ptr::copy_nonoverlapping(temp.as_ptr(), output.add(*output_pos), temp.len());
    *output_pos += temp.len();
    LZMA_OK
}

pub(crate) unsafe fn stream_buffer_decode(
    _memlimit: *mut u64,
    flags: u32,
    _allocator: *const lzma_allocator,
    input: *const u8,
    input_pos: *mut usize,
    input_size: usize,
    output: *mut u8,
    output_pos: *mut usize,
    output_size: usize,
) -> lzma_ret {
    if input.is_null()
        || input_pos.is_null()
        || *input_pos > input_size
        || output_pos.is_null()
        || (output.is_null() && *output_pos != output_size)
        || *output_pos > output_size
    {
        return LZMA_PROG_ERROR;
    }

    let (consumed, decoded) = match decode_xz_stream_once(
        core::slice::from_raw_parts(input.add(*input_pos), input_size - *input_pos),
        (flags & 0x08) != 0,
    ) {
        Ok(result) => result,
        Err(ret) => return ret,
    };
    if output_size - *output_pos < decoded.len() {
        return LZMA_BUF_ERROR;
    }
    ptr::copy_nonoverlapping(decoded.as_ptr(), output.add(*output_pos), decoded.len());
    *output_pos += decoded.len();
    *input_pos += consumed;
    LZMA_OK
}

pub(crate) unsafe fn stream_encoder(
    strm: *mut lzma_stream,
    filters: *const lzma_filter,
    check_id: lzma_check,
) -> lzma_ret {
    if strm.is_null() {
        return LZMA_PROG_ERROR;
    }
    if check::check_is_supported(check_id) == 0 {
        return LZMA_UNSUPPORTED_CHECK;
    }
    let copied = match copy_filters(filters) {
        Ok(copied) => copied,
        Err(ret) => return ret,
    };
    install_next_coder(
        strm,
        NextCoder {
            coder: Box::into_raw(Box::new(StreamEncoderCoder {
                magic: STREAM_ENCODER_MAGIC,
                filters: copied,
                check: check_id,
                input: Vec::new(),
                pending: Vec::new(),
                pending_pos: 0,
                records: Vec::new(),
                header_written: false,
                finished: false,
            }))
            .cast(),
            code: stream_encoder_code,
            end: Some(stream_encoder_end),
            get_progress: None,
            get_check: Some(stream_get_check),
            memconfig: None,
        },
        all_supported_actions(),
    )
}

pub(crate) unsafe fn stream_decoder(strm: *mut lzma_stream, memlimit: u64, flags: u32) -> lzma_ret {
    if strm.is_null() {
        return LZMA_PROG_ERROR;
    }
    install_next_coder(
        strm,
        NextCoder {
            coder: Box::into_raw(Box::new(StreamDecoderCoder {
                input: Vec::new(),
                output: Vec::new(),
                output_pos: 0,
                memlimit,
                flags,
                decoded: false,
            }))
            .cast(),
            code: stream_decoder_code,
            end: Some(stream_decoder_end),
            get_progress: None,
            get_check: None,
            memconfig: Some(stream_decoder_memconfig),
        },
        all_supported_actions(),
    )
}

pub(crate) unsafe fn filters_update(strm: *mut lzma_stream, filters: *const lzma_filter) -> lzma_ret {
    let Some(next) = current_next_coder(strm) else {
        return LZMA_PROG_ERROR;
    };
    if next.code as usize != stream_encoder_code as usize {
        return LZMA_PROG_ERROR;
    }
    let coder = &mut *next.coder.cast::<StreamEncoderCoder>();
    if coder.magic != STREAM_ENCODER_MAGIC || !coder.input.is_empty() || coder.pending_pos < coder.pending.len() || coder.finished {
        return LZMA_PROG_ERROR;
    }

    let mut new_filters = match copy_filters(filters) {
        Ok(filters) => filters,
        Err(ret) => return ret,
    };
    free_filters(&mut coder.filters);
    coder.filters = new_filters;
    LZMA_OK
}

pub(crate) unsafe fn easy_encoder(strm: *mut lzma_stream, preset_id: u32, check_id: lzma_check) -> lzma_ret {
    let mut options: lzma_options_lzma = mem::zeroed();
    if preset::lzma_lzma_preset_impl(&mut options, preset_id) != 0 {
        return LZMA_OPTIONS_ERROR;
    }
    let filters = [
        lzma_filter {
            id: crate::internal::filter::common::LZMA_FILTER_LZMA2,
            options: (&mut options as *mut lzma_options_lzma).cast(),
        },
        lzma_filter {
            id: LZMA_VLI_UNKNOWN,
            options: ptr::null_mut(),
        },
    ];
    stream_encoder(strm, filters.as_ptr(), check_id)
}

pub(crate) unsafe fn easy_buffer_encode(
    preset_id: u32,
    check_id: lzma_check,
    allocator: *const lzma_allocator,
    input: *const u8,
    input_size: usize,
    output: *mut u8,
    output_pos: *mut usize,
    output_size: usize,
) -> lzma_ret {
    let mut options: lzma_options_lzma = mem::zeroed();
    if preset::lzma_lzma_preset_impl(&mut options, preset_id) != 0 {
        return LZMA_OPTIONS_ERROR;
    }
    let mut filters = [
        lzma_filter {
            id: crate::internal::filter::common::LZMA_FILTER_LZMA2,
            options: (&mut options as *mut lzma_options_lzma).cast(),
        },
        lzma_filter {
            id: LZMA_VLI_UNKNOWN,
            options: ptr::null_mut(),
        },
    ];
    stream_buffer_encode(filters.as_mut_ptr(), check_id, allocator, input, input_size, output, output_pos, output_size)
}

pub(crate) unsafe fn easy_encoder_memusage(preset_id: u32) -> u64 {
    let mut options: lzma_options_lzma = mem::zeroed();
    if preset::lzma_lzma_preset_impl(&mut options, preset_id & LZMA_PRESET_LEVEL_MASK) != 0 {
        return u64::from(u32::MAX);
    }
    lzma_rust2::Lzma2Options {
        lzma_options: lzma_rust2::LzmaOptions::with_preset(preset_id & LZMA_PRESET_LEVEL_MASK),
        ..Default::default()
    }
    .lzma_options
    .get_memory_usage() as u64
}

pub(crate) unsafe fn easy_decoder_memusage(preset_id: u32) -> u64 {
    let mut options: lzma_options_lzma = mem::zeroed();
    if preset::lzma_lzma_preset_impl(&mut options, preset_id & LZMA_PRESET_LEVEL_MASK) != 0 {
        return u64::from(u32::MAX);
    }
    u64::from(lzma_rust2::lzma2_get_memory_usage(options.dict_size))
}

pub(crate) unsafe fn auto_decoder(strm: *mut lzma_stream, memlimit: u64, flags: u32) -> lzma_ret {
    stream_decoder(strm, memlimit, flags)
}
