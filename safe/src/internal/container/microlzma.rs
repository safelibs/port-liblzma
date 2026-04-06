use core::ffi::c_void;
use core::ptr;
use std::io::{Cursor, Read, Write};

use crate::ffi::types::{lzma_action, lzma_allocator, lzma_bool, lzma_options_lzma, lzma_ret, lzma_stream, LZMA_BUF_ERROR, LZMA_OK, LZMA_PROG_ERROR, LZMA_STREAM_END};
use crate::internal::common::{all_supported_actions, LZMA_FINISH};
use crate::internal::filter::common::LZMA_FILTER_LZMA1;
use crate::internal::lzma::{io_error_to_ret, parse_filters, TerminalFilter};
use crate::internal::stream_state::{install_next_coder, NextCoder};

struct MicroEncoder {
    options: lzma_options_lzma,
    input: Vec<u8>,
    output: Vec<u8>,
    output_pos: usize,
}

struct MicroDecoder {
    comp_size: u64,
    uncomp_size: u64,
    uncomp_size_is_exact: bool,
    dict_size: u32,
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

unsafe fn microlzma_encoder_code(
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
    let coder = &mut *coder.cast::<MicroEncoder>();
    if coder.output_pos < coder.output.len() {
        return copy_output(&coder.output, &mut coder.output_pos, output, out_pos, out_size);
    }

    if action == LZMA_FINISH && out_size - *out_pos < 6 {
        return LZMA_PROG_ERROR;
    }

    if in_size != 0 {
        coder.input.extend_from_slice(core::slice::from_raw_parts(input, in_size));
        *in_pos = in_size;
    }

    if action != LZMA_FINISH {
        return LZMA_OK;
    }

    let filter_storage = [
        crate::ffi::types::lzma_filter {
            id: LZMA_FILTER_LZMA1,
            options: (&mut coder.options as *mut lzma_options_lzma).cast(),
        },
        crate::ffi::types::lzma_filter {
            id: crate::ffi::types::LZMA_VLI_UNKNOWN,
            options: ptr::null_mut(),
        },
    ];
    let rust_options = match parse_filters(filter_storage.as_ptr()) {
        Ok(chain) => match chain.terminal {
            TerminalFilter::Lzma1 { options, .. } => options,
            _ => return LZMA_PROG_ERROR,
        },
        Err(ret) => return ret,
    };

    let sink = Cursor::new(Vec::new());
    let mut writer = match lzma_rust2::LzmaWriter::new_no_header(sink, &rust_options, false) {
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
    if coder.output.is_empty() {
        return LZMA_PROG_ERROR;
    }
    coder.output[0] = !rust_options.get_props();
    coder.output_pos = 0;
    copy_output(&coder.output, &mut coder.output_pos, output, out_pos, out_size)
}

unsafe fn microlzma_decoder_code(
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
    let coder = &mut *coder.cast::<MicroDecoder>();
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

    if coder.input.len() as u64 != coder.comp_size {
        return LZMA_BUF_ERROR;
    }
    if coder.input.is_empty() {
        return LZMA_BUF_ERROR;
    }

    let props = !coder.input[0];
    let mut compressed = coder.input.clone();
    compressed[0] = 0;

    let consumed = if coder.uncomp_size_is_exact {
        let mut reader = match lzma_rust2::LzmaReader::new_with_props_strict(
            Cursor::new(compressed.as_slice()),
            coder.uncomp_size,
            props,
            coder.dict_size,
            None,
        ) {
            Ok(reader) => reader,
            Err(error) => return io_error_to_ret(&error),
        };

        if let Err(error) = reader.read_to_end(&mut coder.output) {
            let ret = io_error_to_ret(&error);
            if ret == crate::ffi::types::LZMA_DATA_ERROR {
                return LZMA_BUF_ERROR;
            }
            return ret;
        }

        reader.into_inner().position()
    } else {
        let max_guess = coder
            .uncomp_size
            .saturating_add(u64::from(coder.dict_size).max(coder.comp_size.saturating_mul(256)).max(64));
        let mut guess = coder.uncomp_size;

        loop {
            let mut trial_output = Vec::new();
            let mut reader = match lzma_rust2::LzmaReader::new_with_props(
                Cursor::new(compressed.as_slice()),
                guess,
                props,
                coder.dict_size,
                None,
            ) {
                Ok(reader) => reader,
                Err(error) => return io_error_to_ret(&error),
            };

            match reader.read_to_end(&mut trial_output) {
                Ok(_) => {
                    coder.output = trial_output;
                    break reader.into_inner().position();
                }
                Err(error) => {
                    let ret = io_error_to_ret(&error);
                    if matches!(ret, crate::ffi::types::LZMA_DATA_ERROR | LZMA_BUF_ERROR)
                        && guess < max_guess
                    {
                        guess = if guess < coder.uncomp_size.saturating_add(64) {
                            guess.saturating_add(1)
                        } else {
                            (guess.saturating_mul(2)).min(max_guess)
                        };
                        continue;
                    }

                    return ret;
                }
            }
        }
    };

    if coder.uncomp_size_is_exact {
        if coder.output.len() as u64 != coder.uncomp_size {
            return crate::ffi::types::LZMA_DATA_ERROR;
        }
        if consumed != coder.comp_size {
            return crate::ffi::types::LZMA_DATA_ERROR;
        }
    } else if (coder.output.len() as u64) < coder.uncomp_size {
        return crate::ffi::types::LZMA_DATA_ERROR;
    } else {
        coder.output.truncate(coder.uncomp_size as usize);
    }

    coder.decoded = true;
    coder.output_pos = 0;
    copy_output(&coder.output, &mut coder.output_pos, output, out_pos, out_size)
}

unsafe fn micro_encoder_end(coder: *mut c_void, _allocator: *const lzma_allocator) {
    drop(Box::from_raw(coder.cast::<MicroEncoder>()));
}

unsafe fn micro_decoder_end(coder: *mut c_void, _allocator: *const lzma_allocator) {
    drop(Box::from_raw(coder.cast::<MicroDecoder>()));
}

pub(crate) unsafe fn microlzma_encoder(strm: *mut lzma_stream, options: *const lzma_options_lzma) -> lzma_ret {
    if strm.is_null() || options.is_null() {
        return LZMA_PROG_ERROR;
    }

    install_next_coder(
        strm,
        NextCoder {
            coder: Box::into_raw(Box::new(MicroEncoder {
                options: *options,
                input: Vec::new(),
                output: Vec::new(),
                output_pos: 0,
            }))
            .cast(),
            code: microlzma_encoder_code,
            end: Some(micro_encoder_end),
            get_progress: None,
            get_check: None,
            memconfig: None,
        },
        all_supported_actions(),
    )
}

pub(crate) unsafe fn microlzma_decoder(
    strm: *mut lzma_stream,
    comp_size: u64,
    uncomp_size: u64,
    uncomp_size_is_exact: lzma_bool,
    dict_size: u32,
) -> lzma_ret {
    if strm.is_null() {
        return LZMA_PROG_ERROR;
    }

    install_next_coder(
        strm,
        NextCoder {
            coder: Box::into_raw(Box::new(MicroDecoder {
                comp_size,
                uncomp_size,
                uncomp_size_is_exact: uncomp_size_is_exact != 0,
                dict_size,
                input: Vec::new(),
                output: Vec::new(),
                output_pos: 0,
                decoded: false,
            }))
            .cast(),
            code: microlzma_decoder_code,
            end: Some(micro_decoder_end),
            get_progress: None,
            get_check: None,
            memconfig: None,
        },
        all_supported_actions(),
    )
}
