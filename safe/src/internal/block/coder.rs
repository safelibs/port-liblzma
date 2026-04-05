use crate::ffi::types::{lzma_block, lzma_ret, lzma_stream};
use crate::internal::upstream;

pub(crate) unsafe fn block_encoder(strm: *mut lzma_stream, block: *mut lzma_block) -> lzma_ret {
    upstream::block_encoder(strm, block)
}

pub(crate) unsafe fn block_decoder(strm: *mut lzma_stream, block: *mut lzma_block) -> lzma_ret {
    upstream::block_decoder(strm, block)
}
