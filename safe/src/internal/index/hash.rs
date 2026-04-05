use core::mem::size_of;
use core::ptr;

use crate::ffi::types::{
    lzma_allocator, lzma_index_hash, lzma_ret, lzma_vli, LZMA_BUF_ERROR, LZMA_DATA_ERROR, LZMA_OK,
    LZMA_PROG_ERROR, LZMA_STREAM_END,
};
use crate::internal::check::{crc32, sha256::Sha256State};
use crate::internal::common::{lzma_alloc, lzma_free, LZMA_VLI_MAX};

use super::core::{
    index_size_from_counts, index_size_unpadded_from_counts, vli_ceil4, INDEX_INDICATOR,
    UNPADDED_SIZE_MAX, UNPADDED_SIZE_MIN,
};
use crate::internal::vli::lzma_vli_decode_impl;

fn list_size_add(unpadded_size: lzma_vli, uncompressed_size: lzma_vli) -> lzma_vli {
    crate::internal::vli::lzma_vli_size_impl(unpadded_size) as lzma_vli
        + crate::internal::vli::lzma_vli_size_impl(uncompressed_size) as lzma_vli
}

#[derive(Clone)]
struct HashInfo {
    blocks_size: lzma_vli,
    uncompressed_size: lzma_vli,
    count: lzma_vli,
    index_list_size: lzma_vli,
    check: Sha256State,
}

impl HashInfo {
    fn new() -> Self {
        Self {
            blocks_size: 0,
            uncompressed_size: 0,
            count: 0,
            index_list_size: 0,
            check: Sha256State::new(),
        }
    }

    fn append_unchecked(&mut self, unpadded_size: lzma_vli, uncompressed_size: lzma_vli) {
        self.blocks_size += vli_ceil4(unpadded_size);
        self.uncompressed_size += uncompressed_size;
        self.index_list_size += list_size_add(unpadded_size, uncompressed_size);
        self.count += 1;

        let mut bytes = [0u8; 16];
        bytes[..8].copy_from_slice(&unpadded_size.to_ne_bytes());
        bytes[8..].copy_from_slice(&uncompressed_size.to_ne_bytes());
        self.check.update(&bytes);
    }
}

#[repr(C)]
pub(crate) struct IndexHash {
    sequence: u32,
    blocks: HashInfo,
    records: HashInfo,
    remaining: lzma_vli,
    unpadded_size: lzma_vli,
    uncompressed_size: lzma_vli,
    pos: usize,
    crc32: u32,
}

const SEQ_BLOCK: u32 = 0;
const SEQ_COUNT: u32 = 1;
const SEQ_UNPADDED: u32 = 2;
const SEQ_UNCOMPRESSED: u32 = 3;
const SEQ_PADDING_INIT: u32 = 4;
const SEQ_PADDING: u32 = 5;
const SEQ_CRC32: u32 = 6;

unsafe fn hash_ref(ptr: *const lzma_index_hash) -> &'static IndexHash {
    &*ptr.cast::<IndexHash>()
}

unsafe fn hash_mut(ptr: *mut lzma_index_hash) -> &'static mut IndexHash {
    &mut *ptr.cast::<IndexHash>()
}

fn reset_hash(hash: &mut IndexHash) {
    hash.sequence = SEQ_BLOCK;
    hash.blocks = HashInfo::new();
    hash.records = HashInfo::new();
    hash.remaining = 0;
    hash.unpadded_size = 0;
    hash.uncompressed_size = 0;
    hash.pos = 0;
    hash.crc32 = 0;
}

pub(crate) unsafe fn index_hash_init(
    index_hash: *mut lzma_index_hash,
    allocator: *const lzma_allocator,
) -> *mut lzma_index_hash {
    let raw = if index_hash.is_null() {
        let raw = lzma_alloc(size_of::<IndexHash>(), allocator).cast::<IndexHash>();
        if raw.is_null() {
            return ptr::null_mut();
        }
        raw
    } else {
        index_hash.cast::<IndexHash>()
    };

    ptr::write(
        raw,
        IndexHash {
            sequence: 0,
            blocks: HashInfo::new(),
            records: HashInfo::new(),
            remaining: 0,
            unpadded_size: 0,
            uncompressed_size: 0,
            pos: 0,
            crc32: 0,
        },
    );
    reset_hash(&mut *raw);
    raw.cast::<lzma_index_hash>()
}

pub(crate) unsafe fn index_hash_end(
    index_hash: *mut lzma_index_hash,
    allocator: *const lzma_allocator,
) {
    if index_hash.is_null() {
        return;
    }

    ptr::drop_in_place(index_hash.cast::<IndexHash>());
    lzma_free(index_hash.cast(), allocator);
}

pub(crate) unsafe fn index_hash_size(index_hash: *const lzma_index_hash) -> lzma_vli {
    if index_hash.is_null() {
        return 0;
    }

    let hash = hash_ref(index_hash);
    index_size_from_counts(hash.blocks.count, hash.blocks.index_list_size)
}

pub(crate) unsafe fn index_hash_append(
    index_hash: *mut lzma_index_hash,
    unpadded_size: lzma_vli,
    uncompressed_size: lzma_vli,
) -> lzma_ret {
    if index_hash.is_null()
        || hash_ref(index_hash).sequence != SEQ_BLOCK
        || unpadded_size < UNPADDED_SIZE_MIN
        || unpadded_size > UNPADDED_SIZE_MAX
        || uncompressed_size > LZMA_VLI_MAX
    {
        return LZMA_PROG_ERROR;
    }

    let hash = hash_mut(index_hash);
    hash.blocks
        .append_unchecked(unpadded_size, uncompressed_size);

    let size = index_size_from_counts(hash.blocks.count, hash.blocks.index_list_size);
    if hash.blocks.blocks_size > LZMA_VLI_MAX
        || hash.blocks.uncompressed_size > LZMA_VLI_MAX
        || size > crate::internal::stream_flags::LZMA_BACKWARD_SIZE_MAX
        || crate::internal::index::core::index_stream_size_from_counts(
            hash.blocks.blocks_size,
            hash.blocks.count,
            hash.blocks.index_list_size,
        ) > LZMA_VLI_MAX
    {
        return LZMA_DATA_ERROR;
    }

    LZMA_OK
}

pub(crate) unsafe fn index_hash_decode(
    index_hash: *mut lzma_index_hash,
    input: *const u8,
    in_pos: *mut usize,
    in_size: usize,
) -> lzma_ret {
    if index_hash.is_null() || input.is_null() || in_pos.is_null() {
        return LZMA_PROG_ERROR;
    }
    if *in_pos >= in_size {
        return LZMA_BUF_ERROR;
    }

    let hash = hash_mut(index_hash);
    let in_start = *in_pos;
    let mut ret = LZMA_OK;

    while *in_pos < in_size {
        match hash.sequence {
            SEQ_BLOCK => {
                if *input.add(*in_pos) != INDEX_INDICATOR {
                    return LZMA_DATA_ERROR;
                }
                *in_pos += 1;
                hash.sequence = SEQ_COUNT;
            }
            SEQ_COUNT => {
                ret = lzma_vli_decode_impl(
                    &mut hash.remaining,
                    &mut hash.pos,
                    input,
                    in_pos,
                    in_size,
                );
                if ret != LZMA_STREAM_END {
                    break;
                }
                hash.pos = 0;
                if hash.remaining != hash.blocks.count {
                    return LZMA_DATA_ERROR;
                }
                ret = LZMA_OK;
                hash.sequence = if hash.remaining == 0 {
                    SEQ_PADDING_INIT
                } else {
                    SEQ_UNPADDED
                };
            }
            SEQ_UNPADDED | SEQ_UNCOMPRESSED => {
                let target = if hash.sequence == SEQ_UNPADDED {
                    &mut hash.unpadded_size
                } else {
                    &mut hash.uncompressed_size
                };
                ret = lzma_vli_decode_impl(target, &mut hash.pos, input, in_pos, in_size);
                if ret != LZMA_STREAM_END {
                    break;
                }
                hash.pos = 0;
                ret = LZMA_OK;

                if hash.sequence == SEQ_UNPADDED {
                    if hash.unpadded_size < UNPADDED_SIZE_MIN
                        || hash.unpadded_size > UNPADDED_SIZE_MAX
                    {
                        return LZMA_DATA_ERROR;
                    }
                    hash.sequence = SEQ_UNCOMPRESSED;
                } else {
                    hash.records
                        .append_unchecked(hash.unpadded_size, hash.uncompressed_size);
                    if hash.blocks.blocks_size < hash.records.blocks_size
                        || hash.blocks.uncompressed_size < hash.records.uncompressed_size
                        || hash.blocks.index_list_size < hash.records.index_list_size
                    {
                        return LZMA_DATA_ERROR;
                    }
                    hash.sequence = {
                        hash.remaining -= 1;
                        if hash.remaining == 0 {
                            SEQ_PADDING_INIT
                        } else {
                            SEQ_UNPADDED
                        }
                    };
                }
            }
            SEQ_PADDING_INIT => {
                hash.pos = ((4u64.wrapping_sub(index_size_unpadded_from_counts(
                    hash.records.count,
                    hash.records.index_list_size,
                ))) & 3) as usize;
                hash.sequence = SEQ_PADDING;
            }
            SEQ_PADDING => {
                if hash.pos > 0 {
                    hash.pos -= 1;
                    if *input.add(*in_pos) != 0 {
                        return LZMA_DATA_ERROR;
                    }
                    *in_pos += 1;
                } else {
                    if hash.blocks.blocks_size != hash.records.blocks_size
                        || hash.blocks.uncompressed_size != hash.records.uncompressed_size
                        || hash.blocks.index_list_size != hash.records.index_list_size
                    {
                        return LZMA_DATA_ERROR;
                    }

                    let blocks_digest = hash.blocks.check.clone().finish();
                    let records_digest = hash.records.check.clone().finish();
                    if blocks_digest != records_digest {
                        return LZMA_DATA_ERROR;
                    }

                    hash.crc32 = crc32::crc32(
                        core::slice::from_raw_parts(input.add(in_start), *in_pos - in_start),
                        hash.crc32,
                    );
                    hash.sequence = SEQ_CRC32;
                }
            }
            SEQ_CRC32 => {
                while hash.pos < 4 {
                    if *in_pos == in_size {
                        return LZMA_OK;
                    }

                    if ((hash.crc32 >> (hash.pos * 8)) as u8) != *input.add(*in_pos) {
                        return LZMA_DATA_ERROR;
                    }
                    *in_pos += 1;
                    hash.pos += 1;
                }

                return LZMA_STREAM_END;
            }
            _ => return LZMA_PROG_ERROR,
        }
    }

    let used = *in_pos - in_start;
    if used > 0 {
        hash.crc32 = crc32::crc32(
            core::slice::from_raw_parts(input.add(in_start), used),
            hash.crc32,
        );
    }
    ret
}
