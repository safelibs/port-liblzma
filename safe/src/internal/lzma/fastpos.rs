pub(crate) fn slot(dist: u32) -> u32 {
    if dist <= 4 {
        dist
    } else {
        let i = 31 - dist.leading_zeros();
        (i + i) + ((dist >> (i - 1)) & 1)
    }
}
