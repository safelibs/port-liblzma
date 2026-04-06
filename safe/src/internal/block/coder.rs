use core::{ffi::c_void, mem, ptr};

use crate::ffi::types::{
    lzma_action, lzma_allocator, lzma_block, lzma_ret, lzma_stream, LZMA_DATA_ERROR, LZMA_OK,
    LZMA_OPTIONS_ERROR, LZMA_PROG_ERROR, LZMA_STREAM_END, LZMA_STREAM_INIT, LZMA_UNSUPPORTED_CHECK,
    LZMA_VLI_UNKNOWN,
};
use crate::internal::{
    check::{self, CheckState},
    common::{
        ACTION_COUNT, LZMA_CHECK_ID_MAX, LZMA_FINISH, LZMA_RUN, LZMA_SYNC_FLUSH, LZMA_VLI_MAX,
    },
    stream_state::{install_next_coder, lzma_code_impl, lzma_end_impl, NextCoder},
    upstream,
};

enum BlockSequence {
    Code,
    Padding,
    Check,
}

struct BlockEncoderState {
    inner: lzma_stream,
    sequence: BlockSequence,
    compressed_size: u64,
    uncompressed_size: u64,
    check: CheckState,
    check_buf: [u8; 64],
    check_pos: usize,
}

struct BlockDecoderState {
    inner: lzma_stream,
    sequence: BlockSequence,
    compressed_size: u64,
    uncompressed_size: u64,
    compressed_limit: u64,
    uncompressed_limit: u64,
    check: Option<CheckState>,
    check_buf: [u8; 64],
    check_pos: usize,
    verify_check: bool,
}

enum BlockCoderState {
    Encoder(BlockEncoderState),
    Decoder(BlockDecoderState),
}

struct BlockCoder {
    block: *mut lzma_block,
    state: BlockCoderState,
}

const fn block_encoder_actions() -> [bool; ACTION_COUNT] {
    let mut actions = [false; ACTION_COUNT];
    actions[LZMA_RUN as usize] = true;
    actions[LZMA_SYNC_FLUSH as usize] = true;
    actions[LZMA_FINISH as usize] = true;
    actions
}

const fn block_decoder_actions() -> [bool; ACTION_COUNT] {
    let mut actions = [false; ACTION_COUNT];
    actions[LZMA_RUN as usize] = true;
    actions[LZMA_FINISH as usize] = true;
    actions
}

const fn vli_is_valid(vli: u64) -> bool {
    vli <= LZMA_VLI_MAX || vli == LZMA_VLI_UNKNOWN
}

unsafe fn block_encode(
    coder: &mut BlockCoder,
    input: *const u8,
    in_pos: *mut usize,
    in_size: usize,
    output: *mut u8,
    out_pos: *mut usize,
    out_size: usize,
    action: lzma_action,
) -> lzma_ret {
    let block = &mut *coder.block;
    let BlockCoderState::Encoder(state) = &mut coder.state else {
        return LZMA_PROG_ERROR;
    };

    match state.sequence {
        BlockSequence::Code => {
            let avail_in = in_size - *in_pos;
            if LZMA_VLI_MAX - state.uncompressed_size < avail_in as u64 {
                return LZMA_DATA_ERROR;
            }

            let avail_out = out_size - *out_pos;
            state.inner.next_in = if avail_in == 0 {
                ptr::null()
            } else {
                input.add(*in_pos)
            };
            state.inner.avail_in = avail_in;
            state.inner.next_out = if avail_out == 0 {
                ptr::null_mut()
            } else {
                output.add(*out_pos)
            };
            state.inner.avail_out = avail_out;

            let in_start = *in_pos;
            let out_start = *out_pos;
            let ret = lzma_code_impl(&mut state.inner, action);

            let in_used = avail_in - state.inner.avail_in;
            let out_used = avail_out - state.inner.avail_out;
            *in_pos += in_used;
            *out_pos += out_used;

            state.compressed_size = match state.compressed_size.checked_add(out_used as u64) {
                Some(size) if size <= LZMA_VLI_MAX => size,
                _ => return LZMA_DATA_ERROR,
            };
            state.uncompressed_size += in_used as u64;

            if in_used > 0 {
                state
                    .check
                    .update(core::slice::from_raw_parts(input.add(in_start), in_used));
            }

            if ret != LZMA_STREAM_END || action == LZMA_SYNC_FLUSH {
                return ret;
            }

            block.compressed_size = state.compressed_size;
            block.uncompressed_size = state.uncompressed_size;
            state.sequence = BlockSequence::Padding;

            debug_assert_eq!(*in_pos, in_size);
            debug_assert_eq!(action, LZMA_FINISH);
            let _ = out_start;
        }
        BlockSequence::Padding | BlockSequence::Check => {}
    }

    match state.sequence {
        BlockSequence::Padding => {
            while state.compressed_size & 3 != 0 {
                if *out_pos >= out_size {
                    return LZMA_OK;
                }
                *output.add(*out_pos) = 0;
                *out_pos += 1;
                state.compressed_size += 1;
            }

            if block.check == crate::ffi::types::LZMA_CHECK_NONE {
                return LZMA_STREAM_END;
            }

            state.check_buf = mem::replace(&mut state.check, CheckState::None).finish();
            state.sequence = BlockSequence::Check;
        }
        BlockSequence::Code | BlockSequence::Check => {}
    }

    match state.sequence {
        BlockSequence::Check => {
            let check_size = check::check_size(block.check) as usize;
            let copy_size = (check_size - state.check_pos).min(out_size - *out_pos);
            ptr::copy_nonoverlapping(
                state.check_buf.as_ptr().add(state.check_pos),
                output.add(*out_pos),
                copy_size,
            );
            state.check_pos += copy_size;
            *out_pos += copy_size;
            if state.check_pos < check_size {
                return LZMA_OK;
            }

            block.raw_check[..check_size].copy_from_slice(&state.check_buf[..check_size]);
            LZMA_STREAM_END
        }
        BlockSequence::Code | BlockSequence::Padding => LZMA_PROG_ERROR,
    }
}

unsafe fn block_decode(
    coder: &mut BlockCoder,
    input: *const u8,
    in_pos: *mut usize,
    in_size: usize,
    output: *mut u8,
    out_pos: *mut usize,
    out_size: usize,
    action: lzma_action,
) -> lzma_ret {
    let block = &mut *coder.block;
    let BlockCoderState::Decoder(state) = &mut coder.state else {
        return LZMA_PROG_ERROR;
    };

    match state.sequence {
        BlockSequence::Code => {
            let in_remaining = in_size - *in_pos;
            let out_remaining = out_size - *out_pos;
            let in_limit =
                (state.compressed_limit - state.compressed_size).min(in_remaining as u64) as usize;
            let out_limit = (state.uncompressed_limit - state.uncompressed_size)
                .min(out_remaining as u64) as usize;

            state.inner.next_in = if in_limit == 0 {
                ptr::null()
            } else {
                input.add(*in_pos)
            };
            state.inner.avail_in = in_limit;
            state.inner.next_out = if out_limit == 0 {
                ptr::null_mut()
            } else {
                output.add(*out_pos)
            };
            state.inner.avail_out = out_limit;

            let out_start = *out_pos;
            let ret = lzma_code_impl(&mut state.inner, action);

            let in_used = in_limit - state.inner.avail_in;
            let out_used = out_limit - state.inner.avail_out;
            *in_pos += in_used;
            *out_pos += out_used;
            state.compressed_size += in_used as u64;
            state.uncompressed_size += out_used as u64;

            if let Some(check) = &mut state.check {
                if out_used > 0 {
                    check.update(core::slice::from_raw_parts(output.add(out_start), out_used));
                }
            }

            if ret == LZMA_OK {
                let comp_done = state.compressed_size == block.compressed_size;
                let uncomp_done = state.uncompressed_size == block.uncompressed_size;
                if comp_done && uncomp_done {
                    return LZMA_DATA_ERROR;
                }
                if comp_done && *out_pos < out_size {
                    return LZMA_DATA_ERROR;
                }
                if uncomp_done && *in_pos < in_size {
                    return LZMA_DATA_ERROR;
                }
            }

            if ret != LZMA_STREAM_END {
                return ret;
            }

            if (block.compressed_size != LZMA_VLI_UNKNOWN
                && block.compressed_size != state.compressed_size)
                || (block.uncompressed_size != LZMA_VLI_UNKNOWN
                    && block.uncompressed_size != state.uncompressed_size)
            {
                return LZMA_DATA_ERROR;
            }

            block.compressed_size = state.compressed_size;
            block.uncompressed_size = state.uncompressed_size;
            state.sequence = BlockSequence::Padding;
        }
        BlockSequence::Padding | BlockSequence::Check => {}
    }

    match state.sequence {
        BlockSequence::Padding => {
            while state.compressed_size & 3 != 0 {
                if *in_pos >= in_size {
                    return LZMA_OK;
                }
                state.compressed_size += 1;
                if *input.add(*in_pos) != 0 {
                    return LZMA_DATA_ERROR;
                }
                *in_pos += 1;
            }

            if block.check == crate::ffi::types::LZMA_CHECK_NONE {
                return LZMA_STREAM_END;
            }

            if let Some(check) = state.check.take() {
                state.check_buf = check.finish();
            }
            state.sequence = BlockSequence::Check;
        }
        BlockSequence::Code | BlockSequence::Check => {}
    }

    match state.sequence {
        BlockSequence::Check => {
            let check_size = check::check_size(block.check) as usize;
            let copy_size = (check_size - state.check_pos).min(in_size - *in_pos);
            ptr::copy_nonoverlapping(
                input.add(*in_pos),
                block.raw_check.as_mut_ptr().add(state.check_pos),
                copy_size,
            );
            state.check_pos += copy_size;
            *in_pos += copy_size;
            if state.check_pos < check_size {
                return LZMA_OK;
            }

            if state.verify_check && block.raw_check[..check_size] != state.check_buf[..check_size]
            {
                return LZMA_DATA_ERROR;
            }

            LZMA_STREAM_END
        }
        BlockSequence::Code | BlockSequence::Padding => LZMA_PROG_ERROR,
    }
}

unsafe fn block_code(
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
    let coder = &mut *coder.cast::<BlockCoder>();
    match coder.state {
        BlockCoderState::Encoder(_) => block_encode(
            coder, input, in_pos, in_size, output, out_pos, out_size, action,
        ),
        BlockCoderState::Decoder(_) => block_decode(
            coder, input, in_pos, in_size, output, out_pos, out_size, action,
        ),
    }
}

unsafe fn block_end(coder: *mut c_void, _allocator: *const lzma_allocator) {
    let mut coder = Box::from_raw(coder.cast::<BlockCoder>());
    match &mut coder.state {
        BlockCoderState::Encoder(state) => lzma_end_impl(&mut state.inner),
        BlockCoderState::Decoder(state) => lzma_end_impl(&mut state.inner),
    }
}

unsafe fn make_encoder_coder(
    allocator: *const lzma_allocator,
    block: *mut lzma_block,
) -> Result<BlockCoder, lzma_ret> {
    if (*block).version > 1 {
        return Err(LZMA_OPTIONS_ERROR);
    }
    if (*block).check < 0 || (*block).check as usize > LZMA_CHECK_ID_MAX {
        return Err(LZMA_PROG_ERROR);
    }
    if check::check_is_supported((*block).check) == 0 {
        return Err(LZMA_UNSUPPORTED_CHECK);
    }

    let mut inner = LZMA_STREAM_INIT;
    inner.allocator = allocator;
    let ret = upstream::raw_encoder(&mut inner, (*block).filters.cast_const());
    if ret != LZMA_OK {
        return Err(ret);
    }

    Ok(BlockCoder {
        block,
        state: BlockCoderState::Encoder(BlockEncoderState {
            inner,
            sequence: BlockSequence::Code,
            compressed_size: 0,
            uncompressed_size: 0,
            check: CheckState::new((*block).check).ok_or(LZMA_OPTIONS_ERROR)?,
            check_buf: [0; 64],
            check_pos: 0,
        }),
    })
}

unsafe fn make_decoder_coder(
    allocator: *const lzma_allocator,
    block: *mut lzma_block,
) -> Result<BlockCoder, lzma_ret> {
    if super::header::block_unpadded_size(block.cast_const()) == 0
        || !vli_is_valid((*block).uncompressed_size)
    {
        return Err(LZMA_PROG_ERROR);
    }

    let mut inner = LZMA_STREAM_INIT;
    inner.allocator = allocator;
    let ret = upstream::raw_decoder(&mut inner, (*block).filters.cast_const());
    if ret != LZMA_OK {
        return Err(ret);
    }

    let ignore_check = (*block).version >= 1 && (*block).ignore_check != 0;
    let verify_check = !ignore_check && check::check_is_supported((*block).check) != 0;
    let compressed_limit = if (*block).compressed_size == LZMA_VLI_UNKNOWN {
        (LZMA_VLI_MAX & !3)
            .saturating_sub((*block).header_size as u64)
            .saturating_sub(check::check_size((*block).check) as u64)
    } else {
        (*block).compressed_size
    };
    let uncompressed_limit = if (*block).uncompressed_size == LZMA_VLI_UNKNOWN {
        LZMA_VLI_MAX
    } else {
        (*block).uncompressed_size
    };

    Ok(BlockCoder {
        block,
        state: BlockCoderState::Decoder(BlockDecoderState {
            inner,
            sequence: BlockSequence::Code,
            compressed_size: 0,
            uncompressed_size: 0,
            compressed_limit,
            uncompressed_limit,
            check: if verify_check {
                CheckState::new((*block).check)
            } else {
                None
            },
            check_buf: [0; 64],
            check_pos: 0,
            verify_check,
        }),
    })
}

unsafe fn install_block_coder(
    strm: *mut lzma_stream,
    block: *mut lzma_block,
    encode: bool,
) -> lzma_ret {
    if strm.is_null() || block.is_null() {
        return LZMA_PROG_ERROR;
    }

    let coder = if encode {
        make_encoder_coder((*strm).allocator, block)
    } else {
        make_decoder_coder((*strm).allocator, block)
    };
    let coder = match coder {
        Ok(coder) => Box::new(coder),
        Err(ret) => return ret,
    };

    let raw = Box::into_raw(coder);
    let ret = install_next_coder(
        strm,
        NextCoder {
            coder: raw.cast(),
            code: block_code,
            end: Some(block_end),
            get_progress: None,
            get_check: None,
            memconfig: None,
            update: None,
        },
        if encode {
            block_encoder_actions()
        } else {
            block_decoder_actions()
        },
    );
    if ret != LZMA_OK {
        block_end(raw.cast(), ptr::null());
    }
    ret
}

pub(crate) unsafe fn block_encoder(strm: *mut lzma_stream, block: *mut lzma_block) -> lzma_ret {
    install_block_coder(strm, block, true)
}

pub(crate) unsafe fn block_decoder(strm: *mut lzma_stream, block: *mut lzma_block) -> lzma_ret {
    install_block_coder(strm, block, false)
}

#[cfg(test)]
mod tests {
    use core::{mem, ptr};

    use super::*;
    use crate::ffi::types::{lzma_filter, lzma_options_lzma, LZMA_STREAM_INIT};
    use crate::internal::{
        common::LZMA_CHECK_CRC32,
        filter::common::LZMA_FILTER_LZMA2,
        preset,
        stream_state::{lzma_code_impl, lzma_end_impl},
    };

    unsafe fn lzma2_filters(
        options: &mut lzma_options_lzma,
    ) -> [lzma_filter; crate::ffi::types::LZMA_FILTERS_MAX + 1] {
        assert_eq!(preset::lzma_lzma_preset_impl(options, 6), 0);
        [
            lzma_filter {
                id: LZMA_FILTER_LZMA2,
                options: (options as *mut lzma_options_lzma).cast(),
            },
            lzma_filter {
                id: LZMA_VLI_UNKNOWN,
                options: ptr::null_mut(),
            },
            lzma_filter {
                id: LZMA_VLI_UNKNOWN,
                options: ptr::null_mut(),
            },
            lzma_filter {
                id: LZMA_VLI_UNKNOWN,
                options: ptr::null_mut(),
            },
            lzma_filter {
                id: LZMA_VLI_UNKNOWN,
                options: ptr::null_mut(),
            },
        ]
    }

    unsafe fn collect(
        strm: &mut lzma_stream,
        action: lzma_action,
        chunk: usize,
    ) -> (lzma_ret, Vec<u8>) {
        let mut output = Vec::new();
        loop {
            let mut buffer = vec![0u8; chunk];
            strm.next_out = buffer.as_mut_ptr();
            strm.avail_out = buffer.len();
            let ret = lzma_code_impl(strm, action);
            let written = buffer.len() - strm.avail_out;
            output.extend_from_slice(&buffer[..written]);

            if ret == LZMA_STREAM_END {
                return (ret, output);
            }
            if ret != LZMA_OK {
                return (ret, output);
            }
            if action == LZMA_RUN && strm.avail_in == 0 {
                return (ret, output);
            }
        }
    }

    #[test]
    fn block_decoder_streams_large_expansion() {
        let input = vec![0u8; 512 * 1024];

        unsafe {
            let mut options: lzma_options_lzma = mem::zeroed();
            let mut filters = lzma2_filters(&mut options);
            let mut block: lzma_block = mem::zeroed();
            block.version = 1;
            block.check = LZMA_CHECK_CRC32;
            block.compressed_size = LZMA_VLI_UNKNOWN;
            block.uncompressed_size = LZMA_VLI_UNKNOWN;
            block.filters = filters.as_mut_ptr();
            assert_eq!(super::super::header::block_header_size(&mut block), LZMA_OK);

            let mut encode = LZMA_STREAM_INIT;
            assert_eq!(block_encoder(&mut encode, &mut block), LZMA_OK);
            encode.next_in = input.as_ptr();
            encode.avail_in = input.len();
            let (ret, body) = collect(&mut encode, LZMA_FINISH, 4096);
            assert_eq!(ret, LZMA_STREAM_END);
            lzma_end_impl(&mut encode);
            assert!(block.compressed_size.saturating_mul(64) < input.len() as u64);

            let mut decode = LZMA_STREAM_INIT;
            assert_eq!(block_decoder(&mut decode, &mut block), LZMA_OK);
            decode.next_in = body.as_ptr();
            decode.avail_in = body.len();
            let (ret, decoded) = collect(&mut decode, LZMA_FINISH, 4096);
            assert_eq!(ret, LZMA_STREAM_END);
            assert_eq!(decoded, input);
            lzma_end_impl(&mut decode);
        }
    }

    #[test]
    fn block_encoder_sync_flush_emits_body_bytes() {
        let part1 = vec![b'X'; 12 * 1024];
        let part2 = vec![b'Y'; 8 * 1024];

        unsafe {
            let mut options: lzma_options_lzma = mem::zeroed();
            let mut filters = lzma2_filters(&mut options);
            let mut block: lzma_block = mem::zeroed();
            block.version = 1;
            block.check = LZMA_CHECK_CRC32;
            block.compressed_size = LZMA_VLI_UNKNOWN;
            block.uncompressed_size = LZMA_VLI_UNKNOWN;
            block.filters = filters.as_mut_ptr();
            assert_eq!(super::super::header::block_header_size(&mut block), LZMA_OK);

            let mut encode = LZMA_STREAM_INIT;
            assert_eq!(block_encoder(&mut encode, &mut block), LZMA_OK);

            encode.next_in = part1.as_ptr();
            encode.avail_in = part1.len();
            let (_, mut body) = collect(&mut encode, LZMA_RUN, 257);

            encode.next_in = ptr::null();
            encode.avail_in = 0;
            let (ret, flush_bytes) = collect(&mut encode, LZMA_SYNC_FLUSH, 257);
            assert_eq!(ret, LZMA_STREAM_END);
            assert!(!flush_bytes.is_empty());
            body.extend_from_slice(&flush_bytes);

            encode.next_in = part2.as_ptr();
            encode.avail_in = part2.len();
            let (ret, finish_bytes) = collect(&mut encode, LZMA_FINISH, 257);
            assert_eq!(ret, LZMA_STREAM_END);
            body.extend_from_slice(&finish_bytes);
            lzma_end_impl(&mut encode);

            let mut decode = LZMA_STREAM_INIT;
            assert_eq!(block_decoder(&mut decode, &mut block), LZMA_OK);
            decode.next_in = body.as_ptr();
            decode.avail_in = body.len();
            let (ret, decoded) = collect(&mut decode, LZMA_FINISH, 1024);
            assert_eq!(ret, LZMA_STREAM_END);
            assert_eq!(decoded, [part1, part2].concat());
            lzma_end_impl(&mut decode);
        }
    }
}
