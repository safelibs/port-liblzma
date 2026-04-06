use crate::ffi::types::{lzma_options_delta, lzma_ret, LZMA_OPTIONS_ERROR, LZMA_PROG_ERROR};
use crate::internal::common::{LZMA_DELTA_DIST_MAX, LZMA_DELTA_DIST_MIN};
use crate::internal::filter::properties::LZMA_DELTA_TYPE_BYTE;

pub(crate) unsafe fn validate_options(options: *const lzma_options_delta) -> lzma_ret {
    if options.is_null() {
        return LZMA_PROG_ERROR;
    }

    if (*options).r#type != LZMA_DELTA_TYPE_BYTE
        || (*options).dist < LZMA_DELTA_DIST_MIN
        || (*options).dist > LZMA_DELTA_DIST_MAX
    {
        return LZMA_OPTIONS_ERROR;
    }

    0
}

pub(crate) unsafe fn distance_from_options(
    options: *const lzma_options_delta,
) -> Result<usize, lzma_ret> {
    let ret = validate_options(options);
    if ret != 0 {
        return Err(ret);
    }

    Ok((*options).dist as usize)
}
