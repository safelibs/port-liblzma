use crate::ffi::types::{lzma_options_lzma, lzma_ret, lzma_stream};
use crate::internal::upstream;

pub(crate) unsafe fn alone_encoder(
    strm: *mut lzma_stream,
    options: *const lzma_options_lzma,
) -> lzma_ret {
    upstream::alone_encoder(strm, options)
}

pub(crate) unsafe fn alone_decoder(strm: *mut lzma_stream, memlimit: u64) -> lzma_ret {
    upstream::alone_decoder(strm, memlimit)
}
