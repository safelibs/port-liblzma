use core::{mem, ptr};

use crate::ffi::types::{
    lzma_allocator, lzma_block, lzma_check, lzma_filter, lzma_ret, lzma_stream_flags,
    LZMA_BUF_ERROR, LZMA_MEM_ERROR, LZMA_OK, LZMA_PROG_ERROR, LZMA_STREAM_END,
    LZMA_UNSUPPORTED_CHECK,
};
use crate::internal::{
    block, check,
    common::{LZMA_VLI_BYTES_MAX, LZMA_VLI_MAX},
    index,
    stream_flags::{self, LZMA_STREAM_HEADER_SIZE},
    stream_state::{lzma_code_impl, lzma_end_impl, lzma_memusage_impl},
};

use super::stream::{self, LZMA_TELL_ANY_CHECK};

const INDEX_BOUND: usize = (1 + 1 + 2 * LZMA_VLI_BYTES_MAX + 4 + 3) & !3;
const HEADERS_BOUND: usize = 2 * LZMA_STREAM_HEADER_SIZE + INDEX_BOUND;

pub(crate) unsafe fn stream_buffer_bound(uncompressed_size: usize) -> usize {
    let block_bound = block::block_buffer_bound(uncompressed_size);
    if block_bound == 0 {
        return 0;
    }

    let size_limit = usize::min(usize::MAX, LZMA_VLI_MAX as usize);
    if size_limit - block_bound < HEADERS_BOUND {
        return 0;
    }

    block_bound + HEADERS_BOUND
}

pub(crate) unsafe fn stream_buffer_encode(
    filters: *mut lzma_filter,
    check_id: lzma_check,
    allocator: *const lzma_allocator,
    input: *const u8,
    input_size: usize,
    output: *mut u8,
    output_pos: *mut usize,
    output_size: usize,
) -> lzma_ret {
    if filters.is_null()
        || (input.is_null() && input_size != 0)
        || output.is_null()
        || output_pos.is_null()
        || *output_pos > output_size
    {
        return LZMA_PROG_ERROR;
    }

    if check::check_is_supported(check_id) == 0 {
        return LZMA_UNSUPPORTED_CHECK;
    }

    let mut out_pos = *output_pos;
    if output_size - out_pos <= 2 * LZMA_STREAM_HEADER_SIZE {
        return LZMA_BUF_ERROR;
    }
    let block_out_size = output_size - LZMA_STREAM_HEADER_SIZE;

    let mut stream_opts: lzma_stream_flags = mem::zeroed();
    stream_opts.version = 0;
    stream_opts.check = check_id;

    let ret = stream_flags::stream_header_encode_impl(&stream_opts, output.add(out_pos));
    if ret != LZMA_OK {
        return LZMA_PROG_ERROR;
    }
    out_pos += LZMA_STREAM_HEADER_SIZE;

    let mut block_opts: lzma_block = mem::zeroed();
    block_opts.version = 0;
    block_opts.check = check_id;
    block_opts.filters = filters;

    if input_size > 0 {
        let ret = block::block_buffer_encode(
            &mut block_opts,
            allocator,
            input,
            input_size,
            output,
            &mut out_pos,
            block_out_size,
        );
        if ret != LZMA_OK {
            return ret;
        }
    }

    let index_ptr = index::index_init(allocator);
    if index_ptr.is_null() {
        return LZMA_MEM_ERROR;
    }

    let mut ret = LZMA_OK;
    if input_size > 0 {
        ret = index::index_append(
            index_ptr,
            allocator,
            block::block_unpadded_size(&block_opts),
            block_opts.uncompressed_size,
        );
    }

    if ret == LZMA_OK {
        ret = index::index_buffer_encode(index_ptr, output, &mut out_pos, block_out_size);
        stream_opts.backward_size = index::index_size(index_ptr);
    }
    index::index_end(index_ptr, allocator);
    if ret != LZMA_OK {
        return ret;
    }

    let ret = stream_flags::stream_footer_encode_impl(&stream_opts, output.add(out_pos));
    if ret != LZMA_OK {
        return LZMA_PROG_ERROR;
    }

    out_pos += LZMA_STREAM_HEADER_SIZE;
    *output_pos = out_pos;
    LZMA_OK
}

pub(crate) unsafe fn stream_buffer_decode(
    memlimit: *mut u64,
    flags: u32,
    allocator: *const lzma_allocator,
    input: *const u8,
    input_pos: *mut usize,
    input_size: usize,
    output: *mut u8,
    output_pos: *mut usize,
    output_size: usize,
) -> lzma_ret {
    if memlimit.is_null()
        || input_pos.is_null()
        || (input.is_null() && *input_pos != input_size)
        || *input_pos > input_size
        || output_pos.is_null()
        || (output.is_null() && *output_pos != output_size)
        || *output_pos > output_size
    {
        return LZMA_PROG_ERROR;
    }

    if (flags & LZMA_TELL_ANY_CHECK) != 0 {
        return LZMA_PROG_ERROR;
    }

    let mut strm = crate::ffi::types::LZMA_STREAM_INIT;
    strm.allocator = allocator;

    let ret = stream::stream_decoder(&mut strm, *memlimit, flags);
    if ret != LZMA_OK {
        lzma_end_impl(&mut strm);
        return ret;
    }

    let in_start = *input_pos;
    let out_start = *output_pos;
    let in_remaining = input_size - *input_pos;
    let out_remaining = output_size - *output_pos;

    strm.next_in = if in_remaining == 0 {
        ptr::null()
    } else {
        input.add(*input_pos)
    };
    strm.avail_in = in_remaining;
    strm.next_out = if out_remaining == 0 {
        ptr::null_mut()
    } else {
        output.add(*output_pos)
    };
    strm.avail_out = out_remaining;

    let ret = lzma_code_impl(&mut strm, crate::internal::common::LZMA_FINISH);
    let consumed = in_remaining - strm.avail_in;
    let produced = out_remaining - strm.avail_out;

    let final_ret = if ret == LZMA_STREAM_END {
        *input_pos += consumed;
        *output_pos += produced;
        LZMA_OK
    } else {
        *input_pos = in_start;
        *output_pos = out_start;

        if ret == LZMA_OK {
            if consumed == in_remaining {
                crate::ffi::types::LZMA_DATA_ERROR
            } else {
                LZMA_BUF_ERROR
            }
        } else {
            if ret == crate::ffi::types::LZMA_MEMLIMIT_ERROR {
                *memlimit = lzma_memusage_impl(&strm);
            }
            ret
        }
    };

    lzma_end_impl(&mut strm);
    final_ret
}
