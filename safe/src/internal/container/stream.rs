use core::ptr;

use crate::ffi::types::{
    lzma_check, lzma_filter, lzma_ret, lzma_stream, LZMA_OK, LZMA_PROG_ERROR, LZMA_STREAM_END,
};
use crate::internal::upstream;

pub(crate) const LZMA_TELL_NO_CHECK: u32 = 0x01;
pub(crate) const LZMA_TELL_UNSUPPORTED_CHECK: u32 = 0x02;
pub(crate) const LZMA_TELL_ANY_CHECK: u32 = 0x04;
pub(crate) const LZMA_CONCATENATED: u32 = 0x08;
pub(crate) const LZMA_IGNORE_CHECK: u32 = 0x10;
pub(crate) const LZMA_FAIL_FAST: u32 = 0x20;
pub(crate) const STREAM_DECODER_SUPPORTED_FLAGS: u32 = LZMA_TELL_NO_CHECK
    | LZMA_TELL_UNSUPPORTED_CHECK
    | LZMA_TELL_ANY_CHECK
    | LZMA_CONCATENATED
    | LZMA_IGNORE_CHECK
    | LZMA_FAIL_FAST;

pub(crate) unsafe fn copy_output_buffer(
    buffer: &[u8],
    state_pos: &mut usize,
    output: *mut u8,
    out_pos: *mut usize,
    out_size: usize,
) -> lzma_ret {
    let copy_size = (buffer.len() - *state_pos).min(out_size - *out_pos);
    ptr::copy_nonoverlapping(
        buffer.as_ptr().add(*state_pos),
        output.add(*out_pos),
        copy_size,
    );
    *state_pos += copy_size;
    *out_pos += copy_size;
    if *state_pos == buffer.len() {
        LZMA_STREAM_END
    } else {
        LZMA_OK
    }
}

pub(crate) unsafe fn stream_encoder(
    strm: *mut lzma_stream,
    filters: *const lzma_filter,
    check: lzma_check,
) -> lzma_ret {
    upstream::stream_encoder(strm, filters, check)
}

pub(crate) unsafe fn stream_decoder(strm: *mut lzma_stream, memlimit: u64, flags: u32) -> lzma_ret {
    upstream::stream_decoder(strm, memlimit, flags)
}

pub(crate) unsafe fn filters_update(
    strm: *mut lzma_stream,
    filters: *const lzma_filter,
) -> lzma_ret {
    if strm.is_null() || filters.is_null() {
        return LZMA_PROG_ERROR;
    }

    upstream::filters_update(strm, filters)
}
