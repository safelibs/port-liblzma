use crate::ffi::types::{
    lzma_ret, lzma_vli, LZMA_BUF_ERROR, LZMA_DATA_ERROR, LZMA_OK, LZMA_PROG_ERROR,
    LZMA_STREAM_END,
};
use crate::internal::common::{LZMA_VLI_BYTES_MAX, LZMA_VLI_MAX};

pub(crate) unsafe fn lzma_vli_encode_impl(
    vli: lzma_vli,
    vli_pos: *mut usize,
    out: *mut u8,
    out_pos: *mut usize,
    out_size: usize,
) -> lzma_ret {
    if out_pos.is_null() || out.is_null() {
        return LZMA_PROG_ERROR;
    }

    let mut vli_pos_internal = 0usize;
    let single_call = vli_pos.is_null();
    let vli_pos = if single_call {
        &mut vli_pos_internal
    } else {
        &mut *vli_pos
    };

    if *out_pos >= out_size {
        return if single_call {
            LZMA_PROG_ERROR
        } else {
            LZMA_BUF_ERROR
        };
    }

    if *vli_pos >= LZMA_VLI_BYTES_MAX || vli > LZMA_VLI_MAX {
        return LZMA_PROG_ERROR;
    }

    let mut value = vli >> (*vli_pos * 7);

    while value >= 0x80 {
        *vli_pos += 1;
        debug_assert!(*vli_pos < LZMA_VLI_BYTES_MAX);

        *out.add(*out_pos) = (value as u8) | 0x80;
        value >>= 7;
        *out_pos += 1;

        if *out_pos == out_size {
            return if single_call { LZMA_PROG_ERROR } else { LZMA_OK };
        }
    }

    *out.add(*out_pos) = value as u8;
    *out_pos += 1;
    *vli_pos += 1;

    if single_call {
        LZMA_OK
    } else {
        LZMA_STREAM_END
    }
}

pub(crate) unsafe fn lzma_vli_decode_impl(
    vli: *mut lzma_vli,
    vli_pos: *mut usize,
    input: *const u8,
    in_pos: *mut usize,
    in_size: usize,
) -> lzma_ret {
    if vli.is_null() || in_pos.is_null() || input.is_null() {
        return LZMA_PROG_ERROR;
    }

    let mut vli_pos_internal = 0usize;
    let single_call = vli_pos.is_null();
    let vli_pos = if single_call {
        &mut vli_pos_internal
    } else {
        &mut *vli_pos
    };

    if single_call {
        *vli = 0;
        if *in_pos >= in_size {
            return LZMA_DATA_ERROR;
        }
    } else {
        if *vli_pos == 0 {
            *vli = 0;
        }

        if *vli_pos >= LZMA_VLI_BYTES_MAX || (*vli >> (*vli_pos * 7)) != 0 {
            return LZMA_PROG_ERROR;
        }

        if *in_pos >= in_size {
            return LZMA_BUF_ERROR;
        }
    }

    while *in_pos < in_size {
        let byte = *input.add(*in_pos);
        *in_pos += 1;

        *vli += ((byte & 0x7F) as lzma_vli) << (*vli_pos * 7);
        *vli_pos += 1;

        if (byte & 0x80) == 0 {
            if byte == 0 && *vli_pos > 1 {
                return LZMA_DATA_ERROR;
            }

            return if single_call { LZMA_OK } else { LZMA_STREAM_END };
        }

        if *vli_pos == LZMA_VLI_BYTES_MAX {
            return LZMA_DATA_ERROR;
        }
    }

    if single_call {
        LZMA_DATA_ERROR
    } else {
        LZMA_OK
    }
}

pub(crate) const fn lzma_vli_size_impl(vli: lzma_vli) -> u32 {
    if vli > LZMA_VLI_MAX {
        return 0;
    }

    let mut value = vli;
    let mut count = 0u32;
    loop {
        value >>= 7;
        count += 1;
        if value == 0 {
            break;
        }
    }

    count
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ffi::types::LZMA_VLI_UNKNOWN;

    #[test]
    fn vli_size_handles_boundaries() {
        assert_eq!(lzma_vli_size_impl(0), 1);
        assert_eq!(lzma_vli_size_impl(127), 1);
        assert_eq!(lzma_vli_size_impl(128), 2);
        assert_eq!(lzma_vli_size_impl(LZMA_VLI_MAX), 9);
        assert_eq!(lzma_vli_size_impl(LZMA_VLI_UNKNOWN), 0);
    }

    #[test]
    fn vli_encode_matches_upstream_vectors() {
        let mut out = [0u8; 9];
        let mut out_pos = 0usize;

        unsafe {
            assert_eq!(
                lzma_vli_encode_impl(526_617, core::ptr::null_mut(), out.as_mut_ptr(), &mut out_pos, 3),
                LZMA_OK
            );
        }

        assert_eq!(out_pos, 3);
        assert_eq!(&out[..3], &[0x99, 0x92, 0x20]);
    }

    #[test]
    fn vli_encode_multi_call_tracks_position() {
        let mut out = [0u8; 9];
        let mut out_pos = 0usize;
        let mut vli_pos = 0usize;

        unsafe {
            assert_eq!(
                lzma_vli_encode_impl(
                    136_100_349_976_529_025,
                    &mut vli_pos,
                    out.as_mut_ptr(),
                    &mut out_pos,
                    8
                ),
                LZMA_OK
            );
            assert_eq!(out_pos, 8);
            assert_eq!(vli_pos, 8);
            assert_eq!(
                lzma_vli_encode_impl(
                    136_100_349_976_529_025,
                    &mut vli_pos,
                    out.as_mut_ptr(),
                    &mut out_pos,
                    9
                ),
                LZMA_STREAM_END
            );
        }

        assert_eq!(&out, &[0x81, 0x91, 0xA1, 0xB1, 0xC1, 0xD1, 0xE1, 0xF1, 0x01]);
    }

    #[test]
    fn vli_decode_handles_multicall_and_invalid_padding() {
        let bytes = [0x80u8, 0x56];
        let mut in_pos = 0usize;
        let mut vli_pos = 0usize;
        let mut out = 0u64;

        unsafe {
            assert_eq!(
                lzma_vli_decode_impl(&mut out, &mut vli_pos, bytes.as_ptr(), &mut in_pos, 1),
                LZMA_OK
            );
            assert_eq!(in_pos, 1);
            assert_eq!(vli_pos, 1);
            assert_eq!(
                lzma_vli_decode_impl(&mut out, &mut vli_pos, bytes.as_ptr(), &mut in_pos, 2),
                LZMA_STREAM_END
            );
        }

        assert_eq!(out, 11_008);

        let invalid = [0x80u8, 0x00];
        let mut invalid_out = 0u64;
        let mut invalid_pos = 0usize;
        unsafe {
            assert_eq!(
                lzma_vli_decode_impl(
                    &mut invalid_out,
                    core::ptr::null_mut(),
                    invalid.as_ptr(),
                    &mut invalid_pos,
                    invalid.len()
                ),
                LZMA_DATA_ERROR
            );
        }
    }
}
