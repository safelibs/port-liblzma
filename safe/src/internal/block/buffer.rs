use core::mem;
use core::ptr;

use crate::ffi::types::{
    lzma_allocator, lzma_block, lzma_filter, lzma_options_lzma, lzma_ret, LZMA_BUF_ERROR,
    LZMA_DATA_ERROR, LZMA_OK, LZMA_OPTIONS_ERROR, LZMA_PROG_ERROR, LZMA_STREAM_END, LZMA_UNSUPPORTED_CHECK,
    LZMA_VLI_UNKNOWN,
};
use crate::internal::block::header::{block_header_encode, block_header_size, block_total_size, block_unpadded_size};
use crate::internal::check::{self, CheckState};
use crate::internal::common::{LZMA_DICT_SIZE_MIN, LZMA_VLI_MAX};
use crate::internal::filter::common::LZMA_FILTER_LZMA2;
use crate::internal::lzma;

const LZMA2_CHUNK_MAX: u32 = 1 << 16;
const LZMA2_HEADER_UNCOMPRESSED: u32 = 3;
const COMPRESSED_SIZE_MAX: u64 = (LZMA_VLI_MAX
    - super::header::LZMA_BLOCK_HEADER_SIZE_MAX as u64
    - crate::ffi::types::LZMA_CHECK_SIZE_MAX as u64)
    & !3;

const HEADERS_BOUND: u64 = (1
    + 1
    + 2 * crate::internal::common::LZMA_VLI_BYTES_MAX as u64
    + 3
    + 4
    + crate::ffi::types::LZMA_CHECK_SIZE_MAX as u64
    + 3)
    & !3;

fn lzma2_bound(uncompressed_size: u64) -> u64 {
    if uncompressed_size > COMPRESSED_SIZE_MAX {
        return 0;
    }

    let overhead =
        uncompressed_size.div_ceil(LZMA2_CHUNK_MAX as u64) * u64::from(LZMA2_HEADER_UNCOMPRESSED) + 1;
    if COMPRESSED_SIZE_MAX - overhead < uncompressed_size {
        return 0;
    }

    uncompressed_size + overhead
}

fn decode_lzma2_uncompressed_chunks(input: &[u8]) -> Result<Vec<u8>, lzma_ret> {
    let mut pos = 0usize;
    let mut output = Vec::new();

    while pos < input.len() {
        let control = input[pos];
        pos += 1;

        if control == 0x00 {
            if pos != input.len() {
                return Err(LZMA_DATA_ERROR);
            }
            return Ok(output);
        }

        if control != 0x01 && control != 0x02 {
            return Err(LZMA_OPTIONS_ERROR);
        }

        if input.len() - pos < 2 {
            return Err(LZMA_DATA_ERROR);
        }

        let copy_size = (((input[pos] as usize) << 8) | input[pos + 1] as usize) + 1;
        pos += 2;

        if input.len() - pos < copy_size {
            return Err(LZMA_DATA_ERROR);
        }

        output.extend_from_slice(&input[pos..pos + copy_size]);
        pos += copy_size;
    }

    Err(LZMA_DATA_ERROR)
}

pub(crate) unsafe fn block_buffer_bound(uncompressed_size: usize) -> usize {
    let mut size = lzma2_bound(uncompressed_size as u64);
    if size == 0 {
        return 0;
    }
    size = (size + 3) & !3;
    let total = HEADERS_BOUND + size;
    usize::try_from(total).unwrap_or(0)
}

unsafe fn encode_normal_block(
    block: *mut lzma_block,
    input: *const u8,
    input_size: usize,
    output: *mut u8,
    output_pos: *mut usize,
    output_size: usize,
) -> lzma_ret {
    let chain = match lzma::parse_filters((*block).filters.cast_const()) {
        Ok(chain) => chain,
        Err(ret) => return ret,
    };

    let input_slice = if input_size == 0 {
        &[]
    } else {
        core::slice::from_raw_parts(input, input_size)
    };
    let raw = match lzma::encode_raw(&chain, input_slice) {
        Ok(raw) => raw,
        Err(ret) => return ret,
    };

    (*block).compressed_size = raw.len() as u64;
    (*block).uncompressed_size = input_size as u64;

    let ret = block_header_size(block);
    if ret != LZMA_OK {
        return ret;
    }

    let total_size = block_total_size(block.cast_const());
    if total_size == 0 || total_size == LZMA_VLI_UNKNOWN {
        return LZMA_PROG_ERROR;
    }

    if output_size - *output_pos < total_size as usize {
        return LZMA_BUF_ERROR;
    }

    let start = *output_pos;
    let ret = block_header_encode(block.cast_const(), output.add(start));
    if ret != LZMA_OK {
        return ret;
    }

    let data_start = start + (*block).header_size as usize;
    ptr::copy_nonoverlapping(raw.as_ptr(), output.add(data_start), raw.len());

    let mut state = match CheckState::new((*block).check) {
        Some(state) => state,
        None => return LZMA_OPTIONS_ERROR,
    };
    state.update(input_slice);
    (*block).raw_check = state.finish();

    let check_size = check::check_size((*block).check) as usize;
    ptr::copy_nonoverlapping(
        (*block).raw_check.as_ptr(),
        output.add(data_start + raw.len()),
        check_size,
    );

    let end = start + total_size as usize;
    let unpadded = block_unpadded_size(block.cast_const()) as usize;
    ptr::write_bytes(output.add(start + unpadded), 0, end - (start + unpadded));
    *output_pos = end;

    LZMA_OK
}

unsafe fn encode_uncompressed_block(
    block: *mut lzma_block,
    input: *const u8,
    input_size: usize,
    output: *mut u8,
    output_pos: *mut usize,
    output_size: usize,
) -> lzma_ret {
    let mut lzma2: lzma_options_lzma = mem::zeroed();
    lzma2.dict_size = LZMA_DICT_SIZE_MIN;
    let mut filters = [
        lzma_filter {
            id: LZMA_FILTER_LZMA2,
            options: (&mut lzma2 as *mut lzma_options_lzma).cast(),
        },
        lzma_filter {
            id: crate::ffi::types::LZMA_VLI_UNKNOWN,
            options: ptr::null_mut(),
        },
    ];

    let original_filters = (*block).filters;
    (*block).filters = filters.as_mut_ptr();
    (*block).compressed_size = lzma2_bound(input_size as u64);
    (*block).uncompressed_size = input_size as u64;

    let ret = block_header_size(block);
    if ret != LZMA_OK {
        (*block).filters = original_filters;
        return LZMA_PROG_ERROR;
    }

    let total_size = block_total_size(block.cast_const());
    if total_size == 0 || total_size == LZMA_VLI_UNKNOWN || output_size - *output_pos < total_size as usize {
        (*block).filters = original_filters;
        return LZMA_BUF_ERROR;
    }

    let start = *output_pos;
    let ret = block_header_encode(block.cast_const(), output.add(start));
    if ret != LZMA_OK {
        (*block).filters = original_filters;
        return LZMA_PROG_ERROR;
    }
    (*block).filters = original_filters;
    *output_pos += (*block).header_size as usize;

    let mut in_pos = 0usize;
    let mut control = 0x01u8;
    while in_pos < input_size {
        *output.add(*output_pos) = control;
        *output_pos += 1;
        control = 0x02;

        let copy_size = (input_size - in_pos).min(LZMA2_CHUNK_MAX as usize);
        *output.add(*output_pos) = ((copy_size - 1) >> 8) as u8;
        *output.add(*output_pos + 1) = (copy_size - 1) as u8;
        *output_pos += 2;

        ptr::copy_nonoverlapping(input.add(in_pos), output.add(*output_pos), copy_size);
        in_pos += copy_size;
        *output_pos += copy_size;
    }

    *output.add(*output_pos) = 0;
    *output_pos += 1;

    let mut state = match CheckState::new((*block).check) {
        Some(state) => state,
        None => return LZMA_OPTIONS_ERROR,
    };
    let input_slice = if input_size == 0 {
        &[]
    } else {
        core::slice::from_raw_parts(input, input_size)
    };
    state.update(input_slice);
    (*block).raw_check = state.finish();

    let check_size = check::check_size((*block).check) as usize;
    ptr::copy_nonoverlapping((*block).raw_check.as_ptr(), output.add(*output_pos), check_size);
    *output_pos += check_size;

    let end = start + total_size as usize;
    ptr::write_bytes(output.add(*output_pos), 0, end - *output_pos);
    *output_pos = end;
    LZMA_OK
}

unsafe fn block_buffer_encode_internal(
    block: *mut lzma_block,
    input: *const u8,
    input_size: usize,
    output: *mut u8,
    output_pos: *mut usize,
    output_size: usize,
    try_to_compress: bool,
) -> lzma_ret {
    if block.is_null()
        || (input.is_null() && input_size != 0)
        || output.is_null()
        || output_pos.is_null()
        || *output_pos > output_size
    {
        return LZMA_PROG_ERROR;
    }

    if (*block).version > 1 {
        return LZMA_OPTIONS_ERROR;
    }

    if ((*block).check as usize) > crate::internal::common::LZMA_CHECK_ID_MAX
        || (try_to_compress && (*block).filters.is_null())
    {
        return LZMA_PROG_ERROR;
    }

    if check::check_is_supported((*block).check) == 0 {
        return LZMA_UNSUPPORTED_CHECK;
    }

    if try_to_compress {
        let saved_pos = *output_pos;
        let ret = encode_normal_block(block, input, input_size, output, output_pos, output_size);
        if ret == LZMA_OK {
            return ret;
        }
        *output_pos = saved_pos;
    }

    encode_uncompressed_block(block, input, input_size, output, output_pos, output_size)
}

pub(crate) unsafe fn block_buffer_encode(
    block: *mut lzma_block,
    _allocator: *const lzma_allocator,
    input: *const u8,
    input_size: usize,
    output: *mut u8,
    output_pos: *mut usize,
    output_size: usize,
) -> lzma_ret {
    block_buffer_encode_internal(block, input, input_size, output, output_pos, output_size, true)
}

pub(crate) unsafe fn block_uncomp_encode(
    block: *mut lzma_block,
    input: *const u8,
    input_size: usize,
    output: *mut u8,
    output_pos: *mut usize,
    output_size: usize,
) -> lzma_ret {
    block_buffer_encode_internal(block, input, input_size, output, output_pos, output_size, false)
}

pub(crate) unsafe fn block_buffer_decode(
    block: *mut lzma_block,
    _allocator: *const lzma_allocator,
    input: *const u8,
    input_pos: *mut usize,
    input_size: usize,
    output: *mut u8,
    output_pos: *mut usize,
    output_size: usize,
) -> lzma_ret {
    if input_pos.is_null()
        || (input.is_null() && *input_pos != input_size)
        || *input_pos > input_size
        || output_pos.is_null()
        || (output.is_null() && *output_pos != output_size)
        || *output_pos > output_size
        || block.is_null()
    {
        return LZMA_PROG_ERROR;
    }

    let in_start = *input_pos;
    let out_start = *output_pos;

    if block_unpadded_size(block.cast_const()) == 0 || (*block).filters.is_null() {
        return LZMA_PROG_ERROR;
    }

    let compressed_size = (*block).compressed_size as usize;
    let check_size = check::check_size((*block).check) as usize;
    let total_size = block_total_size(block.cast_const());
    if total_size == 0 || total_size == LZMA_VLI_UNKNOWN {
        return LZMA_PROG_ERROR;
    }

    let end = in_start + total_size as usize - (*block).header_size as usize;
    if end > input_size || in_start + compressed_size + check_size > input_size {
        return LZMA_DATA_ERROR;
    }

    let chain = match lzma::parse_filters((*block).filters.cast_const()) {
        Ok(chain) => chain,
        Err(ret) => return ret,
    };

    let compressed = core::slice::from_raw_parts(input.add(in_start), compressed_size);
    let decoded = match lzma::decode_raw(&chain, compressed) {
        Ok(decoded) => decoded,
        Err(ret) => {
            if chain.prefilters.is_empty()
                && matches!(chain.terminal, lzma::TerminalFilter::Lzma2 { .. })
            {
                match decode_lzma2_uncompressed_chunks(compressed) {
                    Ok(decoded) => decoded,
                    Err(_) => {
                        *input_pos = in_start;
                        *output_pos = out_start;
                        return ret;
                    }
                }
            } else {
                *input_pos = in_start;
                *output_pos = out_start;
                return ret;
            }
        }
    };

    if output_size - *output_pos < decoded.len() {
        *input_pos = in_start;
        *output_pos = out_start;
        return LZMA_BUF_ERROR;
    }

    ptr::copy_nonoverlapping(decoded.as_ptr(), output.add(*output_pos), decoded.len());
    *output_pos += decoded.len();
    (*block).uncompressed_size = decoded.len() as u64;

    let check_ptr = input.add(in_start + compressed_size);
    ptr::copy_nonoverlapping(check_ptr, (*block).raw_check.as_mut_ptr(), check_size);

    let ignore_check = (*block).version >= 1 && (*block).ignore_check != 0;
    let verify_check = !ignore_check && check::check_is_supported((*block).check) != 0;
    if verify_check {
        let mut state = match CheckState::new((*block).check) {
            Some(state) => state,
            None => {
                *input_pos = in_start;
                *output_pos = out_start;
                return LZMA_OPTIONS_ERROR;
            }
        };
        state.update(&decoded);
        if state.finish()[..check_size] != (&(*block).raw_check)[..check_size] {
            *input_pos = in_start;
            *output_pos = out_start;
            return crate::ffi::types::LZMA_DATA_ERROR;
        }
    }

    let padding_start = in_start + compressed_size + check_size;
    for i in padding_start..end {
        if *input.add(i) != 0 {
            *input_pos = in_start;
            *output_pos = out_start;
            return crate::ffi::types::LZMA_DATA_ERROR;
        }
    }

    *input_pos = end;
    LZMA_OK
}
