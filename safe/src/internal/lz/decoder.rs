pub(crate) fn decoder_memusage(dict_size: u32) -> u64 {
    lzma_rust2::lzma2_get_memory_usage(dict_size) as u64
}
