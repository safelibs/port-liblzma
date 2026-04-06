use core::ptr;

use crate::ffi::types::{
    lzma_allocator, lzma_block, lzma_ret, lzma_vli, LZMA_DATA_ERROR, LZMA_OK, LZMA_OPTIONS_ERROR,
    LZMA_PROG_ERROR, LZMA_VLI_UNKNOWN,
};
use crate::internal::check;
use crate::internal::common::{LZMA_CHECK_ID_MAX, LZMA_VLI_MAX};
use crate::internal::filter::{
    self, filter_flags_decode_impl, filter_flags_encode_impl, filter_flags_size_impl,
};
use crate::internal::index::core::{UNPADDED_SIZE_MAX, UNPADDED_SIZE_MIN};
use crate::internal::vli::{lzma_vli_decode_impl, lzma_vli_encode_impl, lzma_vli_size_impl};

pub(crate) const LZMA_BLOCK_HEADER_SIZE_MIN: u32 = 8;
pub(crate) const LZMA_BLOCK_HEADER_SIZE_MAX: u32 = 1024;

#[inline]
fn read32le(input: *const u8) -> u32 {
    unsafe {
        u32::from(*input.add(0))
            | (u32::from(*input.add(1)) << 8)
            | (u32::from(*input.add(2)) << 16)
            | (u32::from(*input.add(3)) << 24)
    }
}

#[inline]
fn write32le(output: *mut u8, value: u32) {
    unsafe {
        *output.add(0) = value as u8;
        *output.add(1) = (value >> 8) as u8;
        *output.add(2) = (value >> 16) as u8;
        *output.add(3) = (value >> 24) as u8;
    }
}

#[inline]
fn vli_is_valid(vli: lzma_vli) -> bool {
    vli <= LZMA_VLI_MAX || vli == LZMA_VLI_UNKNOWN
}

pub(crate) unsafe fn block_header_size(block: *mut lzma_block) -> lzma_ret {
    if block.is_null() {
        return LZMA_PROG_ERROR;
    }

    if (*block).version > 1 {
        return LZMA_OPTIONS_ERROR;
    }

    let mut size = 1u32 + 1 + 4;

    if (*block).compressed_size != LZMA_VLI_UNKNOWN {
        let add = lzma_vli_size_impl((*block).compressed_size);
        if add == 0 || (*block).compressed_size == 0 {
            return LZMA_PROG_ERROR;
        }
        size += add;
    }

    if (*block).uncompressed_size != LZMA_VLI_UNKNOWN {
        let add = lzma_vli_size_impl((*block).uncompressed_size);
        if add == 0 {
            return LZMA_PROG_ERROR;
        }
        size += add;
    }

    if (*block).filters.is_null() || (*(*block).filters).id == LZMA_VLI_UNKNOWN {
        return LZMA_PROG_ERROR;
    }

    for i in 0.. {
        let filter = (*block).filters.add(i);
        if (*filter).id == LZMA_VLI_UNKNOWN {
            break;
        }

        if i == crate::ffi::types::LZMA_FILTERS_MAX {
            return LZMA_PROG_ERROR;
        }

        let mut add = 0u32;
        let ret = filter_flags_size_impl(&mut add, filter);
        if ret != LZMA_OK {
            return ret;
        }

        size += add;
    }

    (*block).header_size = (size + 3) & !3;
    LZMA_OK
}

pub(crate) unsafe fn block_header_encode(block: *const lzma_block, output: *mut u8) -> lzma_ret {
    if block.is_null() || output.is_null() {
        return LZMA_PROG_ERROR;
    }

    if block_unpadded_size(block) == 0 || !vli_is_valid((*block).uncompressed_size) {
        return LZMA_PROG_ERROR;
    }

    let out_size = (*block).header_size.saturating_sub(4) as usize;
    *output = (out_size / 4) as u8;
    *output.add(1) = 0;
    let mut out_pos = 2usize;

    if (*block).compressed_size != LZMA_VLI_UNKNOWN {
        let ret = lzma_vli_encode_impl(
            (*block).compressed_size,
            ptr::null_mut(),
            output,
            &mut out_pos,
            out_size,
        );
        if ret != LZMA_OK {
            return ret;
        }
        *output.add(1) |= 0x40;
    }

    if (*block).uncompressed_size != LZMA_VLI_UNKNOWN {
        let ret = lzma_vli_encode_impl(
            (*block).uncompressed_size,
            ptr::null_mut(),
            output,
            &mut out_pos,
            out_size,
        );
        if ret != LZMA_OK {
            return ret;
        }
        *output.add(1) |= 0x80;
    }

    if (*block).filters.is_null() || (*(*block).filters).id == LZMA_VLI_UNKNOWN {
        return LZMA_PROG_ERROR;
    }

    let mut filter_count = 0usize;
    loop {
        if filter_count == crate::ffi::types::LZMA_FILTERS_MAX {
            return LZMA_PROG_ERROR;
        }

        let ret = filter_flags_encode_impl(
            (*block).filters.add(filter_count),
            output,
            &mut out_pos,
            out_size,
        );
        if ret != LZMA_OK {
            return ret;
        }

        filter_count += 1;
        if (*(*block).filters.add(filter_count)).id == LZMA_VLI_UNKNOWN {
            break;
        }
    }

    *output.add(1) |= (filter_count - 1) as u8;
    ptr::write_bytes(output.add(out_pos), 0, out_size - out_pos);
    write32le(
        output.add(out_size),
        check::crc32::crc32(core::slice::from_raw_parts(output, out_size), 0),
    );
    LZMA_OK
}

pub(crate) unsafe fn block_header_decode(
    block: *mut lzma_block,
    allocator: *const lzma_allocator,
    input: *const u8,
) -> lzma_ret {
    if block.is_null() || (*block).filters.is_null() || input.is_null() {
        return LZMA_PROG_ERROR;
    }

    for i in 0..=crate::ffi::types::LZMA_FILTERS_MAX {
        let filter = (*block).filters.add(i);
        (*filter).id = LZMA_VLI_UNKNOWN;
        (*filter).options = ptr::null_mut();
    }

    if (*block).version > 1 {
        (*block).version = 1;
    }
    (*block).ignore_check = 0;

    if lzma_block_header_size_decode(*input) != (*block).header_size
        || ((*block).check as usize) > LZMA_CHECK_ID_MAX
    {
        return LZMA_PROG_ERROR;
    }

    let in_size = (*block).header_size.saturating_sub(4) as usize;
    if check::crc32::crc32(core::slice::from_raw_parts(input, in_size), 0)
        != read32le(input.add(in_size))
    {
        return LZMA_DATA_ERROR;
    }

    if (*input.add(1) & 0x3C) != 0 {
        return LZMA_OPTIONS_ERROR;
    }

    let mut in_pos = 2usize;
    if (*input.add(1) & 0x40) != 0 {
        let ret = lzma_vli_decode_impl(
            &mut (*block).compressed_size,
            ptr::null_mut(),
            input,
            &mut in_pos,
            in_size,
        );
        if ret != LZMA_OK {
            return ret;
        }
        if block_unpadded_size(block.cast_const()) == 0 {
            return LZMA_DATA_ERROR;
        }
    } else {
        (*block).compressed_size = LZMA_VLI_UNKNOWN;
    }

    if (*input.add(1) & 0x80) != 0 {
        let ret = lzma_vli_decode_impl(
            &mut (*block).uncompressed_size,
            ptr::null_mut(),
            input,
            &mut in_pos,
            in_size,
        );
        if ret != LZMA_OK {
            return ret;
        }
    } else {
        (*block).uncompressed_size = LZMA_VLI_UNKNOWN;
    }

    let filter_count = ((*input.add(1) & 3) + 1) as usize;
    for i in 0..filter_count {
        let ret = filter_flags_decode_impl(
            (*block).filters.add(i),
            allocator,
            input,
            &mut in_pos,
            in_size,
        );
        if ret != LZMA_OK {
            filter::filters_free_impl((*block).filters, allocator);
            return ret;
        }
    }

    while in_pos < in_size {
        if *input.add(in_pos) != 0 {
            filter::filters_free_impl((*block).filters, allocator);
            return LZMA_OPTIONS_ERROR;
        }
        in_pos += 1;
    }

    LZMA_OK
}

pub(crate) unsafe fn block_compressed_size(
    block: *mut lzma_block,
    unpadded_size: lzma_vli,
) -> lzma_ret {
    if block_unpadded_size(block.cast_const()) == 0 {
        return LZMA_PROG_ERROR;
    }

    let container_size =
        u64::from((*block).header_size) + u64::from(check::check_size((*block).check));
    if unpadded_size <= container_size {
        return LZMA_DATA_ERROR;
    }

    let compressed_size = unpadded_size - container_size;
    if (*block).compressed_size != LZMA_VLI_UNKNOWN && (*block).compressed_size != compressed_size {
        return LZMA_DATA_ERROR;
    }

    (*block).compressed_size = compressed_size;
    LZMA_OK
}

pub(crate) unsafe fn block_total_size(block: *const lzma_block) -> lzma_vli {
    let mut unpadded = block_unpadded_size(block);
    if unpadded != LZMA_VLI_UNKNOWN {
        unpadded = (unpadded + 3) & !3;
    }
    unpadded
}

pub(crate) unsafe fn block_unpadded_size(block: *const lzma_block) -> lzma_vli {
    if block.is_null()
        || (*block).version > 1
        || (*block).header_size < LZMA_BLOCK_HEADER_SIZE_MIN
        || (*block).header_size > LZMA_BLOCK_HEADER_SIZE_MAX
        || ((*block).header_size & 3) != 0
        || !vli_is_valid((*block).compressed_size)
        || (*block).compressed_size == 0
        || ((*block).check as usize) > LZMA_CHECK_ID_MAX
    {
        return 0;
    }

    if (*block).compressed_size == LZMA_VLI_UNKNOWN {
        return LZMA_VLI_UNKNOWN;
    }

    let unpadded = (*block).compressed_size
        + u64::from((*block).header_size)
        + u64::from(check::check_size((*block).check));
    if unpadded < UNPADDED_SIZE_MIN || unpadded > UNPADDED_SIZE_MAX {
        return 0;
    }

    unpadded
}

#[inline]
fn lzma_block_header_size_decode(encoded: u8) -> u32 {
    (u32::from(encoded) + 1) * 4
}
