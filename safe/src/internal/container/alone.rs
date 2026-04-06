use core::ffi::c_void;
use core::ptr;
use std::io::{Cursor, Read, Write};

use crate::ffi::types::{lzma_action, lzma_allocator, lzma_options_lzma, lzma_ret, lzma_stream, LZMA_OK, LZMA_PROG_ERROR, LZMA_STREAM_END};
use crate::internal::common::{all_supported_actions, LZMA_FINISH};
use crate::internal::lzma::io_error_to_ret;
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

unsafe fn copy_output(output_buf: &[u8], output_pos_state: &mut usize, output: *mut u8, out_pos: *mut usize, out_size: usize) -> lzma_ret {
    let copy_size = (output_buf.len() - *output_pos_state).min(out_size - *out_pos);
    ptr::copy_nonoverlapping(output_buf.as_ptr().add(*output_pos_state), output.add(*out_pos), copy_size);
    *output_pos_state += copy_size;
    *out_pos += copy_size;
    if *output_pos_state == output_buf.len() {
        LZMA_STREAM_END
    } else {
        LZMA_OK
    }
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
        return copy_output(&coder.output, &mut coder.output_pos, output, out_pos, out_size);
    }

    if in_size != 0 {
        coder.input.extend_from_slice(core::slice::from_raw_parts(input, in_size));
        *in_pos = in_size;
    }

    if action != LZMA_FINISH {
        return LZMA_OK;
    }

    let options = match crate::internal::lzma::parse_filters((&[
        crate::ffi::types::lzma_filter {
            id: crate::internal::filter::common::LZMA_FILTER_LZMA1,
            options: (&mut coder.options as *mut lzma_options_lzma).cast(),
        },
        crate::ffi::types::lzma_filter {
            id: crate::ffi::types::LZMA_VLI_UNKNOWN,
            options: ptr::null_mut(),
        },
    ] as *const [crate::ffi::types::lzma_filter; 2]).cast::<crate::ffi::types::lzma_filter>()) {
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
    copy_output(&coder.output, &mut coder.output_pos, output, out_pos, out_size)
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
        return copy_output(&coder.output, &mut coder.output_pos, output, out_pos, out_size);
    }

    if in_size != 0 {
        coder.input.extend_from_slice(core::slice::from_raw_parts(input, in_size));
        *in_pos = in_size;
    }

    if coder.decoded {
        return LZMA_STREAM_END;
    }

    if action != LZMA_FINISH {
        return LZMA_OK;
    }

    let memlimit_kib = ((coder.memlimit.max(1) + 1023) / 1024).min(u32::MAX as u64) as u32;
    coder.memusage = 1;
    let mut reader =
        match lzma_rust2::LzmaReader::new_mem_limit(Cursor::new(&coder.input), memlimit_kib, None)
        {
            Ok(reader) => reader,
            Err(error) => return io_error_to_ret(&error),
        };
    if let Err(error) = reader.read_to_end(&mut coder.output) {
        return io_error_to_ret(&error);
    }

    coder.decoded = true;
    coder.output_pos = 0;
    copy_output(&coder.output, &mut coder.output_pos, output, out_pos, out_size)
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
    *memusage = coder.memusage.max(1);
    *old_memlimit = coder.memlimit.max(1);

    if new_memlimit != 0 {
        coder.memlimit = new_memlimit.max(1);
    }

    LZMA_OK
}

pub(crate) unsafe fn alone_encoder(strm: *mut lzma_stream, options: *const lzma_options_lzma) -> lzma_ret {
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
                memlimit,
                memusage: 1,
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
        },
        all_supported_actions(),
    )
}
