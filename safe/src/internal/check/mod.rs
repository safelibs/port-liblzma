pub(crate) mod crc32;
pub(crate) mod crc64;
pub(crate) mod sha256;

use crate::ffi::types::{lzma_check, lzma_bool};
use crate::internal::common::{
    lzma_bool as to_lzma_bool, LZMA_CHECK_CRC32, LZMA_CHECK_CRC64, LZMA_CHECK_ID_MAX,
    LZMA_CHECK_SHA256,
};

pub(crate) enum CheckState {
    None,
    Crc32(u32),
    Crc64(u64),
    Sha256(sha256::Sha256State),
}

impl CheckState {
    pub(crate) fn new(check: lzma_check) -> Option<Self> {
        match check {
            crate::ffi::types::LZMA_CHECK_NONE => Some(Self::None),
            LZMA_CHECK_CRC32 => Some(Self::Crc32(0)),
            LZMA_CHECK_CRC64 => Some(Self::Crc64(0)),
            LZMA_CHECK_SHA256 => Some(Self::Sha256(sha256::Sha256State::new())),
            _ => None,
        }
    }

    pub(crate) fn update(&mut self, buf: &[u8]) {
        match self {
            Self::None => {}
            Self::Crc32(state) => *state = crc32::crc32(buf, *state),
            Self::Crc64(state) => *state = crc64::crc64(buf, *state),
            Self::Sha256(state) => state.update(buf),
        }
    }

    pub(crate) fn finish(self) -> [u8; 64] {
        let mut out = [0u8; 64];
        match self {
            Self::None => {}
            Self::Crc32(state) => out[..4].copy_from_slice(&state.to_le_bytes()),
            Self::Crc64(state) => out[..8].copy_from_slice(&state.to_le_bytes()),
            Self::Sha256(state) => out[..32].copy_from_slice(&state.finish()),
        }
        out
    }
}

pub(crate) fn check_is_supported(check: lzma_check) -> lzma_bool {
    if check < 0 || (check as usize) > LZMA_CHECK_ID_MAX {
        return 0;
    }

    to_lzma_bool(matches!(
        check,
        crate::ffi::types::LZMA_CHECK_NONE | LZMA_CHECK_CRC32 | LZMA_CHECK_CRC64 | LZMA_CHECK_SHA256
    ))
}

pub(crate) fn check_size(check: lzma_check) -> u32 {
    if check < 0 || (check as usize) > LZMA_CHECK_ID_MAX {
        return u32::MAX;
    }

    const CHECK_SIZES: [u8; LZMA_CHECK_ID_MAX + 1] =
        [0, 4, 4, 4, 8, 8, 8, 16, 16, 16, 32, 32, 32, 64, 64, 64];
    CHECK_SIZES[check as usize] as u32
}

#[cfg(test)]
mod tests {
    use super::{check_is_supported, check_size, CheckState};
    use crate::internal::common::{LZMA_CHECK_CRC32, LZMA_CHECK_CRC64, LZMA_CHECK_SHA256};

    #[test]
    fn support_and_sizes_match_upstream_contract() {
        assert_eq!(check_is_supported(crate::ffi::types::LZMA_CHECK_NONE), 1);
        assert_eq!(check_is_supported(LZMA_CHECK_CRC32), 1);
        assert_eq!(check_is_supported(LZMA_CHECK_CRC64), 1);
        assert_eq!(check_is_supported(LZMA_CHECK_SHA256), 1);
        assert_eq!(check_is_supported(16), 0);

        assert_eq!(check_size(crate::ffi::types::LZMA_CHECK_NONE), 0);
        assert_eq!(check_size(LZMA_CHECK_CRC32), 4);
        assert_eq!(check_size(LZMA_CHECK_CRC64), 8);
        assert_eq!(check_size(LZMA_CHECK_SHA256), 32);
        assert_eq!(check_size(16), u32::MAX);
    }

    #[test]
    fn check_state_finishes_with_liblzma_byte_order() {
        let mut crc = CheckState::new(LZMA_CHECK_CRC32).unwrap();
        crc.update(b"123456789");
        assert_eq!(&crc.finish()[..4], &[0x26, 0x39, 0xF4, 0xCB]);

        let mut sha = CheckState::new(LZMA_CHECK_SHA256).unwrap();
        sha.update(b"123456789");
        assert_eq!(
            &sha.finish()[..4],
            &[0x15, 0xE2, 0xB0, 0xD3]
        );
    }
}
