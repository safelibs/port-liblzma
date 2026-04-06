use crate::ffi::types::{lzma_match_finder, lzma_ret, LZMA_OPTIONS_ERROR};
use crate::internal::common::{LZMA_MF_BT2, LZMA_MF_BT3, LZMA_MF_BT4, LZMA_MF_HC3, LZMA_MF_HC4};

pub(crate) fn map_match_finder(mf: lzma_match_finder) -> Result<lzma_rust2::MfType, lzma_ret> {
    match mf {
        LZMA_MF_HC3 | LZMA_MF_HC4 => Ok(lzma_rust2::MfType::Hc4),
        LZMA_MF_BT2 | LZMA_MF_BT3 | LZMA_MF_BT4 => Ok(lzma_rust2::MfType::Bt4),
        _ => Err(LZMA_OPTIONS_ERROR),
    }
}
