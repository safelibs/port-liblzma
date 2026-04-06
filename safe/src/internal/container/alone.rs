use core::ffi::c_void;
use core::ptr;
use std::io::{Cursor, Read, Write};

use crate::ffi::types::{
    lzma_action, lzma_allocator, lzma_options_lzma, lzma_ret, lzma_stream, LZMA_DATA_ERROR,
    LZMA_MEMLIMIT_ERROR, LZMA_OK, LZMA_PROG_ERROR, LZMA_STREAM_END,
};
use crate::internal::common::{all_supported_actions, LZMA_FINISH};
use crate::internal::container::stream::copy_output_buffer;
use crate::internal::lzma::{io_error_to_ret, LZMA_MEMUSAGE_BASE};
use crate::internal::stream_state::{install_next_coder, NextCoder};

struct AloneEncoder {
    options: lzma_options_lzma,
    input: Vec<u8>,
    output: Vec<u8>,
    output_pos: usize,
}

struct AloneDecoder {
    memlimit: u64,
    memusage: u64,
    input: Vec<u8>,
    output: Vec<u8>,
    output_pos: usize,
    decoded: bool,
}

const ALONE_HEADER_SIZE: usize = 1 + 4 + 8;

fn alone_header_memusage(input: &[u8]) -> Option<u64> {
    if input.len() < ALONE_HEADER_SIZE {
        return None;
    }

    let props = input[0];
    let dict_size = u32::from_le_bytes([input[1], input[2], input[3], input[4]]);
    lzma_rust2::lzma_get_memory_usage_by_props(dict_size, props)
        .ok()
        .map(|usage_kib| {
            u64::from(usage_kib)
                .saturating_mul(1024)
                .saturating_add(LZMA_MEMUSAGE_BASE)
        })
}

unsafe fn alone_encoder_code(
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
    let coder = &mut *coder.cast::<AloneEncoder>();
    if coder.output_pos < coder.output.len() {
        return copy_output_buffer(
            &coder.output,
            &mut coder.output_pos,
            output,
            out_pos,
            out_size,
        );
    }

    if in_size != 0 {
        coder
            .input
            .extend_from_slice(core::slice::from_raw_parts(input, in_size));
        *in_pos = in_size;
    }

    if action != LZMA_FINISH {
        return LZMA_OK;
    }

    let options = match crate::internal::lzma::parse_filters(
        (&[
            crate::ffi::types::lzma_filter {
                id: crate::internal::filter::common::LZMA_FILTER_LZMA1,
                options: (&mut coder.options as *mut lzma_options_lzma).cast(),
            },
            crate::ffi::types::lzma_filter {
                id: crate::ffi::types::LZMA_VLI_UNKNOWN,
                options: ptr::null_mut(),
            },
        ] as *const [crate::ffi::types::lzma_filter; 2])
            .cast::<crate::ffi::types::lzma_filter>(),
    ) {
        Ok(chain) => match chain.terminal {
            crate::internal::lzma::TerminalFilter::Lzma1 { options, .. } => options,
            _ => return LZMA_PROG_ERROR,
        },
        Err(ret) => return ret,
    };

    let sink = Cursor::new(Vec::new());
    let mut writer = match lzma_rust2::LzmaWriter::new_use_header(sink, &options, None) {
        Ok(writer) => writer,
        Err(error) => return io_error_to_ret(&error),
    };

    if let Err(error) = writer.write_all(&coder.input) {
        return io_error_to_ret(&error);
    }

    let sink = match writer.finish() {
        Ok(sink) => sink,
        Err(error) => return io_error_to_ret(&error),
    };

    coder.output = sink.into_inner();
    coder.output_pos = 0;
    copy_output_buffer(
        &coder.output,
        &mut coder.output_pos,
        output,
        out_pos,
        out_size,
    )
}

unsafe fn alone_decoder_code(
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
    let coder = &mut *coder.cast::<AloneDecoder>();
    if coder.output_pos < coder.output.len() {
        return copy_output_buffer(
            &coder.output,
            &mut coder.output_pos,
            output,
            out_pos,
            out_size,
        );
    }

    if in_size != 0 {
        coder
            .input
            .extend_from_slice(core::slice::from_raw_parts(input, in_size));
        *in_pos = in_size;
    }

    if let Some(memusage) = alone_header_memusage(&coder.input) {
        coder.memusage = memusage;
        if coder.memusage > coder.memlimit.max(1) {
            return LZMA_MEMLIMIT_ERROR;
        }
    }

    if coder.decoded {
        return LZMA_STREAM_END;
    }

    if action != LZMA_FINISH {
        return LZMA_OK;
    }

    let memlimit_kib =
        (coder.memlimit.max(1).saturating_add(1023) / 1024).min(u32::MAX as u64) as u32;
    let mut reader = match lzma_rust2::LzmaReader::new_mem_limit(
        Cursor::new(coder.input.as_slice()),
        memlimit_kib,
        None,
    ) {
        Ok(reader) => reader,
        Err(error) => return io_error_to_ret(&error),
    };
    if let Err(error) = reader.read_to_end(&mut coder.output) {
        return io_error_to_ret(&error);
    }

    let consumed = reader.into_inner().position() as usize;
    if consumed < coder.input.len() {
        let props = coder.input[0];
        let dict_size = u32::from_le_bytes([
            coder.input[1],
            coder.input[2],
            coder.input[3],
            coder.input[4],
        ]);
        let payload = &coder.input[ALONE_HEADER_SIZE..];
        let mut verifier = match lzma_rust2::LzmaReader::new_with_props(
            Cursor::new(payload),
            u64::MAX,
            props,
            dict_size,
            None,
        ) {
            Ok(reader) => reader,
            Err(_) => return LZMA_DATA_ERROR,
        };
        let mut full_output = Vec::new();
        if verifier.read_to_end(&mut full_output).is_err() {
            return LZMA_DATA_ERROR;
        }
        if verifier.into_inner().position() as usize != payload.len() || full_output != coder.output
        {
            return LZMA_DATA_ERROR;
        }
    }

    coder.decoded = true;
    coder.output_pos = 0;
    copy_output_buffer(
        &coder.output,
        &mut coder.output_pos,
        output,
        out_pos,
        out_size,
    )
}

unsafe fn alone_end(coder: *mut c_void, _allocator: *const lzma_allocator) {
    drop(Box::from_raw(coder.cast::<AloneEncoder>()));
}

unsafe fn alone_decoder_end(coder: *mut c_void, _allocator: *const lzma_allocator) {
    drop(Box::from_raw(coder.cast::<AloneDecoder>()));
}

unsafe fn alone_decoder_memconfig(
    coder: *mut c_void,
    memusage: *mut u64,
    old_memlimit: *mut u64,
    new_memlimit: u64,
) -> lzma_ret {
    let coder = &mut *coder.cast::<AloneDecoder>();
    *memusage = coder.memusage.max(LZMA_MEMUSAGE_BASE);
    *old_memlimit = coder.memlimit.max(1);

    if new_memlimit != 0 {
        let new_memlimit = new_memlimit.max(1);
        if new_memlimit < coder.memusage.max(LZMA_MEMUSAGE_BASE) {
            return LZMA_MEMLIMIT_ERROR;
        }
        coder.memlimit = new_memlimit;
    }

    LZMA_OK
}

pub(crate) unsafe fn alone_encoder(
    strm: *mut lzma_stream,
    options: *const lzma_options_lzma,
) -> lzma_ret {
    if strm.is_null() || options.is_null() {
        return LZMA_PROG_ERROR;
    }

    install_next_coder(
        strm,
        NextCoder {
            coder: Box::into_raw(Box::new(AloneEncoder {
                options: *options,
                input: Vec::new(),
                output: Vec::new(),
                output_pos: 0,
            }))
            .cast(),
            code: alone_encoder_code,
            end: Some(alone_end),
            get_progress: None,
            get_check: None,
            memconfig: None,
            update: None,
        },
        all_supported_actions(),
    )
}

pub(crate) unsafe fn alone_decoder(strm: *mut lzma_stream, memlimit: u64) -> lzma_ret {
    if strm.is_null() {
        return LZMA_PROG_ERROR;
    }

    install_next_coder(
        strm,
        NextCoder {
            coder: Box::into_raw(Box::new(AloneDecoder {
                memlimit: memlimit.max(1),
                memusage: LZMA_MEMUSAGE_BASE,
                input: Vec::new(),
                output: Vec::new(),
                output_pos: 0,
                decoded: false,
            }))
            .cast(),
            code: alone_decoder_code,
            end: Some(alone_decoder_end),
            get_progress: None,
            get_check: None,
            memconfig: Some(alone_decoder_memconfig),
            update: None,
        },
        all_supported_actions(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ffi::types::LZMA_STREAM_INIT;
    use crate::internal::stream_state::{lzma_code_impl, lzma_end_impl};

    #[test]
    fn alone_decoder_accepts_known_size_with_eopm() {
        let input = include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/upstream/files/good-known_size-with_eopm.lzma"
        ));

        unsafe {
            let mut strm = LZMA_STREAM_INIT;
            assert_eq!(alone_decoder(&mut strm, u64::MAX), LZMA_OK);

            strm.next_in = input.as_ptr();
            strm.avail_in = input.len();
            let mut output = [0u8; 64];
            strm.next_out = output.as_mut_ptr();
            strm.avail_out = output.len();

            assert_eq!(lzma_code_impl(&mut strm, LZMA_FINISH), LZMA_STREAM_END);
            assert_eq!(strm.total_out, 13);
            lzma_end_impl(&mut strm);
        }
    }

    #[test]
    fn alone_decoder_rejects_too_small_known_size_without_eopm() {
        let input = include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/upstream/files/bad-too_small_size-without_eopm-1.lzma"
        ));

        unsafe {
            let mut strm = LZMA_STREAM_INIT;
            assert_eq!(alone_decoder(&mut strm, u64::MAX), LZMA_OK);

            strm.next_in = input.as_ptr();
            strm.avail_in = input.len();
            let mut output = [0u8; 64];
            strm.next_out = output.as_mut_ptr();
            strm.avail_out = output.len();

            assert_eq!(lzma_code_impl(&mut strm, LZMA_FINISH), LZMA_DATA_ERROR);
            lzma_end_impl(&mut strm);
        }
    }
}
