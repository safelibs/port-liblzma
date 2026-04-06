use core::{mem, ptr};

use crate::ffi::types::{
    lzma_allocator, lzma_check, lzma_filter, lzma_options_lzma, lzma_ret, lzma_stream,
    LZMA_OPTIONS_ERROR, LZMA_VLI_UNKNOWN,
};
use crate::internal::{filter::common::LZMA_FILTER_LZMA2, lzma, preset};

use super::{stream, stream_buffer};

fn easy_filters(
    options: &mut lzma_options_lzma,
) -> [lzma_filter; crate::ffi::types::LZMA_FILTERS_MAX + 1] {
    [
        lzma_filter {
            id: LZMA_FILTER_LZMA2,
            options: (options as *mut lzma_options_lzma).cast(),
        },
        lzma_filter {
            id: LZMA_VLI_UNKNOWN,
            options: ptr::null_mut(),
        },
        lzma_filter {
            id: LZMA_VLI_UNKNOWN,
            options: ptr::null_mut(),
        },
        lzma_filter {
            id: LZMA_VLI_UNKNOWN,
            options: ptr::null_mut(),
        },
        lzma_filter {
            id: LZMA_VLI_UNKNOWN,
            options: ptr::null_mut(),
        },
    ]
}

pub(crate) unsafe fn easy_encoder(
    strm: *mut lzma_stream,
    preset_id: u32,
    check: lzma_check,
) -> lzma_ret {
    let mut options: lzma_options_lzma = mem::zeroed();
    if preset::lzma_lzma_preset_impl(&mut options, preset_id) != 0 {
        return LZMA_OPTIONS_ERROR;
    }

    let filters = easy_filters(&mut options);
    stream::stream_encoder(strm, filters.as_ptr(), check)
}

pub(crate) unsafe fn easy_buffer_encode(
    preset_id: u32,
    check: lzma_check,
    allocator: *const lzma_allocator,
    input: *const u8,
    input_size: usize,
    output: *mut u8,
    output_pos: *mut usize,
    output_size: usize,
) -> lzma_ret {
    let mut options: lzma_options_lzma = mem::zeroed();
    if preset::lzma_lzma_preset_impl(&mut options, preset_id) != 0 {
        return LZMA_OPTIONS_ERROR;
    }

    let mut filters = easy_filters(&mut options);
    stream_buffer::stream_buffer_encode(
        filters.as_mut_ptr(),
        check,
        allocator,
        input,
        input_size,
        output,
        output_pos,
        output_size,
    )
}

pub(crate) unsafe fn easy_encoder_memusage(preset_id: u32) -> u64 {
    let mut options: lzma_options_lzma = mem::zeroed();
    if preset::lzma_lzma_preset_impl(&mut options, preset_id) != 0 {
        return u64::from(u32::MAX);
    }

    let filters = easy_filters(&mut options);
    lzma::encoder_memusage(filters.as_ptr())
}

pub(crate) unsafe fn easy_decoder_memusage(preset_id: u32) -> u64 {
    let mut options: lzma_options_lzma = mem::zeroed();
    if preset::lzma_lzma_preset_impl(&mut options, preset_id) != 0 {
        return u64::from(u32::MAX);
    }

    let filters = easy_filters(&mut options);
    lzma::decoder_memusage(filters.as_ptr())
}
