use crate::ffi::types::{
    lzma_allocator, lzma_filter, lzma_ret, lzma_vli, LZMA_DATA_ERROR, LZMA_OK, LZMA_PROG_ERROR,
};
use crate::internal::filter::common::LZMA_FILTER_RESERVED_START;
use crate::internal::filter::properties::{
    properties_decode_impl, properties_encode_impl, properties_size_impl,
};
use crate::internal::vli::{lzma_vli_decode_impl, lzma_vli_encode_impl, lzma_vli_size_impl};

pub(crate) unsafe fn filter_flags_size_impl(
    size: *mut u32,
    filter: *const lzma_filter,
) -> lzma_ret {
    if size.is_null() || filter.is_null() {
        return LZMA_PROG_ERROR;
    }

    if (*filter).id >= LZMA_FILTER_RESERVED_START {
        return LZMA_PROG_ERROR;
    }

    let ret = properties_size_impl(size, filter);
    if ret != LZMA_OK {
        return ret;
    }

    *size += lzma_vli_size_impl((*filter).id) + lzma_vli_size_impl(lzma_vli::from(*size));
    LZMA_OK
}

pub(crate) unsafe fn filter_flags_encode_impl(
    filter: *const lzma_filter,
    out: *mut u8,
    out_pos: *mut usize,
    out_size: usize,
) -> lzma_ret {
    if filter.is_null() {
        return LZMA_PROG_ERROR;
    }

    if (*filter).id >= LZMA_FILTER_RESERVED_START {
        return LZMA_PROG_ERROR;
    }

    let ret = lzma_vli_encode_impl((*filter).id, core::ptr::null_mut(), out, out_pos, out_size);
    if ret != LZMA_OK {
        return ret;
    }

    let mut props_size = 0u32;
    let ret = properties_size_impl(&mut props_size, filter);
    if ret != LZMA_OK {
        return ret;
    }

    let ret = lzma_vli_encode_impl(
        lzma_vli::from(props_size),
        core::ptr::null_mut(),
        out,
        out_pos,
        out_size,
    );
    if ret != LZMA_OK {
        return ret;
    }

    if out_size - *out_pos < usize::try_from(props_size).unwrap() {
        return LZMA_PROG_ERROR;
    }

    let ret = properties_encode_impl(filter, out.add(*out_pos));
    if ret != LZMA_OK {
        return ret;
    }

    *out_pos += usize::try_from(props_size).unwrap();
    LZMA_OK
}

pub(crate) unsafe fn filter_flags_decode_impl(
    filter: *mut lzma_filter,
    allocator: *const lzma_allocator,
    input: *const u8,
    in_pos: *mut usize,
    in_size: usize,
) -> lzma_ret {
    if filter.is_null() || input.is_null() || in_pos.is_null() {
        return LZMA_PROG_ERROR;
    }

    (*filter).options = core::ptr::null_mut();

    let ret = lzma_vli_decode_impl(
        &mut (*filter).id,
        core::ptr::null_mut(),
        input,
        in_pos,
        in_size,
    );
    if ret != LZMA_OK {
        return ret;
    }

    if (*filter).id >= LZMA_FILTER_RESERVED_START {
        return LZMA_DATA_ERROR;
    }

    let mut props_size = 0u64;
    let ret = lzma_vli_decode_impl(
        &mut props_size,
        core::ptr::null_mut(),
        input,
        in_pos,
        in_size,
    );
    if ret != LZMA_OK {
        return ret;
    }

    let available = in_size.saturating_sub(*in_pos) as u64;
    if props_size > available {
        return LZMA_DATA_ERROR;
    }

    let props_size = usize::try_from(props_size).unwrap();
    let ret = properties_decode_impl(filter, allocator, input.add(*in_pos), props_size);
    *in_pos += props_size;
    ret
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ffi::types::{lzma_filter, lzma_options_bcj, LZMA_OPTIONS_ERROR};
    use crate::internal::filter::common::LZMA_FILTER_X86;

    #[test]
    fn bcj_filter_flags_round_trip_with_offset() {
        let options = lzma_options_bcj { start_offset: 257 };
        let filter = lzma_filter {
            id: LZMA_FILTER_X86,
            options: (&options as *const lzma_options_bcj).cast_mut().cast(),
        };
        let mut size = 0u32;
        let mut buffer = [0u8; 16];
        let mut out_pos = 0usize;
        let mut in_pos = 0usize;
        let mut decoded = lzma_filter {
            id: 0,
            options: core::ptr::null_mut(),
        };

        unsafe {
            assert_eq!(filter_flags_size_impl(&mut size, &filter), LZMA_OK);
            assert_eq!(
                filter_flags_encode_impl(&filter, buffer.as_mut_ptr(), &mut out_pos, buffer.len()),
                LZMA_OK
            );
            assert_eq!(
                filter_flags_decode_impl(
                    &mut decoded,
                    core::ptr::null(),
                    buffer.as_ptr(),
                    &mut in_pos,
                    out_pos
                ),
                LZMA_OK
            );
        }

        let decoded_options = unsafe { &*decoded.options.cast::<lzma_options_bcj>() };
        assert_eq!(decoded.id, LZMA_FILTER_X86);
        assert_eq!(decoded_options.start_offset, 257);
        unsafe {
            crate::internal::common::lzma_free(decoded.options.cast(), core::ptr::null());
        }
    }

    #[test]
    fn reserved_filter_id_is_rejected() {
        let filter = lzma_filter {
            id: LZMA_FILTER_RESERVED_START,
            options: core::ptr::null_mut(),
        };
        let mut size = 0u32;
        let mut out_pos = 0usize;
        let mut buffer = [0u8; 8];

        unsafe {
            assert_eq!(filter_flags_size_impl(&mut size, &filter), LZMA_PROG_ERROR);
            assert_eq!(
                filter_flags_encode_impl(&filter, buffer.as_mut_ptr(), &mut out_pos, buffer.len()),
                LZMA_PROG_ERROR
            );
        }

        let _ = LZMA_OPTIONS_ERROR;
    }
}
