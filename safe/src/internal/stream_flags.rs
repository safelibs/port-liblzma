use core::ptr;

use crate::ffi::types::{
    lzma_ret, lzma_stream_flags, lzma_vli, LZMA_DATA_ERROR, LZMA_FORMAT_ERROR, LZMA_OK,
    LZMA_OPTIONS_ERROR, LZMA_PROG_ERROR, LZMA_VLI_UNKNOWN,
};
use crate::internal::check::crc32;
use crate::internal::common::LZMA_CHECK_ID_MAX;

pub(crate) const LZMA_STREAM_FLAGS_SIZE: usize = 2;
pub(crate) const LZMA_STREAM_HEADER_SIZE: usize = 12;
pub(crate) const LZMA_BACKWARD_SIZE_MIN: lzma_vli = 4;
pub(crate) const LZMA_BACKWARD_SIZE_MAX: lzma_vli = 1u64 << 34;

const HEADER_MAGIC: [u8; 6] = [0xFD, 0x37, 0x7A, 0x58, 0x5A, 0x00];
const FOOTER_MAGIC: [u8; 2] = [0x59, 0x5A];

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

fn is_backward_size_valid(flags: &lzma_stream_flags) -> bool {
    flags.backward_size >= LZMA_BACKWARD_SIZE_MIN
        && flags.backward_size <= LZMA_BACKWARD_SIZE_MAX
        && (flags.backward_size & 3) == 0
}

fn stream_flags_encode(flags: &lzma_stream_flags, out: *mut u8) -> bool {
    if (flags.check as u32) > LZMA_CHECK_ID_MAX as u32 {
        return true;
    }

    unsafe {
        *out.add(0) = 0;
        *out.add(1) = flags.check as u8;
    }
    false
}

fn stream_flags_decode(flags: &mut lzma_stream_flags, input: *const u8) -> bool {
    unsafe {
        if *input.add(0) != 0 || (*input.add(1) & 0xF0) != 0 {
            return true;
        }

        flags.version = 0;
        flags.check = i32::from(*input.add(1) & 0x0F);
    }
    false
}

pub(crate) unsafe fn stream_header_encode_impl(
    options: *const lzma_stream_flags,
    out: *mut u8,
) -> lzma_ret {
    if options.is_null() || out.is_null() {
        return LZMA_PROG_ERROR;
    }

    let options = &*options;
    if options.version != 0 {
        return LZMA_OPTIONS_ERROR;
    }

    ptr::copy_nonoverlapping(HEADER_MAGIC.as_ptr(), out, HEADER_MAGIC.len());
    if stream_flags_encode(options, out.add(HEADER_MAGIC.len())) {
        return LZMA_PROG_ERROR;
    }

    let crc = crc32::crc32(
        core::slice::from_raw_parts(out.add(HEADER_MAGIC.len()), LZMA_STREAM_FLAGS_SIZE),
        0,
    );
    write32le(out.add(HEADER_MAGIC.len() + LZMA_STREAM_FLAGS_SIZE), crc);
    LZMA_OK
}

pub(crate) unsafe fn stream_footer_encode_impl(
    options: *const lzma_stream_flags,
    out: *mut u8,
) -> lzma_ret {
    if options.is_null() || out.is_null() {
        return LZMA_PROG_ERROR;
    }

    let options = &*options;
    if options.version != 0 {
        return LZMA_OPTIONS_ERROR;
    }

    if !is_backward_size_valid(options) {
        return LZMA_PROG_ERROR;
    }

    write32le(out.add(4), (options.backward_size / 4 - 1) as u32);
    if stream_flags_encode(options, out.add(8)) {
        return LZMA_PROG_ERROR;
    }

    let crc = crc32::crc32(core::slice::from_raw_parts(out.add(4), 6), 0);
    write32le(out, crc);
    ptr::copy_nonoverlapping(FOOTER_MAGIC.as_ptr(), out.add(10), FOOTER_MAGIC.len());
    LZMA_OK
}

pub(crate) unsafe fn stream_header_decode_impl(
    options: *mut lzma_stream_flags,
    input: *const u8,
) -> lzma_ret {
    if options.is_null() || input.is_null() {
        return LZMA_PROG_ERROR;
    }

    if core::slice::from_raw_parts(input, HEADER_MAGIC.len()) != HEADER_MAGIC {
        return LZMA_FORMAT_ERROR;
    }

    let crc = crc32::crc32(
        core::slice::from_raw_parts(input.add(HEADER_MAGIC.len()), LZMA_STREAM_FLAGS_SIZE),
        0,
    );
    if crc != read32le(input.add(HEADER_MAGIC.len() + LZMA_STREAM_FLAGS_SIZE)) {
        return LZMA_DATA_ERROR;
    }

    if stream_flags_decode(&mut *options, input.add(HEADER_MAGIC.len())) {
        return LZMA_OPTIONS_ERROR;
    }

    (*options).backward_size = LZMA_VLI_UNKNOWN;
    LZMA_OK
}

pub(crate) unsafe fn stream_footer_decode_impl(
    options: *mut lzma_stream_flags,
    input: *const u8,
) -> lzma_ret {
    if options.is_null() || input.is_null() {
        return LZMA_PROG_ERROR;
    }

    if core::slice::from_raw_parts(input.add(10), FOOTER_MAGIC.len()) != FOOTER_MAGIC {
        return LZMA_FORMAT_ERROR;
    }

    let crc = crc32::crc32(core::slice::from_raw_parts(input.add(4), 6), 0);
    if crc != read32le(input) {
        return LZMA_DATA_ERROR;
    }

    if stream_flags_decode(&mut *options, input.add(8)) {
        return LZMA_OPTIONS_ERROR;
    }

    (*options).backward_size = (u64::from(read32le(input.add(4))) + 1) * 4;
    LZMA_OK
}

pub(crate) unsafe fn stream_flags_compare_impl(
    a: *const lzma_stream_flags,
    b: *const lzma_stream_flags,
) -> lzma_ret {
    if a.is_null() || b.is_null() {
        return LZMA_PROG_ERROR;
    }

    let a = &*a;
    let b = &*b;
    if a.version != 0 || b.version != 0 {
        return LZMA_OPTIONS_ERROR;
    }

    if (a.check as u32) > LZMA_CHECK_ID_MAX as u32 || (b.check as u32) > LZMA_CHECK_ID_MAX as u32 {
        return LZMA_PROG_ERROR;
    }

    if a.check != b.check {
        return LZMA_DATA_ERROR;
    }

    if a.backward_size != LZMA_VLI_UNKNOWN && b.backward_size != LZMA_VLI_UNKNOWN {
        if !is_backward_size_valid(a) || !is_backward_size_valid(b) {
            return LZMA_PROG_ERROR;
        }

        if a.backward_size != b.backward_size {
            return LZMA_DATA_ERROR;
        }
    }

    LZMA_OK
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::internal::common::LZMA_CHECK_CRC32;

    #[test]
    fn header_round_trip_sets_unknown_backward_size() {
        let flags = lzma_stream_flags {
            version: 0,
            backward_size: 1234,
            check: LZMA_CHECK_CRC32,
            reserved_enum1: 0,
            reserved_enum2: 0,
            reserved_enum3: 0,
            reserved_enum4: 0,
            reserved_bool1: 0,
            reserved_bool2: 0,
            reserved_bool3: 0,
            reserved_bool4: 0,
            reserved_bool5: 0,
            reserved_bool6: 0,
            reserved_bool7: 0,
            reserved_bool8: 0,
            reserved_int1: 0,
            reserved_int2: 0,
        };
        let mut decoded = flags;
        let mut buf = [0u8; LZMA_STREAM_HEADER_SIZE];

        unsafe {
            assert_eq!(stream_header_encode_impl(&flags, buf.as_mut_ptr()), LZMA_OK);
            assert_eq!(
                stream_header_decode_impl(&mut decoded, buf.as_ptr()),
                LZMA_OK
            );
        }

        assert_eq!(decoded.version, 0);
        assert_eq!(decoded.backward_size, LZMA_VLI_UNKNOWN);
        assert_eq!(decoded.check, LZMA_CHECK_CRC32);
    }

    #[test]
    fn footer_round_trip_preserves_backward_size() {
        let flags = lzma_stream_flags {
            version: 0,
            backward_size: LZMA_BACKWARD_SIZE_MIN,
            check: 4,
            reserved_enum1: 0,
            reserved_enum2: 0,
            reserved_enum3: 0,
            reserved_enum4: 0,
            reserved_bool1: 0,
            reserved_bool2: 0,
            reserved_bool3: 0,
            reserved_bool4: 0,
            reserved_bool5: 0,
            reserved_bool6: 0,
            reserved_bool7: 0,
            reserved_bool8: 0,
            reserved_int1: 0,
            reserved_int2: 0,
        };
        let mut decoded = flags;
        let mut buf = [0u8; LZMA_STREAM_HEADER_SIZE];

        unsafe {
            assert_eq!(stream_footer_encode_impl(&flags, buf.as_mut_ptr()), LZMA_OK);
            assert_eq!(
                stream_footer_decode_impl(&mut decoded, buf.as_ptr()),
                LZMA_OK
            );
            assert_eq!(stream_flags_compare_impl(&flags, &decoded), LZMA_OK);
        }

        assert_eq!(decoded.backward_size, flags.backward_size);
        assert_eq!(decoded.check, flags.check);
    }
}
