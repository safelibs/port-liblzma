// Generated from the upstream public prototypes in lzma.h and the Linux symbol map.
#![allow(non_snake_case)]

use core::ffi::{c_char, c_int};

use super::types::*;
use crate::internal::{check, filter, hardware, index, preset, stream_flags, stream_state, vli};

#[no_mangle]
#[allow(unused_variables)]
pub unsafe extern "C" fn lzma_alone_decoder(arg0: *mut lzma_stream, arg1: u64) -> lzma_ret {
    LZMA_OPTIONS_ERROR
}

#[no_mangle]
#[allow(unused_variables)]
pub unsafe extern "C" fn lzma_alone_encoder(
    arg0: *mut lzma_stream,
    arg1: *const lzma_options_lzma,
) -> lzma_ret {
    LZMA_OPTIONS_ERROR
}

#[no_mangle]
#[allow(unused_variables)]
pub unsafe extern "C" fn lzma_auto_decoder(
    arg0: *mut lzma_stream,
    arg1: u64,
    arg2: u32,
) -> lzma_ret {
    LZMA_OPTIONS_ERROR
}

#[no_mangle]
#[allow(unused_variables)]
pub unsafe extern "C" fn lzma_block_buffer_bound(arg0: usize) -> usize {
    0
}

#[no_mangle]
#[allow(unused_variables)]
pub unsafe extern "C" fn lzma_block_buffer_decode(
    arg0: *mut lzma_block,
    arg1: *const lzma_allocator,
    arg2: *const u8,
    arg3: *mut usize,
    arg4: usize,
    arg5: *mut u8,
    arg6: *mut usize,
    arg7: usize,
) -> lzma_ret {
    LZMA_OPTIONS_ERROR
}

#[no_mangle]
#[allow(unused_variables)]
pub unsafe extern "C" fn lzma_block_buffer_encode(
    arg0: *mut lzma_block,
    arg1: *const lzma_allocator,
    arg2: *const u8,
    arg3: usize,
    arg4: *mut u8,
    arg5: *mut usize,
    arg6: usize,
) -> lzma_ret {
    LZMA_OPTIONS_ERROR
}

#[no_mangle]
#[allow(unused_variables)]
pub unsafe extern "C" fn lzma_block_compressed_size(
    arg0: *mut lzma_block,
    arg1: lzma_vli,
) -> lzma_ret {
    LZMA_OPTIONS_ERROR
}

#[no_mangle]
#[allow(unused_variables)]
pub unsafe extern "C" fn lzma_block_decoder(
    arg0: *mut lzma_stream,
    arg1: *mut lzma_block,
) -> lzma_ret {
    LZMA_OPTIONS_ERROR
}

#[no_mangle]
#[allow(unused_variables)]
pub unsafe extern "C" fn lzma_block_encoder(
    arg0: *mut lzma_stream,
    arg1: *mut lzma_block,
) -> lzma_ret {
    LZMA_OPTIONS_ERROR
}

#[no_mangle]
#[allow(unused_variables)]
pub unsafe extern "C" fn lzma_block_header_decode(
    arg0: *mut lzma_block,
    arg1: *const lzma_allocator,
    arg2: *const u8,
) -> lzma_ret {
    LZMA_OPTIONS_ERROR
}

#[no_mangle]
#[allow(unused_variables)]
pub unsafe extern "C" fn lzma_block_header_encode(
    arg0: *const lzma_block,
    arg1: *mut u8,
) -> lzma_ret {
    LZMA_OPTIONS_ERROR
}

#[no_mangle]
#[allow(unused_variables)]
pub unsafe extern "C" fn lzma_block_header_size(arg0: *mut lzma_block) -> lzma_ret {
    LZMA_OPTIONS_ERROR
}

#[no_mangle]
#[allow(unused_variables)]
pub unsafe extern "C" fn lzma_block_total_size(arg0: *const lzma_block) -> lzma_vli {
    0
}

#[no_mangle]
#[allow(unused_variables)]
pub unsafe extern "C" fn lzma_block_unpadded_size(arg0: *const lzma_block) -> lzma_vli {
    0
}

#[no_mangle]
#[allow(unused_variables)]
pub unsafe extern "C" fn lzma_check_is_supported(arg0: lzma_check) -> lzma_bool {
    check::check_is_supported(arg0)
}

#[no_mangle]
#[allow(unused_variables)]
pub unsafe extern "C" fn lzma_check_size(arg0: lzma_check) -> u32 {
    check::check_size(arg0)
}

#[no_mangle]
#[allow(unused_variables)]
pub unsafe extern "C" fn lzma_code(arg0: *mut lzma_stream, arg1: lzma_action) -> lzma_ret {
    stream_state::lzma_code_impl(arg0, arg1)
}

#[no_mangle]
#[allow(unused_variables)]
pub unsafe extern "C" fn lzma_crc32(arg0: *const u8, arg1: usize, arg2: u32) -> u32 {
    if arg1 == 0 {
        return check::crc32::crc32(&[], arg2);
    }
    if arg0.is_null() {
        return arg2;
    }
    check::crc32::crc32(core::slice::from_raw_parts(arg0, arg1), arg2)
}

#[no_mangle]
#[allow(unused_variables)]
pub unsafe extern "C" fn lzma_crc64(arg0: *const u8, arg1: usize, arg2: u64) -> u64 {
    if arg1 == 0 {
        return check::crc64::crc64(&[], arg2);
    }
    if arg0.is_null() {
        return arg2;
    }
    check::crc64::crc64(core::slice::from_raw_parts(arg0, arg1), arg2)
}

#[no_mangle]
#[allow(unused_variables)]
pub unsafe extern "C" fn lzma_easy_buffer_encode(
    arg0: u32,
    arg1: lzma_check,
    arg2: *const lzma_allocator,
    arg3: *const u8,
    arg4: usize,
    arg5: *mut u8,
    arg6: *mut usize,
    arg7: usize,
) -> lzma_ret {
    LZMA_OPTIONS_ERROR
}

#[no_mangle]
#[allow(unused_variables)]
pub unsafe extern "C" fn lzma_easy_decoder_memusage(arg0: u32) -> u64 {
    u64::MAX
}

#[no_mangle]
#[allow(unused_variables)]
pub unsafe extern "C" fn lzma_easy_encoder(
    arg0: *mut lzma_stream,
    arg1: u32,
    arg2: lzma_check,
) -> lzma_ret {
    LZMA_OPTIONS_ERROR
}

#[no_mangle]
#[allow(unused_variables)]
pub unsafe extern "C" fn lzma_easy_encoder_memusage(arg0: u32) -> u64 {
    u64::MAX
}

#[no_mangle]
#[allow(unused_variables)]
pub unsafe extern "C" fn lzma_end(arg0: *mut lzma_stream) {
    stream_state::lzma_end_impl(arg0)
}

#[no_mangle]
#[allow(unused_variables)]
pub unsafe extern "C" fn lzma_filter_decoder_is_supported(arg0: lzma_vli) -> lzma_bool {
    filter::decoder_is_supported(arg0)
}

#[no_mangle]
#[allow(unused_variables)]
pub unsafe extern "C" fn lzma_filter_encoder_is_supported(arg0: lzma_vli) -> lzma_bool {
    filter::encoder_is_supported(arg0)
}

#[no_mangle]
#[allow(unused_variables)]
pub unsafe extern "C" fn lzma_filter_flags_decode(
    arg0: *mut lzma_filter,
    arg1: *const lzma_allocator,
    arg2: *const u8,
    arg3: *mut usize,
    arg4: usize,
) -> lzma_ret {
    filter::filter_flags_decode_impl(arg0, arg1, arg2, arg3, arg4)
}

#[no_mangle]
#[allow(unused_variables)]
pub unsafe extern "C" fn lzma_filter_flags_encode(
    arg0: *const lzma_filter,
    arg1: *mut u8,
    arg2: *mut usize,
    arg3: usize,
) -> lzma_ret {
    filter::filter_flags_encode_impl(arg0, arg1, arg2, arg3)
}

#[no_mangle]
#[allow(unused_variables)]
pub unsafe extern "C" fn lzma_filter_flags_size(
    arg0: *mut u32,
    arg1: *const lzma_filter,
) -> lzma_ret {
    filter::filter_flags_size_impl(arg0, arg1)
}

#[no_mangle]
#[allow(unused_variables)]
pub unsafe extern "C" fn lzma_filters_copy(
    arg0: *const lzma_filter,
    arg1: *mut lzma_filter,
    arg2: *const lzma_allocator,
) -> lzma_ret {
    filter::filters_copy_impl(arg0, arg1, arg2)
}

#[no_mangle]
#[allow(unused_variables)]
pub unsafe extern "C" fn lzma_filters_update(
    arg0: *mut lzma_stream,
    arg1: *const lzma_filter,
) -> lzma_ret {
    LZMA_OPTIONS_ERROR
}

#[no_mangle]
#[allow(unused_variables)]
pub unsafe extern "C" fn lzma_get_check(arg0: *const lzma_stream) -> lzma_check {
    stream_state::lzma_get_check_impl(arg0)
}

#[no_mangle]
#[allow(unused_variables)]
pub unsafe extern "C" fn lzma_index_append(
    arg0: *mut lzma_index,
    arg1: *const lzma_allocator,
    arg2: lzma_vli,
    arg3: lzma_vli,
) -> lzma_ret {
    index::index_append(arg0, arg1, arg2, arg3)
}

#[no_mangle]
#[allow(unused_variables)]
pub unsafe extern "C" fn lzma_index_block_count(arg0: *const lzma_index) -> lzma_vli {
    index::index_block_count(arg0)
}

#[no_mangle]
#[allow(unused_variables)]
pub unsafe extern "C" fn lzma_index_buffer_decode(
    arg0: *mut *mut lzma_index,
    arg1: *mut u64,
    arg2: *const lzma_allocator,
    arg3: *const u8,
    arg4: *mut usize,
    arg5: usize,
) -> lzma_ret {
    index::index_buffer_decode(arg0, arg1, arg2, arg3, arg4, arg5)
}

#[no_mangle]
#[allow(unused_variables)]
pub unsafe extern "C" fn lzma_index_buffer_encode(
    arg0: *const lzma_index,
    arg1: *mut u8,
    arg2: *mut usize,
    arg3: usize,
) -> lzma_ret {
    index::index_buffer_encode(arg0, arg1, arg2, arg3)
}

#[no_mangle]
#[allow(unused_variables)]
pub unsafe extern "C" fn lzma_index_cat(
    arg0: *mut lzma_index,
    arg1: *mut lzma_index,
    arg2: *const lzma_allocator,
) -> lzma_ret {
    index::index_cat(arg0, arg1, arg2)
}

#[no_mangle]
#[allow(unused_variables)]
pub unsafe extern "C" fn lzma_index_checks(arg0: *const lzma_index) -> u32 {
    index::index_checks(arg0)
}

#[no_mangle]
#[allow(unused_variables)]
pub unsafe extern "C" fn lzma_index_decoder(
    arg0: *mut lzma_stream,
    arg1: *mut *mut lzma_index,
    arg2: u64,
) -> lzma_ret {
    index::index_decoder(arg0, arg1, arg2)
}

#[no_mangle]
#[allow(unused_variables)]
pub unsafe extern "C" fn lzma_index_dup(
    arg0: *const lzma_index,
    arg1: *const lzma_allocator,
) -> *mut lzma_index {
    index::index_dup(arg0, arg1)
}

#[no_mangle]
#[allow(unused_variables)]
pub unsafe extern "C" fn lzma_index_encoder(
    arg0: *mut lzma_stream,
    arg1: *const lzma_index,
) -> lzma_ret {
    index::index_encoder(arg0, arg1)
}

#[no_mangle]
#[allow(unused_variables)]
pub unsafe extern "C" fn lzma_index_end(arg0: *mut lzma_index, arg1: *const lzma_allocator) {
    index::index_end(arg0, arg1)
}

#[no_mangle]
#[allow(unused_variables)]
pub unsafe extern "C" fn lzma_index_file_size(arg0: *const lzma_index) -> lzma_vli {
    index::index_file_size(arg0)
}

#[no_mangle]
#[allow(unused_variables)]
pub unsafe extern "C" fn lzma_index_hash_append(
    arg0: *mut lzma_index_hash,
    arg1: lzma_vli,
    arg2: lzma_vli,
) -> lzma_ret {
    index::index_hash_append(arg0, arg1, arg2)
}

#[no_mangle]
#[allow(unused_variables)]
pub unsafe extern "C" fn lzma_index_hash_decode(
    arg0: *mut lzma_index_hash,
    arg1: *const u8,
    arg2: *mut usize,
    arg3: usize,
) -> lzma_ret {
    index::index_hash_decode(arg0, arg1, arg2, arg3)
}

#[no_mangle]
#[allow(unused_variables)]
pub unsafe extern "C" fn lzma_index_hash_end(
    arg0: *mut lzma_index_hash,
    arg1: *const lzma_allocator,
) {
    index::index_hash_end(arg0, arg1)
}

#[no_mangle]
#[allow(unused_variables)]
pub unsafe extern "C" fn lzma_index_hash_init(
    arg0: *mut lzma_index_hash,
    arg1: *const lzma_allocator,
) -> *mut lzma_index_hash {
    index::index_hash_init(arg0, arg1)
}

#[no_mangle]
#[allow(unused_variables)]
pub unsafe extern "C" fn lzma_index_hash_size(arg0: *const lzma_index_hash) -> lzma_vli {
    index::index_hash_size(arg0)
}

#[no_mangle]
#[allow(unused_variables)]
pub unsafe extern "C" fn lzma_index_init(arg0: *const lzma_allocator) -> *mut lzma_index {
    index::index_init(arg0)
}

#[no_mangle]
#[allow(unused_variables)]
pub unsafe extern "C" fn lzma_index_iter_init(arg0: *mut lzma_index_iter, arg1: *const lzma_index) {
    index::index_iter_init(arg0, arg1)
}

#[no_mangle]
#[allow(unused_variables)]
pub unsafe extern "C" fn lzma_index_iter_locate(
    arg0: *mut lzma_index_iter,
    arg1: lzma_vli,
) -> lzma_bool {
    index::index_iter_locate(arg0, arg1)
}

#[no_mangle]
#[allow(unused_variables)]
pub unsafe extern "C" fn lzma_index_iter_next(
    arg0: *mut lzma_index_iter,
    arg1: lzma_index_iter_mode,
) -> lzma_bool {
    index::index_iter_next(arg0, arg1)
}

#[no_mangle]
#[allow(unused_variables)]
pub unsafe extern "C" fn lzma_index_iter_rewind(arg0: *mut lzma_index_iter) {
    index::index_iter_rewind(arg0)
}

#[no_mangle]
#[allow(unused_variables)]
pub unsafe extern "C" fn lzma_index_memusage(arg0: lzma_vli, arg1: lzma_vli) -> u64 {
    index::index_memusage(arg0, arg1)
}

#[no_mangle]
#[allow(unused_variables)]
pub unsafe extern "C" fn lzma_index_memused(arg0: *const lzma_index) -> u64 {
    index::index_memused(arg0)
}

#[no_mangle]
#[allow(unused_variables)]
pub unsafe extern "C" fn lzma_index_size(arg0: *const lzma_index) -> lzma_vli {
    index::index_size(arg0)
}

#[no_mangle]
#[allow(unused_variables)]
pub unsafe extern "C" fn lzma_index_stream_count(arg0: *const lzma_index) -> lzma_vli {
    index::index_stream_count(arg0)
}

#[no_mangle]
#[allow(unused_variables)]
pub unsafe extern "C" fn lzma_index_stream_flags(
    arg0: *mut lzma_index,
    arg1: *const lzma_stream_flags,
) -> lzma_ret {
    index::index_stream_flags(arg0, arg1)
}

#[no_mangle]
#[allow(unused_variables)]
pub unsafe extern "C" fn lzma_index_stream_padding(
    arg0: *mut lzma_index,
    arg1: lzma_vli,
) -> lzma_ret {
    index::index_stream_padding(arg0, arg1)
}

#[no_mangle]
#[allow(unused_variables)]
pub unsafe extern "C" fn lzma_index_stream_size(arg0: *const lzma_index) -> lzma_vli {
    index::index_stream_size(arg0)
}

#[no_mangle]
#[allow(unused_variables)]
pub unsafe extern "C" fn lzma_index_total_size(arg0: *const lzma_index) -> lzma_vli {
    index::index_total_size(arg0)
}

#[no_mangle]
#[allow(unused_variables)]
pub unsafe extern "C" fn lzma_index_uncompressed_size(arg0: *const lzma_index) -> lzma_vli {
    index::index_uncompressed_size(arg0)
}

#[no_mangle]
#[allow(unused_variables)]
pub unsafe extern "C" fn lzma_lzma_preset(arg0: *mut lzma_options_lzma, arg1: u32) -> lzma_bool {
    preset::lzma_lzma_preset_impl(arg0, arg1)
}

#[no_mangle]
#[allow(unused_variables)]
pub unsafe extern "C" fn lzma_memlimit_get(arg0: *const lzma_stream) -> u64 {
    stream_state::lzma_memlimit_get_impl(arg0)
}

#[no_mangle]
#[allow(unused_variables)]
pub unsafe extern "C" fn lzma_memlimit_set(arg0: *mut lzma_stream, arg1: u64) -> lzma_ret {
    stream_state::lzma_memlimit_set_impl(arg0, arg1)
}

#[no_mangle]
#[allow(unused_variables)]
pub unsafe extern "C" fn lzma_memusage(arg0: *const lzma_stream) -> u64 {
    stream_state::lzma_memusage_impl(arg0)
}

#[no_mangle]
#[allow(unused_variables)]
pub unsafe extern "C" fn lzma_mf_is_supported(arg0: lzma_match_finder) -> lzma_bool {
    preset::mf_is_supported(arg0)
}

#[no_mangle]
#[allow(unused_variables)]
pub unsafe extern "C" fn lzma_mode_is_supported(arg0: lzma_mode) -> lzma_bool {
    preset::mode_is_supported(arg0)
}

#[no_mangle]
#[allow(unused_variables)]
pub unsafe extern "C" fn lzma_physmem() -> u64 {
    hardware::physmem()
}

#[no_mangle]
#[allow(unused_variables)]
pub unsafe extern "C" fn lzma_properties_decode(
    arg0: *mut lzma_filter,
    arg1: *const lzma_allocator,
    arg2: *const u8,
    arg3: usize,
) -> lzma_ret {
    filter::properties_decode_impl(arg0, arg1, arg2, arg3)
}

#[no_mangle]
#[allow(unused_variables)]
pub unsafe extern "C" fn lzma_properties_encode(
    arg0: *const lzma_filter,
    arg1: *mut u8,
) -> lzma_ret {
    filter::properties_encode_impl(arg0, arg1)
}

#[no_mangle]
#[allow(unused_variables)]
pub unsafe extern "C" fn lzma_properties_size(
    arg0: *mut u32,
    arg1: *const lzma_filter,
) -> lzma_ret {
    filter::properties_size_impl(arg0, arg1)
}

#[no_mangle]
#[allow(unused_variables)]
pub unsafe extern "C" fn lzma_raw_buffer_decode(
    arg0: *const lzma_filter,
    arg1: *const lzma_allocator,
    arg2: *const u8,
    arg3: *mut usize,
    arg4: usize,
    arg5: *mut u8,
    arg6: *mut usize,
    arg7: usize,
) -> lzma_ret {
    LZMA_OPTIONS_ERROR
}

#[no_mangle]
#[allow(unused_variables)]
pub unsafe extern "C" fn lzma_raw_buffer_encode(
    arg0: *const lzma_filter,
    arg1: *const lzma_allocator,
    arg2: *const u8,
    arg3: usize,
    arg4: *mut u8,
    arg5: *mut usize,
    arg6: usize,
) -> lzma_ret {
    LZMA_OPTIONS_ERROR
}

#[no_mangle]
#[allow(unused_variables)]
pub unsafe extern "C" fn lzma_raw_decoder(
    arg0: *mut lzma_stream,
    arg1: *const lzma_filter,
) -> lzma_ret {
    LZMA_OPTIONS_ERROR
}

#[no_mangle]
#[allow(unused_variables)]
pub unsafe extern "C" fn lzma_raw_decoder_memusage(arg0: *const lzma_filter) -> u64 {
    u64::MAX
}

#[no_mangle]
#[allow(unused_variables)]
pub unsafe extern "C" fn lzma_raw_encoder(
    arg0: *mut lzma_stream,
    arg1: *const lzma_filter,
) -> lzma_ret {
    LZMA_OPTIONS_ERROR
}

#[no_mangle]
#[allow(unused_variables)]
pub unsafe extern "C" fn lzma_raw_encoder_memusage(arg0: *const lzma_filter) -> u64 {
    u64::MAX
}

#[no_mangle]
#[allow(unused_variables)]
pub unsafe extern "C" fn lzma_stream_buffer_bound(arg0: usize) -> usize {
    0
}

#[no_mangle]
#[allow(unused_variables)]
pub unsafe extern "C" fn lzma_stream_buffer_decode(
    arg0: *mut u64,
    arg1: u32,
    arg2: *const lzma_allocator,
    arg3: *const u8,
    arg4: *mut usize,
    arg5: usize,
    arg6: *mut u8,
    arg7: *mut usize,
    arg8: usize,
) -> lzma_ret {
    LZMA_OPTIONS_ERROR
}

#[no_mangle]
#[allow(unused_variables)]
pub unsafe extern "C" fn lzma_stream_buffer_encode(
    arg0: *mut lzma_filter,
    arg1: lzma_check,
    arg2: *const lzma_allocator,
    arg3: *const u8,
    arg4: usize,
    arg5: *mut u8,
    arg6: *mut usize,
    arg7: usize,
) -> lzma_ret {
    LZMA_OPTIONS_ERROR
}

#[no_mangle]
#[allow(unused_variables)]
pub unsafe extern "C" fn lzma_stream_decoder(
    arg0: *mut lzma_stream,
    arg1: u64,
    arg2: u32,
) -> lzma_ret {
    LZMA_OPTIONS_ERROR
}

#[no_mangle]
#[allow(unused_variables)]
pub unsafe extern "C" fn lzma_stream_encoder(
    arg0: *mut lzma_stream,
    arg1: *const lzma_filter,
    arg2: lzma_check,
) -> lzma_ret {
    LZMA_OPTIONS_ERROR
}

#[no_mangle]
#[allow(unused_variables)]
pub unsafe extern "C" fn lzma_stream_flags_compare(
    arg0: *const lzma_stream_flags,
    arg1: *const lzma_stream_flags,
) -> lzma_ret {
    stream_flags::stream_flags_compare_impl(arg0, arg1)
}

#[no_mangle]
#[allow(unused_variables)]
pub unsafe extern "C" fn lzma_stream_footer_decode(
    arg0: *mut lzma_stream_flags,
    arg1: *const u8,
) -> lzma_ret {
    stream_flags::stream_footer_decode_impl(arg0, arg1)
}

#[no_mangle]
#[allow(unused_variables)]
pub unsafe extern "C" fn lzma_stream_footer_encode(
    arg0: *const lzma_stream_flags,
    arg1: *mut u8,
) -> lzma_ret {
    stream_flags::stream_footer_encode_impl(arg0, arg1)
}

#[no_mangle]
#[allow(unused_variables)]
pub unsafe extern "C" fn lzma_stream_header_decode(
    arg0: *mut lzma_stream_flags,
    arg1: *const u8,
) -> lzma_ret {
    stream_flags::stream_header_decode_impl(arg0, arg1)
}

#[no_mangle]
#[allow(unused_variables)]
pub unsafe extern "C" fn lzma_stream_header_encode(
    arg0: *const lzma_stream_flags,
    arg1: *mut u8,
) -> lzma_ret {
    stream_flags::stream_header_encode_impl(arg0, arg1)
}

#[no_mangle]
#[allow(unused_variables)]
pub unsafe extern "C" fn lzma_version_number() -> u32 {
    LZMA_VERSION
}

#[no_mangle]
#[allow(unused_variables)]
pub unsafe extern "C" fn lzma_version_string() -> *const c_char {
    version_string_ptr()
}

#[no_mangle]
#[allow(unused_variables)]
pub unsafe extern "C" fn lzma_vli_decode(
    arg0: *mut lzma_vli,
    arg1: *mut usize,
    arg2: *const u8,
    arg3: *mut usize,
    arg4: usize,
) -> lzma_ret {
    vli::lzma_vli_decode_impl(arg0, arg1, arg2, arg3, arg4)
}

#[no_mangle]
#[allow(unused_variables)]
pub unsafe extern "C" fn lzma_vli_encode(
    arg0: lzma_vli,
    arg1: *mut usize,
    arg2: *mut u8,
    arg3: *mut usize,
    arg4: usize,
) -> lzma_ret {
    vli::lzma_vli_encode_impl(arg0, arg1, arg2, arg3, arg4)
}

#[no_mangle]
#[allow(unused_variables)]
pub unsafe extern "C" fn lzma_vli_size(arg0: lzma_vli) -> u32 {
    vli::lzma_vli_size_impl(arg0)
}

#[no_mangle]
#[allow(unused_variables)]
pub unsafe extern "C" fn lzma_block_uncomp_encode(
    arg0: *mut lzma_block,
    arg1: *const u8,
    arg2: usize,
    arg3: *mut u8,
    arg4: *mut usize,
    arg5: usize,
) -> lzma_ret {
    LZMA_OPTIONS_ERROR
}

#[no_mangle]
#[allow(unused_variables)]
pub unsafe extern "C" fn lzma_cputhreads() -> u32 {
    hardware::cputhreads()
}

#[no_mangle]
#[allow(unused_variables)]
pub unsafe extern "C" fn lzma_get_progress(arg0: *mut lzma_stream, arg1: *mut u64, arg2: *mut u64) {
    stream_state::lzma_get_progress_impl(arg0, arg1, arg2)
}

#[no_mangle]
#[allow(unused_variables)]
pub unsafe extern "C" fn lzma_stream_encoder_mt(
    arg0: *mut lzma_stream,
    arg1: *const lzma_mt,
) -> lzma_ret {
    LZMA_OPTIONS_ERROR
}

#[no_mangle]
#[allow(unused_variables)]
pub unsafe extern "C" fn lzma_stream_encoder_mt_memusage(arg0: *const lzma_mt) -> u64 {
    u64::MAX
}

#[no_mangle]
#[allow(unused_variables)]
pub unsafe extern "C" fn lzma_file_info_decoder(
    arg0: *mut lzma_stream,
    arg1: *mut *mut lzma_index,
    arg2: u64,
    arg3: u64,
) -> lzma_ret {
    index::file_info_decoder(arg0, arg1, arg2, arg3)
}

#[no_mangle]
#[allow(unused_variables)]
pub unsafe extern "C" fn lzma_filters_free(arg0: *mut lzma_filter, arg1: *const lzma_allocator) {
    filter::filters_free_impl(arg0, arg1)
}

#[no_mangle]
#[allow(unused_variables)]
pub unsafe extern "C" fn lzma_lzip_decoder(
    arg0: *mut lzma_stream,
    arg1: u64,
    arg2: u32,
) -> lzma_ret {
    LZMA_OPTIONS_ERROR
}

#[no_mangle]
#[allow(unused_variables)]
pub unsafe extern "C" fn lzma_microlzma_decoder(
    arg0: *mut lzma_stream,
    arg1: u64,
    arg2: u64,
    arg3: lzma_bool,
    arg4: u32,
) -> lzma_ret {
    LZMA_OPTIONS_ERROR
}

#[no_mangle]
#[allow(unused_variables)]
pub unsafe extern "C" fn lzma_microlzma_encoder(
    arg0: *mut lzma_stream,
    arg1: *const lzma_options_lzma,
) -> lzma_ret {
    LZMA_OPTIONS_ERROR
}

#[no_mangle]
#[allow(unused_variables)]
pub unsafe extern "C" fn lzma_stream_decoder_mt(
    arg0: *mut lzma_stream,
    arg1: *const lzma_mt,
) -> lzma_ret {
    LZMA_OPTIONS_ERROR
}

#[no_mangle]
#[allow(unused_variables)]
pub unsafe extern "C" fn lzma_str_from_filters(
    arg0: *mut *mut c_char,
    arg1: *const lzma_filter,
    arg2: u32,
    arg3: *const lzma_allocator,
) -> lzma_ret {
    filter::str_from_filters_impl(arg0, arg1, arg2, arg3)
}

#[no_mangle]
#[allow(unused_variables)]
pub unsafe extern "C" fn lzma_str_list_filters(
    arg0: *mut *mut c_char,
    arg1: lzma_vli,
    arg2: u32,
    arg3: *const lzma_allocator,
) -> lzma_ret {
    filter::str_list_filters_impl(arg0, arg1, arg2, arg3)
}

#[no_mangle]
#[allow(unused_variables)]
pub unsafe extern "C" fn lzma_str_to_filters(
    arg0: *const c_char,
    arg1: *mut c_int,
    arg2: *mut lzma_filter,
    arg3: u32,
    arg4: *const lzma_allocator,
) -> *const c_char {
    filter::str_to_filters_impl(arg0, arg1, arg2, arg3, arg4)
}
