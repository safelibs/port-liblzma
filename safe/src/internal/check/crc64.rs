use std::sync::OnceLock;

const POLY64: u64 = 0xC96C_5795_D787_0F42;

const CRC64_TABLE: [[u64; 256]; 4] = generate_crc64_table();

const fn generate_crc64_table() -> [[u64; 256]; 4] {
    let mut table = [[0u64; 256]; 4];
    let mut i = 0usize;
    while i < 256 {
        let mut r = i as u64;
        let mut j = 0usize;
        while j < 8 {
            if (r & 1) != 0 {
                r = (r >> 1) ^ POLY64;
            } else {
                r >>= 1;
            }
            j += 1;
        }
        table[0][i] = r;
        i += 1;
    }

    let mut slice = 1usize;
    while slice < 4 {
        i = 0;
        while i < 256 {
            let prev = table[slice - 1][i];
            table[slice][i] = table[0][(prev & 0xFF) as usize] ^ (prev >> 8);
            i += 1;
        }
        slice += 1;
    }

    table
}

type Crc64Impl = fn(&[u8], u64) -> u64;

fn dispatch() -> Crc64Impl {
    static DISPATCH: OnceLock<Crc64Impl> = OnceLock::new();
    *DISPATCH.get_or_init(|| {
        #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
        {
            let _ = std::arch::is_x86_feature_detected!("pclmulqdq");
            let _ = std::arch::is_x86_feature_detected!("ssse3");
            let _ = std::arch::is_x86_feature_detected!("sse4.1");
        }

        crc64_slice_by_4
    })
}

#[inline]
pub(crate) fn crc64(buf: &[u8], crc: u64) -> u64 {
    dispatch()(buf, crc)
}

fn crc64_slice_by_4(buf: &[u8], crc: u64) -> u64 {
    let mut crc = !crc;
    let mut index = 0usize;
    let len = buf.len();

    if len > 4 {
        while index < len && ((buf.as_ptr() as usize + index) & 3) != 0 {
            crc = CRC64_TABLE[0][(buf[index] ^ (crc as u8)) as usize] ^ (crc >> 8);
            index += 1;
        }

        let limit = index + ((len - index) & !3usize);
        while index < limit {
            let block = u32::from_le_bytes([
                buf[index],
                buf[index + 1],
                buf[index + 2],
                buf[index + 3],
            ]);
            let tmp = (crc as u32) ^ block;
            crc = CRC64_TABLE[3][(tmp & 0xFF) as usize]
                ^ CRC64_TABLE[2][((tmp >> 8) & 0xFF) as usize]
                ^ (crc >> 32)
                ^ CRC64_TABLE[1][((tmp >> 16) & 0xFF) as usize]
                ^ CRC64_TABLE[0][(tmp >> 24) as usize];
            index += 4;
        }
    }

    while index < len {
        crc = CRC64_TABLE[0][(buf[index] ^ (crc as u8)) as usize] ^ (crc >> 8);
        index += 1;
    }

    !crc
}

#[cfg(test)]
mod tests {
    use super::crc64;

    #[test]
    fn crc64_matches_reference_vectors() {
        assert_eq!(crc64(b"123456789", 0), 0x995D_C9BB_DF19_39FA);

        let mut crc = 0x96E3_0D51_84B7_FA2C;
        let mut seed = 29u32;
        for _start in 0..32usize {
            for size in 1..(256 - 32) {
                let mut buf = [0u8; 256];
                for byte in &mut buf {
                    seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12_345);
                    *byte = (seed >> 22) as u8;
                }
                crc = crc64(&buf[..size], crc);
            }
        }

        assert_eq!(crc, 0x23AB_7871_7723_1C9F);
    }
}
