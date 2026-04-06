use crate::ffi::types::lzma_match_finder;
use crate::internal::common::{
    LZMA_DICT_SIZE_MIN, LZMA_MF_BT2, LZMA_MF_BT3, LZMA_MF_BT4, LZMA_MF_HC3, LZMA_MF_HC4,
};

const HASH_2_SIZE: u32 = 1 << 10;
const HASH_3_SIZE: u32 = 1 << 16;
const LZ_ENCODER_DICT_SIZE_MAX: u32 = (1 << 30) + (1 << 29);
const LZ_ENCODER_EXTRA_RESERVE: u32 = 1 << 19;

fn hash_bytes(mf: lzma_match_finder) -> Option<u32> {
    match mf {
        LZMA_MF_HC3 => Some(3),
        LZMA_MF_HC4 => Some(4),
        LZMA_MF_BT2 => Some(2),
        LZMA_MF_BT3 => Some(3),
        LZMA_MF_BT4 => Some(4),
        _ => None,
    }
}

pub(crate) fn encoder_memusage(
    dict_size: u32,
    before_size: u32,
    after_size: u32,
    match_len_max: u32,
    nice_len: u32,
    mf: lzma_match_finder,
) -> u64 {
    if dict_size < LZMA_DICT_SIZE_MIN
        || dict_size > LZ_ENCODER_DICT_SIZE_MAX
        || nice_len > match_len_max
    {
        return u64::MAX;
    }

    let hash_bytes = match hash_bytes(mf) {
        Some(hash_bytes) => hash_bytes,
        None => return u64::MAX,
    };

    let keep_size_before = u64::from(before_size) + u64::from(dict_size);
    let keep_size_after = u64::from(after_size) + u64::from(match_len_max);

    let mut reserve = u64::from(dict_size / 2);
    if reserve > (1u64 << 30) {
        reserve /= 2;
    }

    reserve = reserve
        .saturating_add(u64::from(
            before_size
                .saturating_add(match_len_max)
                .saturating_add(after_size)
                / 2,
        ))
        .saturating_add(u64::from(LZ_ENCODER_EXTRA_RESERVE));

    let buffer_size = keep_size_before
        .saturating_add(reserve)
        .saturating_add(keep_size_after);

    let mut hash_slots = if hash_bytes == 2 {
        0xFFFFu32
    } else {
        let mut slots = dict_size.saturating_sub(1);
        slots |= slots >> 1;
        slots |= slots >> 2;
        slots |= slots >> 4;
        slots |= slots >> 8;
        slots >>= 1;
        slots |= 0xFFFF;
        if slots > (1 << 24) {
            if hash_bytes == 3 {
                slots = (1 << 24) - 1;
            } else {
                slots >>= 1;
            }
        }
        slots
    };

    hash_slots = hash_slots.saturating_add(1);
    if hash_bytes > 2 {
        hash_slots = hash_slots.saturating_add(HASH_2_SIZE);
    }
    if hash_bytes > 3 {
        hash_slots = hash_slots.saturating_add(HASH_3_SIZE);
    }

    let is_bt = (mf & 0x10) != 0;
    let mut sons_count = u64::from(dict_size).saturating_add(1);
    if is_bt {
        sons_count = sons_count.saturating_mul(2);
    }

    u64::from(hash_slots)
        .saturating_add(sons_count)
        .saturating_mul(4)
        .saturating_add(buffer_size)
}
