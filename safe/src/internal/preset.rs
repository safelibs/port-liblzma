use crate::ffi::types::{lzma_bool, lzma_match_finder, lzma_mode, lzma_options_lzma};
use crate::internal::common::{
    lzma_bool as to_lzma_bool, LZMA_LC_DEFAULT, LZMA_LP_DEFAULT, LZMA_MF_BT2, LZMA_MF_BT3,
    LZMA_MF_BT4, LZMA_MF_HC3, LZMA_MF_HC4, LZMA_MODE_FAST, LZMA_MODE_NORMAL,
    LZMA_PB_DEFAULT, LZMA_PRESET_EXTREME, LZMA_PRESET_LEVEL_MASK,
};

pub(crate) unsafe fn lzma_lzma_preset_impl(
    options: *mut lzma_options_lzma,
    preset: u32,
) -> lzma_bool {
    if options.is_null() {
        return 1;
    }

    let level = preset & LZMA_PRESET_LEVEL_MASK;
    let flags = preset & !LZMA_PRESET_LEVEL_MASK;
    let supported_flags = LZMA_PRESET_EXTREME;

    if level > 9 || (flags & !supported_flags) != 0 {
        return 1;
    }

    (*options).preset_dict = core::ptr::null();
    (*options).preset_dict_size = 0;
    (*options).lc = LZMA_LC_DEFAULT;
    (*options).lp = LZMA_LP_DEFAULT;
    (*options).pb = LZMA_PB_DEFAULT;

    const DICT_POW2: [u8; 10] = [18, 20, 21, 22, 22, 23, 23, 24, 25, 26];
    (*options).dict_size = 1u32 << DICT_POW2[level as usize];

    if level <= 3 {
        (*options).mode = LZMA_MODE_FAST;
        (*options).mf = if level == 0 { LZMA_MF_HC3 } else { LZMA_MF_HC4 };
        (*options).nice_len = if level <= 1 { 128 } else { 273 };
        const DEPTHS: [u32; 4] = [4, 8, 24, 48];
        (*options).depth = DEPTHS[level as usize];
    } else {
        (*options).mode = LZMA_MODE_NORMAL;
        (*options).mf = LZMA_MF_BT4;
        (*options).nice_len = match level {
            4 => 16,
            5 => 32,
            _ => 64,
        };
        (*options).depth = 0;
    }

    if (flags & LZMA_PRESET_EXTREME) != 0 {
        (*options).mode = LZMA_MODE_NORMAL;
        (*options).mf = LZMA_MF_BT4;
        if level == 3 || level == 5 {
            (*options).nice_len = 192;
            (*options).depth = 0;
        } else {
            (*options).nice_len = 273;
            (*options).depth = 512;
        }
    }

    0
}

pub(crate) const fn mf_is_supported(mf: lzma_match_finder) -> lzma_bool {
    to_lzma_bool(matches!(
        mf,
        LZMA_MF_HC3 | LZMA_MF_HC4 | LZMA_MF_BT2 | LZMA_MF_BT3 | LZMA_MF_BT4
    ))
}

pub(crate) const fn mode_is_supported(mode: lzma_mode) -> lzma_bool {
    to_lzma_bool(matches!(mode, LZMA_MODE_FAST | LZMA_MODE_NORMAL))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ffi::types::lzma_options_lzma;
    use crate::internal::common::{LZMA_MF_BT4, LZMA_PRESET_EXTREME};

    #[test]
    fn preset_level_six_matches_upstream_defaults() {
        let mut options = unsafe { core::mem::zeroed::<lzma_options_lzma>() };
        unsafe {
            assert_eq!(lzma_lzma_preset_impl(&mut options, 6), 0);
        }

        assert_eq!(options.dict_size, 1 << 23);
        assert_eq!(options.mode, LZMA_MODE_NORMAL);
        assert_eq!(options.mf, LZMA_MF_BT4);
        assert_eq!(options.nice_len, 64);
        assert_eq!(options.depth, 0);
    }

    #[test]
    fn extreme_preset_updates_search_depth() {
        let mut options = unsafe { core::mem::zeroed::<lzma_options_lzma>() };
        unsafe {
            assert_eq!(lzma_lzma_preset_impl(&mut options, 3 | LZMA_PRESET_EXTREME), 0);
        }

        assert_eq!(options.mode, LZMA_MODE_NORMAL);
        assert_eq!(options.mf, LZMA_MF_BT4);
        assert_eq!(options.nice_len, 192);
        assert_eq!(options.depth, 0);
    }

    #[test]
    fn support_queries_match_supported_modes() {
        assert_eq!(mf_is_supported(LZMA_MF_HC3), 1);
        assert_eq!(mf_is_supported(0x55), 0);
        assert_eq!(mode_is_supported(LZMA_MODE_FAST), 1);
        assert_eq!(mode_is_supported(0), 0);
    }
}
