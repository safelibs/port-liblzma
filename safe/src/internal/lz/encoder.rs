use crate::ffi::types::lzma_match_finder;

pub(crate) fn encoder_memusage(dict_size: u32, extra_before: u32, extra_after: u32, nice_len: u32, mf: lzma_match_finder) -> u64 {
    let _ = crate::internal::lz::match_finder::map_match_finder(mf).unwrap_or_default();
    u64::from(dict_size)
        .saturating_add(u64::from(extra_before))
        .saturating_add(u64::from(extra_after))
        .saturating_add(u64::from(nice_len))
        .max(1)
}
