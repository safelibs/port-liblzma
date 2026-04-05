use crate::ffi::types::{lzma_allocator, lzma_block, lzma_ret, lzma_vli};
use crate::internal::upstream;

pub(crate) unsafe fn block_header_size(block: *mut lzma_block) -> lzma_ret {
    upstream::block_header_size(block)
}

pub(crate) unsafe fn block_header_encode(block: *const lzma_block, output: *mut u8) -> lzma_ret {
    upstream::block_header_encode(block, output)
}

pub(crate) unsafe fn block_header_decode(
    block: *mut lzma_block,
    allocator: *const lzma_allocator,
    input: *const u8,
) -> lzma_ret {
    upstream::block_header_decode(block, allocator, input)
}

pub(crate) unsafe fn block_compressed_size(
    block: *mut lzma_block,
    unpadded_size: lzma_vli,
) -> lzma_ret {
    upstream::block_compressed_size(block, unpadded_size)
}

pub(crate) unsafe fn block_total_size(block: *const lzma_block) -> lzma_vli {
    upstream::block_total_size(block)
}

pub(crate) unsafe fn block_unpadded_size(block: *const lzma_block) -> lzma_vli {
    upstream::block_unpadded_size(block)
}
