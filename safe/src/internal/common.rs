use core::ffi::c_void;
use core::ptr;

use crate::ffi::types::{
    lzma_action, lzma_allocator, lzma_bool, lzma_check, lzma_match_finder, lzma_mode, lzma_ret,
    lzma_vli, LZMA_RESERVED_ENUM,
};

pub(crate) const LZMA_RUN: lzma_action = 0;
pub(crate) const LZMA_SYNC_FLUSH: lzma_action = 1;
pub(crate) const LZMA_FULL_FLUSH: lzma_action = 2;
pub(crate) const LZMA_FINISH: lzma_action = 3;
pub(crate) const LZMA_FULL_BARRIER: lzma_action = 4;
pub(crate) const LZMA_ACTION_MAX: usize = LZMA_FULL_BARRIER as usize;
pub(crate) const ACTION_COUNT: usize = LZMA_ACTION_MAX + 1;

pub(crate) const LZMA_RET_INTERNAL1: lzma_ret = 101;
pub(crate) const LZMA_TIMED_OUT: lzma_ret = LZMA_RET_INTERNAL1;

pub(crate) const LZMA_CHECK_CRC32: lzma_check = 1;
pub(crate) const LZMA_CHECK_CRC64: lzma_check = 4;
pub(crate) const LZMA_CHECK_SHA256: lzma_check = 10;
pub(crate) const LZMA_CHECK_ID_MAX: usize = 15;

pub(crate) const LZMA_MF_HC3: lzma_match_finder = 0x03;
pub(crate) const LZMA_MF_HC4: lzma_match_finder = 0x04;
pub(crate) const LZMA_MF_BT2: lzma_match_finder = 0x12;
pub(crate) const LZMA_MF_BT3: lzma_match_finder = 0x13;
pub(crate) const LZMA_MF_BT4: lzma_match_finder = 0x14;

pub(crate) const LZMA_MODE_FAST: lzma_mode = 1;
pub(crate) const LZMA_MODE_NORMAL: lzma_mode = 2;

pub(crate) const LZMA_PRESET_LEVEL_MASK: u32 = 0x1F;
pub(crate) const LZMA_PRESET_EXTREME: u32 = 1u32 << 31;

pub(crate) const LZMA_LC_DEFAULT: u32 = 3;
pub(crate) const LZMA_LP_DEFAULT: u32 = 0;
pub(crate) const LZMA_PB_DEFAULT: u32 = 2;

pub(crate) const LZMA_VLI_MAX: lzma_vli = u64::MAX / 2;
pub(crate) const LZMA_VLI_BYTES_MAX: usize = 9;

#[inline]
pub(crate) const fn lzma_bool(value: bool) -> lzma_bool {
    value as lzma_bool
}

#[inline]
pub(crate) const fn action_index(action: lzma_action) -> Option<usize> {
    match action {
        LZMA_RUN | LZMA_SYNC_FLUSH | LZMA_FULL_FLUSH | LZMA_FINISH | LZMA_FULL_BARRIER => {
            Some(action as usize)
        }
        _ => None,
    }
}

#[inline]
pub(crate) const fn default_supported_actions() -> [bool; ACTION_COUNT] {
    [false; ACTION_COUNT]
}

#[inline]
pub(crate) const fn all_supported_actions() -> [bool; ACTION_COUNT] {
    [true; ACTION_COUNT]
}

#[inline]
pub(crate) unsafe fn lzma_alloc(size: usize, allocator: *const lzma_allocator) -> *mut c_void {
    let size = size.max(1);

    if !allocator.is_null() {
        if let Some(alloc) = (*allocator).alloc {
            return alloc((*allocator).opaque, 1, size);
        }
    }

    libc::malloc(size)
}

#[inline]
pub(crate) unsafe fn lzma_alloc_zero(
    size: usize,
    allocator: *const lzma_allocator,
) -> *mut c_void {
    let size = size.max(1);

    if !allocator.is_null() {
        if let Some(alloc) = (*allocator).alloc {
            let ptr = alloc((*allocator).opaque, 1, size);
            if !ptr.is_null() {
                ptr::write_bytes(ptr.cast::<u8>(), 0, size);
            }
            return ptr;
        }
    }

    libc::calloc(1, size)
}

#[inline]
pub(crate) unsafe fn lzma_free(ptr: *mut c_void, allocator: *const lzma_allocator) {
    if !allocator.is_null() {
        if let Some(free) = (*allocator).free {
            free((*allocator).opaque, ptr);
            return;
        }
    }

    libc::free(ptr);
}

#[inline]
pub(crate) const fn reserved_members_are_clear(strm: &crate::ffi::types::lzma_stream) -> bool {
    strm.reserved_ptr1.is_null()
        && strm.reserved_ptr2.is_null()
        && strm.reserved_ptr3.is_null()
        && strm.reserved_ptr4.is_null()
        && strm.reserved_int2 == 0
        && strm.reserved_int3 == 0
        && strm.reserved_int4 == 0
        && strm.reserved_enum1 == LZMA_RESERVED_ENUM
        && strm.reserved_enum2 == LZMA_RESERVED_ENUM
}

#[cfg(test)]
mod tests {
    use core::ffi::c_void;
    use core::ptr;

    use super::*;
    use crate::ffi::types::lzma_allocator;

    #[derive(Default)]
    struct AllocRecorder {
        nmemb: usize,
        size: usize,
        freed_ptr: *mut c_void,
        backing: *mut c_void,
    }

    unsafe extern "C" fn record_alloc(
        opaque: *mut c_void,
        nmemb: usize,
        size: usize,
    ) -> *mut c_void {
        let recorder = &mut *opaque.cast::<AllocRecorder>();
        recorder.nmemb = nmemb;
        recorder.size = size;
        recorder.backing = libc::malloc(size.max(1));
        recorder.backing
    }

    unsafe extern "C" fn record_free(opaque: *mut c_void, ptr: *mut c_void) {
        let recorder = &mut *opaque.cast::<AllocRecorder>();
        recorder.freed_ptr = ptr;
        libc::free(ptr);
    }

    #[test]
    fn zero_sized_custom_allocations_use_one_byte() {
        let mut recorder = AllocRecorder::default();
        let allocator = lzma_allocator {
            alloc: Some(record_alloc),
            free: Some(record_free),
            opaque: (&mut recorder as *mut AllocRecorder).cast(),
        };

        unsafe {
            let ptr = lzma_alloc(0, &allocator);
            assert!(!ptr.is_null());
            assert_eq!(recorder.nmemb, 1);
            assert_eq!(recorder.size, 1);
            lzma_free(ptr, &allocator);
        }

        assert_eq!(recorder.freed_ptr, recorder.backing);
    }

    #[test]
    fn zero_sized_custom_zero_allocations_are_cleared() {
        let mut recorder = AllocRecorder::default();
        let allocator = lzma_allocator {
            alloc: Some(record_alloc),
            free: Some(record_free),
            opaque: (&mut recorder as *mut AllocRecorder).cast(),
        };

        unsafe {
            let ptr = lzma_alloc_zero(0, &allocator).cast::<u8>();
            assert!(!ptr.is_null());
            assert_eq!(recorder.size, 1);
            assert_eq!(*ptr, 0);
            lzma_free(ptr.cast(), &allocator);
        }
    }

    #[test]
    fn zero_sized_default_allocation_returns_memory() {
        unsafe {
            let ptr = lzma_alloc(0, ptr::null());
            assert!(!ptr.is_null());
            lzma_free(ptr, ptr::null());
        }
    }

    #[test]
    fn reserved_field_validator_matches_stream_init() {
        let strm = crate::ffi::types::LZMA_STREAM_INIT;
        assert!(reserved_members_are_clear(&strm));
    }
}
