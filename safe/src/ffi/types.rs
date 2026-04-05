#![allow(non_camel_case_types)]

use core::ffi::{c_char, c_int, c_void};

pub type lzma_bool = u8;
pub type lzma_reserved_enum = c_int;
pub type lzma_ret = c_int;
pub type lzma_action = c_int;
pub type lzma_check = c_int;
pub type lzma_delta_type = c_int;
pub type lzma_match_finder = c_int;
pub type lzma_mode = c_int;
pub type lzma_index_iter_mode = c_int;
pub type lzma_vli = u64;

pub const LZMA_OK: lzma_ret = 0;
pub const LZMA_STREAM_END: lzma_ret = 1;
pub const LZMA_NO_CHECK: lzma_ret = 2;
pub const LZMA_UNSUPPORTED_CHECK: lzma_ret = 3;
pub const LZMA_GET_CHECK: lzma_ret = 4;
pub const LZMA_MEM_ERROR: lzma_ret = 5;
pub const LZMA_MEMLIMIT_ERROR: lzma_ret = 6;
pub const LZMA_FORMAT_ERROR: lzma_ret = 7;
pub const LZMA_OPTIONS_ERROR: lzma_ret = 8;
pub const LZMA_DATA_ERROR: lzma_ret = 9;
pub const LZMA_BUF_ERROR: lzma_ret = 10;
pub const LZMA_PROG_ERROR: lzma_ret = 11;
pub const LZMA_SEEK_NEEDED: lzma_ret = 12;

pub const LZMA_CHECK_NONE: lzma_check = 0;
pub const LZMA_RESERVED_ENUM: lzma_reserved_enum = 0;

pub const LZMA_FILTERS_MAX: usize = 4;
pub const LZMA_CHECK_SIZE_MAX: usize = 64;
pub const LZMA_VLI_UNKNOWN: lzma_vli = u64::MAX;
pub const LZMA_VERSION: u32 = 50_040_052;
pub const LZMA_VERSION_STRING: &[u8; 6] = b"5.4.5\0";

#[repr(C)]
#[derive(Copy, Clone)]
pub struct lzma_allocator {
    pub alloc: Option<unsafe extern "C" fn(*mut c_void, usize, usize) -> *mut c_void>,
    pub free: Option<unsafe extern "C" fn(*mut c_void, *mut c_void)>,
    pub opaque: *mut c_void,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct lzma_internal {
    _private: [u8; 0],
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct lzma_index {
    _private: [u8; 0],
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct lzma_index_hash {
    _private: [u8; 0],
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct lzma_stream {
    pub next_in: *const u8,
    pub avail_in: usize,
    pub total_in: u64,
    pub next_out: *mut u8,
    pub avail_out: usize,
    pub total_out: u64,
    pub allocator: *const lzma_allocator,
    pub internal: *mut lzma_internal,
    pub reserved_ptr1: *mut c_void,
    pub reserved_ptr2: *mut c_void,
    pub reserved_ptr3: *mut c_void,
    pub reserved_ptr4: *mut c_void,
    pub seek_pos: u64,
    pub reserved_int2: u64,
    pub reserved_int3: usize,
    pub reserved_int4: usize,
    pub reserved_enum1: lzma_reserved_enum,
    pub reserved_enum2: lzma_reserved_enum,
}

pub const LZMA_STREAM_INIT: lzma_stream = lzma_stream {
    next_in: core::ptr::null(),
    avail_in: 0,
    total_in: 0,
    next_out: core::ptr::null_mut(),
    avail_out: 0,
    total_out: 0,
    allocator: core::ptr::null(),
    internal: core::ptr::null_mut(),
    reserved_ptr1: core::ptr::null_mut(),
    reserved_ptr2: core::ptr::null_mut(),
    reserved_ptr3: core::ptr::null_mut(),
    reserved_ptr4: core::ptr::null_mut(),
    seek_pos: 0,
    reserved_int2: 0,
    reserved_int3: 0,
    reserved_int4: 0,
    reserved_enum1: LZMA_RESERVED_ENUM,
    reserved_enum2: LZMA_RESERVED_ENUM,
};

#[repr(C)]
#[derive(Copy, Clone)]
pub struct lzma_filter {
    pub id: lzma_vli,
    pub options: *mut c_void,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct lzma_mt {
    pub flags: u32,
    pub threads: u32,
    pub block_size: u64,
    pub timeout: u32,
    pub preset: u32,
    pub filters: *const lzma_filter,
    pub check: lzma_check,
    pub reserved_enum1: lzma_reserved_enum,
    pub reserved_enum2: lzma_reserved_enum,
    pub reserved_enum3: lzma_reserved_enum,
    pub reserved_int1: u32,
    pub reserved_int2: u32,
    pub reserved_int3: u32,
    pub reserved_int4: u32,
    pub memlimit_threading: u64,
    pub memlimit_stop: u64,
    pub reserved_int7: u64,
    pub reserved_int8: u64,
    pub reserved_ptr1: *mut c_void,
    pub reserved_ptr2: *mut c_void,
    pub reserved_ptr3: *mut c_void,
    pub reserved_ptr4: *mut c_void,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct lzma_options_bcj {
    pub start_offset: u32,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct lzma_options_delta {
    pub r#type: lzma_delta_type,
    pub dist: u32,
    pub reserved_int1: u32,
    pub reserved_int2: u32,
    pub reserved_int3: u32,
    pub reserved_int4: u32,
    pub reserved_ptr1: *mut c_void,
    pub reserved_ptr2: *mut c_void,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct lzma_options_lzma {
    pub dict_size: u32,
    pub preset_dict: *const u8,
    pub preset_dict_size: u32,
    pub lc: u32,
    pub lp: u32,
    pub pb: u32,
    pub mode: lzma_mode,
    pub nice_len: u32,
    pub mf: lzma_match_finder,
    pub depth: u32,
    pub ext_flags: u32,
    pub ext_size_low: u32,
    pub ext_size_high: u32,
    pub reserved_int4: u32,
    pub reserved_int5: u32,
    pub reserved_int6: u32,
    pub reserved_int7: u32,
    pub reserved_int8: u32,
    pub reserved_enum1: lzma_reserved_enum,
    pub reserved_enum2: lzma_reserved_enum,
    pub reserved_enum3: lzma_reserved_enum,
    pub reserved_enum4: lzma_reserved_enum,
    pub reserved_ptr1: *mut c_void,
    pub reserved_ptr2: *mut c_void,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct lzma_stream_flags {
    pub version: u32,
    pub backward_size: lzma_vli,
    pub check: lzma_check,
    pub reserved_enum1: lzma_reserved_enum,
    pub reserved_enum2: lzma_reserved_enum,
    pub reserved_enum3: lzma_reserved_enum,
    pub reserved_enum4: lzma_reserved_enum,
    pub reserved_bool1: lzma_bool,
    pub reserved_bool2: lzma_bool,
    pub reserved_bool3: lzma_bool,
    pub reserved_bool4: lzma_bool,
    pub reserved_bool5: lzma_bool,
    pub reserved_bool6: lzma_bool,
    pub reserved_bool7: lzma_bool,
    pub reserved_bool8: lzma_bool,
    pub reserved_int1: u32,
    pub reserved_int2: u32,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct lzma_block {
    pub version: u32,
    pub header_size: u32,
    pub check: lzma_check,
    pub compressed_size: lzma_vli,
    pub uncompressed_size: lzma_vli,
    pub filters: *mut lzma_filter,
    pub raw_check: [u8; LZMA_CHECK_SIZE_MAX],
    pub reserved_ptr1: *mut c_void,
    pub reserved_ptr2: *mut c_void,
    pub reserved_ptr3: *mut c_void,
    pub reserved_int1: u32,
    pub reserved_int2: u32,
    pub reserved_int3: lzma_vli,
    pub reserved_int4: lzma_vli,
    pub reserved_int5: lzma_vli,
    pub reserved_int6: lzma_vli,
    pub reserved_int7: lzma_vli,
    pub reserved_int8: lzma_vli,
    pub reserved_enum1: lzma_reserved_enum,
    pub reserved_enum2: lzma_reserved_enum,
    pub reserved_enum3: lzma_reserved_enum,
    pub reserved_enum4: lzma_reserved_enum,
    pub ignore_check: lzma_bool,
    pub reserved_bool2: lzma_bool,
    pub reserved_bool3: lzma_bool,
    pub reserved_bool4: lzma_bool,
    pub reserved_bool5: lzma_bool,
    pub reserved_bool6: lzma_bool,
    pub reserved_bool7: lzma_bool,
    pub reserved_bool8: lzma_bool,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct lzma_index_iter_stream {
    pub flags: *const lzma_stream_flags,
    pub reserved_ptr1: *const c_void,
    pub reserved_ptr2: *const c_void,
    pub reserved_ptr3: *const c_void,
    pub number: lzma_vli,
    pub block_count: lzma_vli,
    pub compressed_offset: lzma_vli,
    pub uncompressed_offset: lzma_vli,
    pub compressed_size: lzma_vli,
    pub uncompressed_size: lzma_vli,
    pub padding: lzma_vli,
    pub reserved_vli1: lzma_vli,
    pub reserved_vli2: lzma_vli,
    pub reserved_vli3: lzma_vli,
    pub reserved_vli4: lzma_vli,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct lzma_index_iter_block {
    pub number_in_file: lzma_vli,
    pub compressed_file_offset: lzma_vli,
    pub uncompressed_file_offset: lzma_vli,
    pub number_in_stream: lzma_vli,
    pub compressed_stream_offset: lzma_vli,
    pub uncompressed_stream_offset: lzma_vli,
    pub uncompressed_size: lzma_vli,
    pub unpadded_size: lzma_vli,
    pub total_size: lzma_vli,
    pub reserved_vli1: lzma_vli,
    pub reserved_vli2: lzma_vli,
    pub reserved_vli3: lzma_vli,
    pub reserved_vli4: lzma_vli,
    pub reserved_ptr1: *const c_void,
    pub reserved_ptr2: *const c_void,
    pub reserved_ptr3: *const c_void,
    pub reserved_ptr4: *const c_void,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub union lzma_index_iter_internal {
    pub p: *const c_void,
    pub s: usize,
    pub v: lzma_vli,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct lzma_index_iter {
    pub stream: lzma_index_iter_stream,
    pub block: lzma_index_iter_block,
    pub internal: [lzma_index_iter_internal; 6],
}

pub fn version_string_ptr() -> *const c_char {
    LZMA_VERSION_STRING.as_ptr().cast()
}
