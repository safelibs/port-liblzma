use std::sync::OnceLock;

const POLY32: u32 = 0xEDB8_8320;

const CRC32_TABLE: [[u32; 256]; 8] = generate_crc32_table();

const fn generate_crc32_table() -> [[u32; 256]; 8] {
    let mut table = [[0u32; 256]; 8];
    let mut i = 0usize;
    while i < 256 {
        let mut r = i as u32;
        let mut j = 0usize;
        while j < 8 {
            if (r & 1) != 0 {
                r = (r >> 1) ^ POLY32;
            } else {
                r >>= 1;
            }
            j += 1;
        }
        table[0][i] = r;
        i += 1;
    }

    let mut slice = 1usize;
    while slice < 8 {
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

type Crc32Impl = fn(&[u8], u32) -> u32;

fn dispatch() -> Crc32Impl {
    static DISPATCH: OnceLock<Crc32Impl> = OnceLock::new();
    *DISPATCH.get_or_init(|| {
        #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
        {
            let _ = std::arch::is_x86_feature_detected!("pclmulqdq");
        }

        // Stable Rust doesn't expose an IEEE CRC-32 hardware primitive,
        // so the best portable implementation remains slice-by-8.
        crc32_slice_by_8
    })
}

#[inline]
pub(crate) fn crc32(buf: &[u8], crc: u32) -> u32 {
    dispatch()(buf, crc)
}

fn crc32_slice_by_8(buf: &[u8], crc: u32) -> u32 {
    let mut crc = !crc;
    let mut index = 0usize;
    let len = buf.len();

    if len > 8 {
        while index < len && ((buf.as_ptr() as usize + index) & 7) != 0 {
            crc = CRC32_TABLE[0][(buf[index] ^ (crc as u8)) as usize] ^ (crc >> 8);
            index += 1;
        }

        let limit = index + ((len - index) & !7usize);
        while index < limit {
            let block0 =
                u32::from_le_bytes([buf[index], buf[index + 1], buf[index + 2], buf[index + 3]]);
            crc ^= block0;
            crc = CRC32_TABLE[7][(crc & 0xFF) as usize]
                ^ CRC32_TABLE[6][((crc >> 8) & 0xFF) as usize]
                ^ CRC32_TABLE[5][((crc >> 16) & 0xFF) as usize]
                ^ CRC32_TABLE[4][(crc >> 24) as usize];

            let block1 = u32::from_le_bytes([
                buf[index + 4],
                buf[index + 5],
                buf[index + 6],
                buf[index + 7],
            ]);
            crc = CRC32_TABLE[3][(block1 & 0xFF) as usize]
                ^ CRC32_TABLE[2][((block1 >> 8) & 0xFF) as usize]
                ^ crc
                ^ CRC32_TABLE[1][((block1 >> 16) & 0xFF) as usize]
                ^ CRC32_TABLE[0][(block1 >> 24) as usize];

            index += 8;
        }
    }

    while index < len {
        crc = CRC32_TABLE[0][(buf[index] ^ (crc as u8)) as usize] ^ (crc >> 8);
        index += 1;
    }

    !crc
}

#[cfg(test)]
mod tests {
    use super::crc32;

    #[test]
    fn crc32_matches_reference_vectors() {
        assert_eq!(crc32(b"123456789", 0), 0xCBF4_3926);
        let mut crc = 0;
        for byte in b"123456789" {
            crc = crc32(core::slice::from_ref(byte), crc);
        }
        assert_eq!(crc, 0xCBF4_3926);
    }
}
