use core::cmp;
use core::mem::size_of;
use core::ptr;

use crate::ffi::types::{
    lzma_allocator, lzma_filter, lzma_options_bcj, lzma_options_delta, lzma_options_lzma, lzma_ret,
    lzma_vli, LZMA_MEM_ERROR, LZMA_OK, LZMA_OPTIONS_ERROR, LZMA_PROG_ERROR,
};
use crate::internal::common::{LZMA_DICT_SIZE_MIN, LZMA_LCLP_MAX, LZMA_PB_MAX};
use crate::internal::filter::common::{
    LZMA_FILTER_ARM, LZMA_FILTER_ARM64, LZMA_FILTER_ARMTHUMB, LZMA_FILTER_DELTA, LZMA_FILTER_IA64,
    LZMA_FILTER_LZMA1, LZMA_FILTER_LZMA1EXT, LZMA_FILTER_LZMA2, LZMA_FILTER_POWERPC,
    LZMA_FILTER_SPARC, LZMA_FILTER_X86,
};
use crate::internal::{common::lzma_alloc_zero, common::lzma_free};

pub(crate) const LZMA_DELTA_TYPE_BYTE: i32 = 0;
pub(crate) const LZMA_DELTA_DIST_MIN: u32 = crate::internal::common::LZMA_DELTA_DIST_MIN;
pub(crate) const LZMA_DELTA_DIST_MAX: u32 = crate::internal::common::LZMA_DELTA_DIST_MAX;

fn is_bcj_filter(id: lzma_vli) -> bool {
    matches!(
        id,
        LZMA_FILTER_X86
            | LZMA_FILTER_POWERPC
            | LZMA_FILTER_IA64
            | LZMA_FILTER_ARM
            | LZMA_FILTER_ARMTHUMB
            | LZMA_FILTER_SPARC
            | LZMA_FILTER_ARM64
    )
}

fn is_lclppb_valid(options: &lzma_options_lzma) -> bool {
    options.lc <= LZMA_LCLP_MAX
        && options.lp <= LZMA_LCLP_MAX
        && options.lc + options.lp <= LZMA_LCLP_MAX
        && options.pb <= LZMA_PB_MAX
}

fn lzma_lclppb_encode(options: &lzma_options_lzma) -> Option<u8> {
    if !is_lclppb_valid(options) {
        return None;
    }

    Some(((options.pb * 5 + options.lp) * 9 + options.lc) as u8)
}

fn lzma_lclppb_decode(options: &mut lzma_options_lzma, byte: u8) -> bool {
    if byte > ((4 * 5 + 4) * 9 + 8) {
        return true;
    }

    options.pb = u32::from(byte) / (9 * 5);
    let mut byte = u32::from(byte) - options.pb * 9 * 5;
    options.lp = byte / 9;
    byte -= options.lp * 9;
    options.lc = byte;

    options.lc + options.lp > LZMA_LCLP_MAX
}

fn get_dist_slot(dist: u32) -> u32 {
    if dist <= 4 {
        dist
    } else {
        let i = 31 - dist.leading_zeros();
        (i + i) + ((dist >> (i - 1)) & 1)
    }
}

fn write32le(out: *mut u8, value: u32) {
    unsafe {
        *out.add(0) = value as u8;
        *out.add(1) = (value >> 8) as u8;
        *out.add(2) = (value >> 16) as u8;
        *out.add(3) = (value >> 24) as u8;
    }
}

fn read32le(input: *const u8) -> u32 {
    unsafe {
        u32::from(*input.add(0))
            | (u32::from(*input.add(1)) << 8)
            | (u32::from(*input.add(2)) << 16)
            | (u32::from(*input.add(3)) << 24)
    }
}

pub(crate) unsafe fn properties_size_impl(size: *mut u32, filter: *const lzma_filter) -> lzma_ret {
    if size.is_null() || filter.is_null() {
        return LZMA_PROG_ERROR;
    }

    match (*filter).id {
        LZMA_FILTER_LZMA1 | LZMA_FILTER_LZMA1EXT => {
            *size = 5;
            LZMA_OK
        }
        LZMA_FILTER_LZMA2 => {
            *size = 1;
            LZMA_OK
        }
        id if is_bcj_filter(id) => {
            let opt = (*filter).options.cast::<lzma_options_bcj>();
            *size = if opt.is_null() || (*opt).start_offset == 0 {
                0
            } else {
                4
            };
            LZMA_OK
        }
        LZMA_FILTER_DELTA => {
            *size = 1;
            LZMA_OK
        }
        _ => {
            if (*filter).id <= crate::internal::common::LZMA_VLI_MAX {
                LZMA_OPTIONS_ERROR
            } else {
                LZMA_PROG_ERROR
            }
        }
    }
}

pub(crate) unsafe fn properties_encode_impl(
    filter: *const lzma_filter,
    props: *mut u8,
) -> lzma_ret {
    if filter.is_null() {
        return LZMA_PROG_ERROR;
    }

    match (*filter).id {
        LZMA_FILTER_LZMA1 | LZMA_FILTER_LZMA1EXT => {
            let options = (*filter).options.cast::<lzma_options_lzma>();
            if options.is_null() || props.is_null() {
                return LZMA_PROG_ERROR;
            }

            let options = &*options;
            let Some(byte) = lzma_lclppb_encode(options) else {
                return LZMA_PROG_ERROR;
            };

            *props = byte;
            write32le(props.add(1), options.dict_size);
            LZMA_OK
        }
        LZMA_FILTER_LZMA2 => {
            let options = (*filter).options.cast::<lzma_options_lzma>();
            if options.is_null() || props.is_null() {
                return LZMA_PROG_ERROR;
            }

            let options = &*options;
            let mut d = cmp::max(options.dict_size, LZMA_DICT_SIZE_MIN);
            d -= 1;
            d |= d >> 2;
            d |= d >> 3;
            d |= d >> 4;
            d |= d >> 8;
            d |= d >> 16;

            *props = if d == u32::MAX {
                40
            } else {
                (get_dist_slot(d + 1) - 24) as u8
            };

            LZMA_OK
        }
        id if is_bcj_filter(id) => {
            let opt = (*filter).options.cast::<lzma_options_bcj>();
            if opt.is_null() || (*opt).start_offset == 0 {
                return LZMA_OK;
            }

            if props.is_null() {
                return LZMA_PROG_ERROR;
            }

            write32le(props, (*opt).start_offset);
            LZMA_OK
        }
        LZMA_FILTER_DELTA => {
            let options = (*filter).options.cast::<lzma_options_delta>();
            if options.is_null() || props.is_null() {
                return LZMA_PROG_ERROR;
            }

            let options = &*options;
            if options.r#type != LZMA_DELTA_TYPE_BYTE
                || options.dist < LZMA_DELTA_DIST_MIN
                || options.dist > LZMA_DELTA_DIST_MAX
            {
                return LZMA_PROG_ERROR;
            }

            *props = (options.dist - LZMA_DELTA_DIST_MIN) as u8;
            LZMA_OK
        }
        _ => LZMA_PROG_ERROR,
    }
}

pub(crate) unsafe fn properties_decode_impl(
    filter: *mut lzma_filter,
    allocator: *const lzma_allocator,
    props: *const u8,
    props_size: usize,
) -> lzma_ret {
    if filter.is_null() {
        return LZMA_PROG_ERROR;
    }

    (*filter).options = ptr::null_mut();
    if props_size != 0 && props.is_null() {
        return LZMA_PROG_ERROR;
    }

    match (*filter).id {
        LZMA_FILTER_LZMA1 | LZMA_FILTER_LZMA1EXT => {
            if props_size != 5 {
                return LZMA_OPTIONS_ERROR;
            }

            let opt = lzma_alloc_zero(size_of::<lzma_options_lzma>(), allocator)
                .cast::<lzma_options_lzma>();
            if opt.is_null() {
                return LZMA_MEM_ERROR;
            }

            if lzma_lclppb_decode(&mut *opt, *props) {
                lzma_free(opt.cast(), allocator);
                return LZMA_OPTIONS_ERROR;
            }

            (*opt).dict_size = read32le(props.add(1));
            (*opt).preset_dict = ptr::null();
            (*opt).preset_dict_size = 0;
            (*filter).options = opt.cast();
            LZMA_OK
        }
        LZMA_FILTER_LZMA2 => {
            if props_size != 1 {
                return LZMA_OPTIONS_ERROR;
            }

            if (*props & 0xC0) != 0 || *props > 40 {
                return LZMA_OPTIONS_ERROR;
            }

            let opt = lzma_alloc_zero(size_of::<lzma_options_lzma>(), allocator)
                .cast::<lzma_options_lzma>();
            if opt.is_null() {
                return LZMA_MEM_ERROR;
            }

            (*opt).dict_size = if *props == 40 {
                u32::MAX
            } else {
                let mut dict_size = 2u32 | u32::from(*props & 1);
                dict_size <<= u32::from(*props / 2) + 11;
                dict_size
            };
            (*opt).preset_dict = ptr::null();
            (*opt).preset_dict_size = 0;
            (*filter).options = opt.cast();
            LZMA_OK
        }
        id if is_bcj_filter(id) => {
            if props_size == 0 {
                return LZMA_OK;
            }

            if props_size != 4 {
                return LZMA_OPTIONS_ERROR;
            }

            let opt = lzma_alloc_zero(size_of::<lzma_options_bcj>(), allocator)
                .cast::<lzma_options_bcj>();
            if opt.is_null() {
                return LZMA_MEM_ERROR;
            }

            (*opt).start_offset = read32le(props);
            if (*opt).start_offset == 0 {
                lzma_free(opt.cast(), allocator);
            } else {
                (*filter).options = opt.cast();
            }
            LZMA_OK
        }
        LZMA_FILTER_DELTA => {
            if props_size != 1 {
                return LZMA_OPTIONS_ERROR;
            }

            let opt = lzma_alloc_zero(size_of::<lzma_options_delta>(), allocator)
                .cast::<lzma_options_delta>();
            if opt.is_null() {
                return LZMA_MEM_ERROR;
            }

            (*opt).r#type = LZMA_DELTA_TYPE_BYTE;
            (*opt).dist = u32::from(*props) + 1;
            (*filter).options = opt.cast();
            LZMA_OK
        }
        _ => LZMA_OPTIONS_ERROR,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ffi::types::lzma_filter;
    use crate::internal::common::{
        LZMA_LC_DEFAULT, LZMA_LP_DEFAULT, LZMA_MF_BT4, LZMA_MF_HC4, LZMA_MODE_FAST,
        LZMA_MODE_NORMAL, LZMA_PB_DEFAULT,
    };

    #[test]
    fn lzma2_property_round_trip_matches_dict_encoding() {
        let options = lzma_options_lzma {
            dict_size: 1 << 23,
            preset_dict: ptr::null(),
            preset_dict_size: 0,
            lc: LZMA_LC_DEFAULT,
            lp: LZMA_LP_DEFAULT,
            pb: LZMA_PB_DEFAULT,
            mode: LZMA_MODE_NORMAL,
            nice_len: 64,
            mf: LZMA_MF_BT4,
            depth: 0,
            ext_flags: 0,
            ext_size_low: 0,
            ext_size_high: 0,
            reserved_int4: 0,
            reserved_int5: 0,
            reserved_int6: 0,
            reserved_int7: 0,
            reserved_int8: 0,
            reserved_enum1: 0,
            reserved_enum2: 0,
            reserved_enum3: 0,
            reserved_enum4: 0,
            reserved_ptr1: ptr::null_mut(),
            reserved_ptr2: ptr::null_mut(),
        };
        let filter = lzma_filter {
            id: LZMA_FILTER_LZMA2,
            options: (&options as *const lzma_options_lzma).cast_mut().cast(),
        };
        let mut props = [0u8; 1];
        let mut decoded = lzma_filter {
            id: LZMA_FILTER_LZMA2,
            options: ptr::null_mut(),
        };

        unsafe {
            assert_eq!(properties_encode_impl(&filter, props.as_mut_ptr()), LZMA_OK);
            assert_eq!(props[0], 22);
            assert_eq!(
                properties_decode_impl(&mut decoded, ptr::null(), props.as_ptr(), props.len()),
                LZMA_OK
            );
        }

        let decoded = unsafe { &*decoded.options.cast::<lzma_options_lzma>() };
        assert_eq!(decoded.dict_size, options.dict_size);
        unsafe {
            lzma_free(decoded as *const lzma_options_lzma as *mut _, ptr::null());
        }
    }

    #[test]
    fn lzma1_lclppb_validation_rejects_invalid_sum() {
        let options = lzma_options_lzma {
            dict_size: 4096,
            preset_dict: ptr::null(),
            preset_dict_size: 0,
            lc: 3,
            lp: 3,
            pb: 2,
            mode: LZMA_MODE_FAST,
            nice_len: 32,
            mf: LZMA_MF_HC4,
            depth: 0,
            ext_flags: 0,
            ext_size_low: 0,
            ext_size_high: 0,
            reserved_int4: 0,
            reserved_int5: 0,
            reserved_int6: 0,
            reserved_int7: 0,
            reserved_int8: 0,
            reserved_enum1: 0,
            reserved_enum2: 0,
            reserved_enum3: 0,
            reserved_enum4: 0,
            reserved_ptr1: ptr::null_mut(),
            reserved_ptr2: ptr::null_mut(),
        };
        let filter = lzma_filter {
            id: LZMA_FILTER_LZMA1,
            options: (&options as *const lzma_options_lzma).cast_mut().cast(),
        };
        let mut props = [0u8; 5];

        unsafe {
            assert_eq!(
                properties_encode_impl(&filter, props.as_mut_ptr()),
                LZMA_PROG_ERROR
            );
        }
    }
}
