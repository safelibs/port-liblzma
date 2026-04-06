use crate::internal::common::LZMA_DELTA_DIST_MAX;

pub(crate) fn encode_all(input: &[u8], distance: usize) -> Vec<u8> {
    debug_assert!((1..=LZMA_DELTA_DIST_MAX as usize).contains(&distance));

    let mut history = [0u8; 256];
    let mut pos = 0u8;
    let mut out = Vec::with_capacity(input.len());

    for &byte in input {
        let index = pos as usize;
        let delta = history[(distance.wrapping_add(index)) & 0xFF];
        out.push(byte.wrapping_sub(delta));
        history[index & 0xFF] = byte;
        pos = pos.wrapping_sub(1);
    }

    out
}
