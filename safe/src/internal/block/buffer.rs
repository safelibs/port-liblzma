use crate::ffi::types::{lzma_allocator, lzma_block, lzma_ret};
use crate::internal::upstream;

pub(crate) unsafe fn block_buffer_bound(uncompressed_size: usize) -> usize {
    upstream::block_buffer_bound(uncompressed_size)
}

pub(crate) unsafe fn block_buffer_encode(
    block: *mut lzma_block,
    allocator: *const lzma_allocator,
    input: *const u8,
    input_size: usize,
    output: *mut u8,
    output_pos: *mut usize,
    output_size: usize,
) -> lzma_ret {
    upstream::block_buffer_encode(
        block,
        allocator,
        input,
        input_size,
        output,
        output_pos,
        output_size,
    )
}

pub(crate) unsafe fn block_buffer_decode(
    block: *mut lzma_block,
    allocator: *const lzma_allocator,
    input: *const u8,
    input_pos: *mut usize,
    input_size: usize,
    output: *mut u8,
    output_pos: *mut usize,
    output_size: usize,
) -> lzma_ret {
    upstream::block_buffer_decode(
        block,
        allocator,
        input,
        input_pos,
        input_size,
        output,
        output_pos,
        output_size,
    )
}

pub(crate) unsafe fn block_uncomp_encode(
    block: *mut lzma_block,
    input: *const u8,
    input_size: usize,
    output: *mut u8,
    output_pos: *mut usize,
    output_size: usize,
) -> lzma_ret {
    upstream::block_uncomp_encode(block, input, input_size, output, output_pos, output_size)
}
