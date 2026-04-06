use core::ffi::c_void;
use core::ptr;

use crate::ffi::types::{
    lzma_action, lzma_allocator, lzma_block, lzma_ret, lzma_stream, LZMA_OK, LZMA_PROG_ERROR,
    LZMA_STREAM_END,
};
use crate::internal::common::{all_supported_actions, LZMA_FINISH};
use crate::internal::stream_state::{install_next_coder, NextCoder};

struct BlockCoder {
    block: lzma_block,
    encode: bool,
    input: Vec<u8>,
    output: Vec<u8>,
    output_pos: usize,
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

    if coder.output_pos < coder.output.len() {
        let copy_size = (coder.output.len() - coder.output_pos).min(out_size - *out_pos);
        ptr::copy_nonoverlapping(
            coder.output.as_ptr().add(coder.output_pos),
            output.add(*out_pos),
            copy_size,
        );
        coder.output_pos += copy_size;
        *out_pos += copy_size;
        return if coder.output_pos == coder.output.len() {
            LZMA_STREAM_END
        } else {
            LZMA_OK
        };
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

    let mut out_vec = vec![0u8; crate::internal::block::buffer::block_buffer_bound(coder.input.len())];
    let mut out_pos_local = 0usize;
    let mut block = coder.block;
    let ret = if coder.encode {
        super::buffer::block_buffer_encode(
            &mut block,
            ptr::null(),
            coder.input.as_ptr(),
            coder.input.len(),
            out_vec.as_mut_ptr(),
            &mut out_pos_local,
            out_vec.len(),
        )
    } else {
        let mut in_pos_local = block.header_size as usize;
        let mut decoded = vec![0u8; coder.input.len().saturating_mul(64).max(4096)];
        let mut decoded_pos = 0usize;
        let ret = super::buffer::block_buffer_decode(
            &mut block,
            ptr::null(),
            coder.input.as_ptr(),
            &mut in_pos_local,
            coder.input.len(),
            decoded.as_mut_ptr(),
            &mut decoded_pos,
            decoded.len(),
        );
        if ret == LZMA_OK {
            coder.output = decoded[..decoded_pos].to_vec();
        }
        ret
    };

    if coder.encode && ret == crate::ffi::types::LZMA_OK {
        coder.output = out_vec[..out_pos_local].to_vec();
    }

    if ret != crate::ffi::types::LZMA_OK {
        return ret;
    }

    coder.output_pos = 0;
    block_code(
        coder as *mut BlockCoder as *mut c_void,
        ptr::null(),
        ptr::null(),
        in_pos,
        0,
        output,
        out_pos,
        out_size,
        action,
    )
}

unsafe fn block_end(coder: *mut c_void, _allocator: *const lzma_allocator) {
    drop(Box::from_raw(coder.cast::<BlockCoder>()));
}

unsafe fn install_block_coder(strm: *mut lzma_stream, block: *mut lzma_block, encode: bool) -> lzma_ret {
    if strm.is_null() || block.is_null() {
        return LZMA_PROG_ERROR;
    }

    let coder = Box::new(BlockCoder {
        block: *block,
        encode,
        input: Vec::new(),
        output: Vec::new(),
        output_pos: 0,
    });

    install_next_coder(
        strm,
        NextCoder {
            coder: Box::into_raw(coder).cast(),
            code: block_code,
            end: Some(block_end),
            get_progress: None,
            get_check: None,
            memconfig: None,
        },
        all_supported_actions(),
    )
}

pub(crate) unsafe fn block_encoder(strm: *mut lzma_stream, block: *mut lzma_block) -> lzma_ret {
    install_block_coder(strm, block, true)
}

pub(crate) unsafe fn block_decoder(strm: *mut lzma_stream, block: *mut lzma_block) -> lzma_ret {
    install_block_coder(strm, block, false)
}
