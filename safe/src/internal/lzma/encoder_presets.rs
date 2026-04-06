pub(crate) fn dict_size_for_level(level: u32) -> Option<u32> {
    const DICTS: [u32; 10] = [
        1 << 18,
        1 << 20,
        1 << 21,
        1 << 22,
        1 << 22,
        1 << 23,
        1 << 23,
        1 << 24,
        1 << 25,
        1 << 26,
    ];

    DICTS.get(level as usize).copied()
}
