pub(crate) fn bit_price(probability: u16) -> u32 {
    let centered = u32::from(probability.max(1));
    2048u32.saturating_sub(centered.min(2048))
}
