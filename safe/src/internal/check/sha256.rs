#[derive(Clone)]
pub(crate) struct Sha256State {
    state: [u32; 8],
    buffer: [u8; 64],
    buffer_len: usize,
    total_size: u64,
}

const SHA256_K: [u32; 64] = [
    0x428A2F98, 0x71374491, 0xB5C0FBCF, 0xE9B5DBA5, 0x3956C25B, 0x59F111F1, 0x923F82A4, 0xAB1C5ED5,
    0xD807AA98, 0x12835B01, 0x243185BE, 0x550C7DC3, 0x72BE5D74, 0x80DEB1FE, 0x9BDC06A7, 0xC19BF174,
    0xE49B69C1, 0xEFBE4786, 0x0FC19DC6, 0x240CA1CC, 0x2DE92C6F, 0x4A7484AA, 0x5CB0A9DC, 0x76F988DA,
    0x983E5152, 0xA831C66D, 0xB00327C8, 0xBF597FC7, 0xC6E00BF3, 0xD5A79147, 0x06CA6351, 0x14292967,
    0x27B70A85, 0x2E1B2138, 0x4D2C6DFC, 0x53380D13, 0x650A7354, 0x766A0ABB, 0x81C2C92E, 0x92722C85,
    0xA2BFE8A1, 0xA81A664B, 0xC24B8B70, 0xC76C51A3, 0xD192E819, 0xD6990624, 0xF40E3585, 0x106AA070,
    0x19A4C116, 0x1E376C08, 0x2748774C, 0x34B0BCB5, 0x391C0CB3, 0x4ED8AA4A, 0x5B9CCA4F, 0x682E6FF3,
    0x748F82EE, 0x78A5636F, 0x84C87814, 0x8CC70208, 0x90BEFFFA, 0xA4506CEB, 0xBEF9A3F7, 0xC67178F2,
];

#[inline]
const fn rotr32(value: u32, amount: u32) -> u32 {
    value.rotate_right(amount)
}

#[inline]
const fn ch(x: u32, y: u32, z: u32) -> u32 {
    z ^ (x & (y ^ z))
}

#[inline]
const fn maj(x: u32, y: u32, z: u32) -> u32 {
    (x & (y ^ z)).wrapping_add(y & z)
}

#[inline]
const fn big_sigma0(x: u32) -> u32 {
    rotr32(x ^ rotr32(x ^ rotr32(x, 9), 11), 2)
}

#[inline]
const fn big_sigma1(x: u32) -> u32 {
    rotr32(x ^ rotr32(x ^ rotr32(x, 14), 5), 6)
}

#[inline]
const fn small_sigma0(x: u32) -> u32 {
    rotr32(x ^ rotr32(x, 11), 7) ^ (x >> 3)
}

#[inline]
const fn small_sigma1(x: u32) -> u32 {
    rotr32(x ^ rotr32(x, 2), 17) ^ (x >> 10)
}

impl Sha256State {
    pub(crate) fn new() -> Self {
        Self {
            state: [
                0x6A09E667, 0xBB67AE85, 0x3C6EF372, 0xA54FF53A, 0x510E527F, 0x9B05688C, 0x1F83D9AB,
                0x5BE0CD19,
            ],
            buffer: [0; 64],
            buffer_len: 0,
            total_size: 0,
        }
    }

    pub(crate) fn update(&mut self, mut input: &[u8]) {
        while !input.is_empty() {
            let copy_len = (64 - self.buffer_len).min(input.len());
            self.buffer[self.buffer_len..self.buffer_len + copy_len]
                .copy_from_slice(&input[..copy_len]);
            self.buffer_len += copy_len;
            self.total_size += copy_len as u64;
            input = &input[copy_len..];

            if self.buffer_len == 64 {
                transform(&mut self.state, &self.buffer);
                self.buffer_len = 0;
            }
        }
    }

    pub(crate) fn finish(mut self) -> [u8; 32] {
        self.buffer[self.buffer_len] = 0x80;
        self.buffer_len += 1;

        if self.buffer_len > 56 {
            self.buffer[self.buffer_len..].fill(0);
            transform(&mut self.state, &self.buffer);
            self.buffer_len = 0;
        }

        self.buffer[self.buffer_len..56].fill(0);
        let bit_len = self.total_size * 8;
        self.buffer[56..64].copy_from_slice(&bit_len.to_be_bytes());
        transform(&mut self.state, &self.buffer);

        let mut out = [0u8; 32];
        for (chunk, word) in out.chunks_exact_mut(4).zip(self.state) {
            chunk.copy_from_slice(&word.to_be_bytes());
        }
        out
    }
}

fn transform(state: &mut [u32; 8], block: &[u8; 64]) {
    let mut w = [0u32; 64];
    for (index, chunk) in block.chunks_exact(4).enumerate().take(16) {
        w[index] = u32::from_be_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
    }

    for index in 16..64 {
        w[index] = small_sigma1(w[index - 2])
            .wrapping_add(w[index - 7])
            .wrapping_add(small_sigma0(w[index - 15]))
            .wrapping_add(w[index - 16]);
    }

    let mut a = state[0];
    let mut b = state[1];
    let mut c = state[2];
    let mut d = state[3];
    let mut e = state[4];
    let mut f = state[5];
    let mut g = state[6];
    let mut h = state[7];

    for index in 0..64 {
        let t1 = h
            .wrapping_add(big_sigma1(e))
            .wrapping_add(ch(e, f, g))
            .wrapping_add(SHA256_K[index])
            .wrapping_add(w[index]);
        let t2 = big_sigma0(a).wrapping_add(maj(a, b, c));

        h = g;
        g = f;
        f = e;
        e = d.wrapping_add(t1);
        d = c;
        c = b;
        b = a;
        a = t1.wrapping_add(t2);
    }

    state[0] = state[0].wrapping_add(a);
    state[1] = state[1].wrapping_add(b);
    state[2] = state[2].wrapping_add(c);
    state[3] = state[3].wrapping_add(d);
    state[4] = state[4].wrapping_add(e);
    state[5] = state[5].wrapping_add(f);
    state[6] = state[6].wrapping_add(g);
    state[7] = state[7].wrapping_add(h);
}

#[cfg(test)]
mod tests {
    use super::Sha256State;

    #[test]
    fn sha256_matches_standard_vector() {
        let mut state = Sha256State::new();
        state.update(b"123");
        state.update(b"456789");
        assert_eq!(
            state.finish(),
            [
                0x15, 0xE2, 0xB0, 0xD3, 0xC3, 0x38, 0x91, 0xEB, 0xB0, 0xF1, 0xEF, 0x60, 0x9E, 0xC4,
                0x19, 0x42, 0x0C, 0x20, 0xE3, 0x20, 0xCE, 0x94, 0xC6, 0x5F, 0xBC, 0x8C, 0x33, 0x12,
                0x44, 0x8E, 0xB2, 0x25,
            ]
        );
    }
}
