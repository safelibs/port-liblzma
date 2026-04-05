use core::mem::size_of;
use core::ptr;

use crate::ffi::types::{
    lzma_allocator, lzma_bool, lzma_filter, lzma_options_bcj, lzma_options_delta,
    lzma_options_lzma, lzma_ret, lzma_vli, LZMA_FILTERS_MAX, LZMA_MEM_ERROR, LZMA_OK,
    LZMA_OPTIONS_ERROR, LZMA_PROG_ERROR, LZMA_VLI_UNKNOWN,
};
use crate::internal::common::{lzma_alloc, lzma_bool as to_lzma_bool, lzma_free};

pub(crate) const LZMA_FILTER_DELTA: lzma_vli = 0x03;
pub(crate) const LZMA_FILTER_X86: lzma_vli = 0x04;
pub(crate) const LZMA_FILTER_POWERPC: lzma_vli = 0x05;
pub(crate) const LZMA_FILTER_IA64: lzma_vli = 0x06;
pub(crate) const LZMA_FILTER_ARM: lzma_vli = 0x07;
pub(crate) const LZMA_FILTER_ARMTHUMB: lzma_vli = 0x08;
pub(crate) const LZMA_FILTER_SPARC: lzma_vli = 0x09;
pub(crate) const LZMA_FILTER_ARM64: lzma_vli = 0x0A;
pub(crate) const LZMA_FILTER_LZMA2: lzma_vli = 0x21;
pub(crate) const LZMA_FILTER_LZMA1: lzma_vli = 0x4000_0000_0000_0001;
pub(crate) const LZMA_FILTER_LZMA1EXT: lzma_vli = 0x4000_0000_0000_0002;
pub(crate) const LZMA_FILTER_RESERVED_START: lzma_vli = 1u64 << 62;

#[derive(Copy, Clone)]
pub(crate) struct Feature {
    pub(crate) id: lzma_vli,
    pub(crate) options_size: usize,
    pub(crate) non_last_ok: bool,
    pub(crate) last_ok: bool,
    pub(crate) changes_size: bool,
}

pub(crate) const FEATURES: [Feature; 11] = [
    Feature {
        id: LZMA_FILTER_LZMA1,
        options_size: size_of::<lzma_options_lzma>(),
        non_last_ok: false,
        last_ok: true,
        changes_size: true,
    },
    Feature {
        id: LZMA_FILTER_LZMA1EXT,
        options_size: size_of::<lzma_options_lzma>(),
        non_last_ok: false,
        last_ok: true,
        changes_size: true,
    },
    Feature {
        id: LZMA_FILTER_LZMA2,
        options_size: size_of::<lzma_options_lzma>(),
        non_last_ok: false,
        last_ok: true,
        changes_size: true,
    },
    Feature {
        id: LZMA_FILTER_X86,
        options_size: size_of::<lzma_options_bcj>(),
        non_last_ok: true,
        last_ok: false,
        changes_size: false,
    },
    Feature {
        id: LZMA_FILTER_POWERPC,
        options_size: size_of::<lzma_options_bcj>(),
        non_last_ok: true,
        last_ok: false,
        changes_size: false,
    },
    Feature {
        id: LZMA_FILTER_IA64,
        options_size: size_of::<lzma_options_bcj>(),
        non_last_ok: true,
        last_ok: false,
        changes_size: false,
    },
    Feature {
        id: LZMA_FILTER_ARM,
        options_size: size_of::<lzma_options_bcj>(),
        non_last_ok: true,
        last_ok: false,
        changes_size: false,
    },
    Feature {
        id: LZMA_FILTER_ARMTHUMB,
        options_size: size_of::<lzma_options_bcj>(),
        non_last_ok: true,
        last_ok: false,
        changes_size: false,
    },
    Feature {
        id: LZMA_FILTER_ARM64,
        options_size: size_of::<lzma_options_bcj>(),
        non_last_ok: true,
        last_ok: false,
        changes_size: false,
    },
    Feature {
        id: LZMA_FILTER_SPARC,
        options_size: size_of::<lzma_options_bcj>(),
        non_last_ok: true,
        last_ok: false,
        changes_size: false,
    },
    Feature {
        id: LZMA_FILTER_DELTA,
        options_size: size_of::<lzma_options_delta>(),
        non_last_ok: true,
        last_ok: false,
        changes_size: false,
    },
];

pub(crate) fn feature_by_id(id: lzma_vli) -> Option<&'static Feature> {
    FEATURES.iter().find(|feature| feature.id == id)
}

#[inline]
pub(crate) fn encoder_is_supported(id: lzma_vli) -> lzma_bool {
    to_lzma_bool(feature_by_id(id).is_some())
}

#[inline]
pub(crate) fn decoder_is_supported(id: lzma_vli) -> lzma_bool {
    to_lzma_bool(feature_by_id(id).is_some())
}

pub(crate) unsafe fn filters_copy_impl(
    src: *const lzma_filter,
    real_dest: *mut lzma_filter,
    allocator: *const lzma_allocator,
) -> lzma_ret {
    if src.is_null() || real_dest.is_null() {
        return LZMA_PROG_ERROR;
    }

    let mut dest = [lzma_filter {
        id: LZMA_VLI_UNKNOWN,
        options: ptr::null_mut(),
    }; LZMA_FILTERS_MAX + 1];

    let mut i = 0usize;
    let ret = loop {
        let filter = *src.add(i);
        if filter.id == LZMA_VLI_UNKNOWN {
            break LZMA_OK;
        }

        if i == LZMA_FILTERS_MAX {
            break LZMA_OPTIONS_ERROR;
        }

        dest[i].id = filter.id;
        if filter.options.is_null() {
            dest[i].options = ptr::null_mut();
        } else {
            let Some(feature) = feature_by_id(filter.id) else {
                break LZMA_OPTIONS_ERROR;
            };

            let copied = lzma_alloc(feature.options_size, allocator);
            if copied.is_null() {
                break LZMA_MEM_ERROR;
            }

            ptr::copy_nonoverlapping(
                filter.options.cast::<u8>(),
                copied.cast::<u8>(),
                feature.options_size,
            );
            dest[i].options = copied;
        }

        i += 1;
    };

    if ret != LZMA_OK {
        while i > 0 {
            i -= 1;
            lzma_free(dest[i].options.cast(), allocator);
        }
        return ret;
    }

    dest[i].id = LZMA_VLI_UNKNOWN;
    dest[i].options = ptr::null_mut();
    ptr::copy_nonoverlapping(dest.as_ptr(), real_dest, i + 1);
    LZMA_OK
}

pub(crate) unsafe fn filters_free_impl(
    filters: *mut lzma_filter,
    allocator: *const lzma_allocator,
) {
    if filters.is_null() {
        return;
    }

    for i in 0.. {
        let filter = filters.add(i);
        if (*filter).id == LZMA_VLI_UNKNOWN {
            return;
        }

        if i == LZMA_FILTERS_MAX {
            debug_assert!(false);
            return;
        }

        lzma_free((*filter).options.cast(), allocator);
        (*filter).options = ptr::null_mut();
        (*filter).id = LZMA_VLI_UNKNOWN;
    }
}

pub(crate) unsafe fn validate_chain_impl(
    filters: *const lzma_filter,
    count: *mut usize,
) -> lzma_ret {
    if filters.is_null() || (*filters).id == LZMA_VLI_UNKNOWN {
        return LZMA_PROG_ERROR;
    }

    let mut changes_size_count = 0usize;
    let mut non_last_ok = true;
    let mut last_ok;
    let mut i = 0usize;

    loop {
        let filter = *filters.add(i);
        let Some(feature) = feature_by_id(filter.id) else {
            return LZMA_OPTIONS_ERROR;
        };

        if !non_last_ok {
            return LZMA_OPTIONS_ERROR;
        }

        non_last_ok = feature.non_last_ok;
        last_ok = feature.last_ok;
        changes_size_count += usize::from(feature.changes_size);

        i += 1;
        if (*filters.add(i)).id == LZMA_VLI_UNKNOWN {
            break;
        }
    }

    if i > LZMA_FILTERS_MAX || !last_ok || changes_size_count > 3 {
        return LZMA_OPTIONS_ERROR;
    }

    if !count.is_null() {
        *count = i;
    }

    LZMA_OK
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ffi::types::{lzma_filter, lzma_options_bcj};

    #[test]
    fn copy_allows_unknown_placeholders_with_null_options() {
        let src = [
            lzma_filter {
                id: 0x1234,
                options: ptr::null_mut(),
            },
            lzma_filter {
                id: LZMA_VLI_UNKNOWN,
                options: ptr::null_mut(),
            },
        ];
        let mut dest = [lzma_filter {
            id: 0,
            options: ptr::null_mut(),
        }; LZMA_FILTERS_MAX + 1];

        unsafe {
            assert_eq!(
                filters_copy_impl(src.as_ptr(), dest.as_mut_ptr(), ptr::null()),
                LZMA_OK
            );
        }

        assert_eq!(dest[0].id, 0x1234);
        assert!(dest[0].options.is_null());
        assert_eq!(dest[1].id, LZMA_VLI_UNKNOWN);
    }

    #[test]
    fn validate_chain_rejects_non_last_lzma2() {
        let filters = [
            lzma_filter {
                id: LZMA_FILTER_LZMA2,
                options: ptr::null_mut(),
            },
            lzma_filter {
                id: LZMA_FILTER_X86,
                options: ptr::null_mut(),
            },
            lzma_filter {
                id: LZMA_VLI_UNKNOWN,
                options: ptr::null_mut(),
            },
        ];

        unsafe {
            assert_eq!(
                validate_chain_impl(filters.as_ptr(), ptr::null_mut()),
                LZMA_OPTIONS_ERROR
            );
        }
    }

    #[test]
    fn filters_free_clears_entries() {
        let mut options = Box::new(lzma_options_bcj { start_offset: 7 });
        let mut filters = [
            lzma_filter {
                id: LZMA_FILTER_X86,
                options: (&mut *options as *mut lzma_options_bcj).cast(),
            },
            lzma_filter {
                id: LZMA_VLI_UNKNOWN,
                options: ptr::null_mut(),
            },
            lzma_filter {
                id: 0,
                options: ptr::null_mut(),
            },
            lzma_filter {
                id: 0,
                options: ptr::null_mut(),
            },
            lzma_filter {
                id: 0,
                options: ptr::null_mut(),
            },
        ];

        core::mem::forget(options);
        unsafe {
            filters_free_impl(filters.as_mut_ptr(), ptr::null());
        }

        assert_eq!(filters[0].id, LZMA_VLI_UNKNOWN);
        assert!(filters[0].options.is_null());
    }
}
