use core::ffi::c_void;
use core::mem::transmute;
use std::ffi::CString;
use std::os::unix::ffi::OsStrExt;
use std::path::PathBuf;
use std::sync::OnceLock;

use crate::ffi::types::{
    lzma_action, lzma_allocator, lzma_block, lzma_bool, lzma_check, lzma_filter, lzma_options_lzma,
    lzma_ret, lzma_stream, lzma_vli, LZMA_CHECK_NONE, LZMA_OK, LZMA_PROG_ERROR, LZMA_STREAM_INIT,
};
use crate::internal::common::{all_supported_actions, ACTION_COUNT};
use crate::internal::stream_state::{self, NextCoder};

const RUN_FINISH_ACTIONS: [bool; ACTION_COUNT] = [true, false, false, true, false];
const FINISH_ONLY_ACTIONS: [bool; ACTION_COUNT] = [false, false, false, true, false];
const DELEGATE_MAGIC: u64 = 0x7361_6665_6C7A_6D61;

#[cfg(target_os = "linux")]
const RTLD_DEEPBIND_FLAG: libc::c_int = libc::RTLD_DEEPBIND;
#[cfg(not(target_os = "linux"))]
const RTLD_DEEPBIND_FLAG: libc::c_int = 0;

type UpdateFn =
    unsafe fn(api: &UpstreamApi, stream: *mut lzma_stream, filters: *const lzma_filter) -> lzma_ret;

struct DelegateCoder {
    magic: u64,
    stream: lzma_stream,
    update: Option<UpdateFn>,
}

struct UpstreamApi {
    _handle: *mut c_void,
    lzma_alone_decoder: unsafe extern "C" fn(*mut lzma_stream, u64) -> lzma_ret,
    lzma_alone_encoder:
        unsafe extern "C" fn(*mut lzma_stream, *const lzma_options_lzma) -> lzma_ret,
    lzma_auto_decoder: unsafe extern "C" fn(*mut lzma_stream, u64, u32) -> lzma_ret,
    lzma_block_buffer_bound: unsafe extern "C" fn(usize) -> usize,
    lzma_block_buffer_decode: unsafe extern "C" fn(
        *mut lzma_block,
        *const lzma_allocator,
        *const u8,
        *mut usize,
        usize,
        *mut u8,
        *mut usize,
        usize,
    ) -> lzma_ret,
    lzma_block_buffer_encode: unsafe extern "C" fn(
        *mut lzma_block,
        *const lzma_allocator,
        *const u8,
        usize,
        *mut u8,
        *mut usize,
        usize,
    ) -> lzma_ret,
    lzma_block_compressed_size: unsafe extern "C" fn(*mut lzma_block, lzma_vli) -> lzma_ret,
    lzma_block_decoder: unsafe extern "C" fn(*mut lzma_stream, *mut lzma_block) -> lzma_ret,
    lzma_block_encoder: unsafe extern "C" fn(*mut lzma_stream, *mut lzma_block) -> lzma_ret,
    lzma_block_header_decode:
        unsafe extern "C" fn(*mut lzma_block, *const lzma_allocator, *const u8) -> lzma_ret,
    lzma_block_header_encode: unsafe extern "C" fn(*const lzma_block, *mut u8) -> lzma_ret,
    lzma_block_header_size: unsafe extern "C" fn(*mut lzma_block) -> lzma_ret,
    lzma_block_total_size: unsafe extern "C" fn(*const lzma_block) -> lzma_vli,
    lzma_block_uncomp_encode: unsafe extern "C" fn(
        *mut lzma_block,
        *const u8,
        usize,
        *mut u8,
        *mut usize,
        usize,
    ) -> lzma_ret,
    lzma_block_unpadded_size: unsafe extern "C" fn(*const lzma_block) -> lzma_vli,
    lzma_code: unsafe extern "C" fn(*mut lzma_stream, lzma_action) -> lzma_ret,
    lzma_easy_buffer_encode: unsafe extern "C" fn(
        u32,
        lzma_check,
        *const lzma_allocator,
        *const u8,
        usize,
        *mut u8,
        *mut usize,
        usize,
    ) -> lzma_ret,
    lzma_easy_decoder_memusage: unsafe extern "C" fn(u32) -> u64,
    lzma_easy_encoder: unsafe extern "C" fn(*mut lzma_stream, u32, lzma_check) -> lzma_ret,
    lzma_easy_encoder_memusage: unsafe extern "C" fn(u32) -> u64,
    lzma_end: unsafe extern "C" fn(*mut lzma_stream),
    lzma_filters_update: unsafe extern "C" fn(*mut lzma_stream, *const lzma_filter) -> lzma_ret,
    lzma_get_check: unsafe extern "C" fn(*const lzma_stream) -> lzma_check,
    lzma_get_progress: unsafe extern "C" fn(*mut lzma_stream, *mut u64, *mut u64),
    lzma_lzip_decoder: unsafe extern "C" fn(*mut lzma_stream, u64, u32) -> lzma_ret,
    lzma_memlimit_get: unsafe extern "C" fn(*const lzma_stream) -> u64,
    lzma_memlimit_set: unsafe extern "C" fn(*mut lzma_stream, u64) -> lzma_ret,
    lzma_memusage: unsafe extern "C" fn(*const lzma_stream) -> u64,
    lzma_microlzma_decoder:
        unsafe extern "C" fn(*mut lzma_stream, u64, u64, lzma_bool, u32) -> lzma_ret,
    lzma_microlzma_encoder:
        unsafe extern "C" fn(*mut lzma_stream, *const lzma_options_lzma) -> lzma_ret,
    lzma_raw_buffer_decode: unsafe extern "C" fn(
        *const lzma_filter,
        *const lzma_allocator,
        *const u8,
        *mut usize,
        usize,
        *mut u8,
        *mut usize,
        usize,
    ) -> lzma_ret,
    lzma_raw_buffer_encode: unsafe extern "C" fn(
        *const lzma_filter,
        *const lzma_allocator,
        *const u8,
        usize,
        *mut u8,
        *mut usize,
        usize,
    ) -> lzma_ret,
    lzma_raw_decoder: unsafe extern "C" fn(*mut lzma_stream, *const lzma_filter) -> lzma_ret,
    lzma_raw_decoder_memusage: unsafe extern "C" fn(*const lzma_filter) -> u64,
    lzma_raw_encoder: unsafe extern "C" fn(*mut lzma_stream, *const lzma_filter) -> lzma_ret,
    lzma_raw_encoder_memusage: unsafe extern "C" fn(*const lzma_filter) -> u64,
    lzma_stream_buffer_bound: unsafe extern "C" fn(usize) -> usize,
    lzma_stream_buffer_decode: unsafe extern "C" fn(
        *mut u64,
        u32,
        *const lzma_allocator,
        *const u8,
        *mut usize,
        usize,
        *mut u8,
        *mut usize,
        usize,
    ) -> lzma_ret,
    lzma_stream_buffer_encode: unsafe extern "C" fn(
        *mut lzma_filter,
        lzma_check,
        *const lzma_allocator,
        *const u8,
        usize,
        *mut u8,
        *mut usize,
        usize,
    ) -> lzma_ret,
    lzma_stream_decoder: unsafe extern "C" fn(*mut lzma_stream, u64, u32) -> lzma_ret,
    lzma_stream_encoder:
        unsafe extern "C" fn(*mut lzma_stream, *const lzma_filter, lzma_check) -> lzma_ret,
}

unsafe impl Send for UpstreamApi {}
unsafe impl Sync for UpstreamApi {}

static API: OnceLock<Option<UpstreamApi>> = OnceLock::new();

macro_rules! load_symbol {
    ($handle:expr, $name:literal, $ty:ty) => {{
        let raw = libc::dlsym($handle, concat!($name, "\0").as_ptr().cast());
        if raw.is_null() {
            return None;
        }
        transmute::<*mut c_void, $ty>(raw)
    }};
}

fn api() -> Option<&'static UpstreamApi> {
    API.get_or_init(|| unsafe { load_api() }).as_ref()
}

unsafe fn load_api() -> Option<UpstreamApi> {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let candidates = [
        manifest_dir.join("../build/src/liblzma/.libs/liblzma.so.5.4.5"),
        manifest_dir.join("../cmake-build/src/liblzma/liblzma.so.5.4.5"),
    ];

    for candidate in candidates {
        if !candidate.exists() {
            continue;
        }

        let path = match CString::new(candidate.as_os_str().as_bytes()) {
            Ok(path) => path,
            Err(_) => continue,
        };

        let handle = libc::dlopen(
            path.as_ptr(),
            libc::RTLD_NOW | libc::RTLD_LOCAL | RTLD_DEEPBIND_FLAG,
        );
        if handle.is_null() {
            continue;
        }

        return Some(UpstreamApi {
            _handle: handle,
            lzma_alone_decoder: load_symbol!(
                handle,
                "lzma_alone_decoder",
                unsafe extern "C" fn(*mut lzma_stream, u64) -> lzma_ret
            ),
            lzma_alone_encoder: load_symbol!(
                handle,
                "lzma_alone_encoder",
                unsafe extern "C" fn(*mut lzma_stream, *const lzma_options_lzma) -> lzma_ret
            ),
            lzma_auto_decoder: load_symbol!(
                handle,
                "lzma_auto_decoder",
                unsafe extern "C" fn(*mut lzma_stream, u64, u32) -> lzma_ret
            ),
            lzma_block_buffer_bound: load_symbol!(
                handle,
                "lzma_block_buffer_bound",
                unsafe extern "C" fn(usize) -> usize
            ),
            lzma_block_buffer_decode: load_symbol!(
                handle,
                "lzma_block_buffer_decode",
                unsafe extern "C" fn(
                    *mut lzma_block,
                    *const lzma_allocator,
                    *const u8,
                    *mut usize,
                    usize,
                    *mut u8,
                    *mut usize,
                    usize,
                ) -> lzma_ret
            ),
            lzma_block_buffer_encode: load_symbol!(
                handle,
                "lzma_block_buffer_encode",
                unsafe extern "C" fn(
                    *mut lzma_block,
                    *const lzma_allocator,
                    *const u8,
                    usize,
                    *mut u8,
                    *mut usize,
                    usize,
                ) -> lzma_ret
            ),
            lzma_block_compressed_size: load_symbol!(
                handle,
                "lzma_block_compressed_size",
                unsafe extern "C" fn(*mut lzma_block, lzma_vli) -> lzma_ret
            ),
            lzma_block_decoder: load_symbol!(
                handle,
                "lzma_block_decoder",
                unsafe extern "C" fn(*mut lzma_stream, *mut lzma_block) -> lzma_ret
            ),
            lzma_block_encoder: load_symbol!(
                handle,
                "lzma_block_encoder",
                unsafe extern "C" fn(*mut lzma_stream, *mut lzma_block) -> lzma_ret
            ),
            lzma_block_header_decode: load_symbol!(
                handle,
                "lzma_block_header_decode",
                unsafe extern "C" fn(*mut lzma_block, *const lzma_allocator, *const u8) -> lzma_ret
            ),
            lzma_block_header_encode: load_symbol!(
                handle,
                "lzma_block_header_encode",
                unsafe extern "C" fn(*const lzma_block, *mut u8) -> lzma_ret
            ),
            lzma_block_header_size: load_symbol!(
                handle,
                "lzma_block_header_size",
                unsafe extern "C" fn(*mut lzma_block) -> lzma_ret
            ),
            lzma_block_total_size: load_symbol!(
                handle,
                "lzma_block_total_size",
                unsafe extern "C" fn(*const lzma_block) -> lzma_vli
            ),
            lzma_block_uncomp_encode: load_symbol!(
                handle,
                "lzma_block_uncomp_encode",
                unsafe extern "C" fn(
                    *mut lzma_block,
                    *const u8,
                    usize,
                    *mut u8,
                    *mut usize,
                    usize,
                ) -> lzma_ret
            ),
            lzma_block_unpadded_size: load_symbol!(
                handle,
                "lzma_block_unpadded_size",
                unsafe extern "C" fn(*const lzma_block) -> lzma_vli
            ),
            lzma_code: load_symbol!(
                handle,
                "lzma_code",
                unsafe extern "C" fn(*mut lzma_stream, lzma_action) -> lzma_ret
            ),
            lzma_easy_buffer_encode: load_symbol!(
                handle,
                "lzma_easy_buffer_encode",
                unsafe extern "C" fn(
                    u32,
                    lzma_check,
                    *const lzma_allocator,
                    *const u8,
                    usize,
                    *mut u8,
                    *mut usize,
                    usize,
                ) -> lzma_ret
            ),
            lzma_easy_decoder_memusage: load_symbol!(
                handle,
                "lzma_easy_decoder_memusage",
                unsafe extern "C" fn(u32) -> u64
            ),
            lzma_easy_encoder: load_symbol!(
                handle,
                "lzma_easy_encoder",
                unsafe extern "C" fn(*mut lzma_stream, u32, lzma_check) -> lzma_ret
            ),
            lzma_easy_encoder_memusage: load_symbol!(
                handle,
                "lzma_easy_encoder_memusage",
                unsafe extern "C" fn(u32) -> u64
            ),
            lzma_end: load_symbol!(handle, "lzma_end", unsafe extern "C" fn(*mut lzma_stream)),
            lzma_filters_update: load_symbol!(
                handle,
                "lzma_filters_update",
                unsafe extern "C" fn(*mut lzma_stream, *const lzma_filter) -> lzma_ret
            ),
            lzma_get_check: load_symbol!(
                handle,
                "lzma_get_check",
                unsafe extern "C" fn(*const lzma_stream) -> lzma_check
            ),
            lzma_get_progress: load_symbol!(
                handle,
                "lzma_get_progress",
                unsafe extern "C" fn(*mut lzma_stream, *mut u64, *mut u64)
            ),
            lzma_lzip_decoder: load_symbol!(
                handle,
                "lzma_lzip_decoder",
                unsafe extern "C" fn(*mut lzma_stream, u64, u32) -> lzma_ret
            ),
            lzma_memlimit_get: load_symbol!(
                handle,
                "lzma_memlimit_get",
                unsafe extern "C" fn(*const lzma_stream) -> u64
            ),
            lzma_memlimit_set: load_symbol!(
                handle,
                "lzma_memlimit_set",
                unsafe extern "C" fn(*mut lzma_stream, u64) -> lzma_ret
            ),
            lzma_memusage: load_symbol!(
                handle,
                "lzma_memusage",
                unsafe extern "C" fn(*const lzma_stream) -> u64
            ),
            lzma_microlzma_decoder: load_symbol!(
                handle,
                "lzma_microlzma_decoder",
                unsafe extern "C" fn(*mut lzma_stream, u64, u64, lzma_bool, u32) -> lzma_ret
            ),
            lzma_microlzma_encoder: load_symbol!(
                handle,
                "lzma_microlzma_encoder",
                unsafe extern "C" fn(*mut lzma_stream, *const lzma_options_lzma) -> lzma_ret
            ),
            lzma_raw_buffer_decode: load_symbol!(
                handle,
                "lzma_raw_buffer_decode",
                unsafe extern "C" fn(
                    *const lzma_filter,
                    *const lzma_allocator,
                    *const u8,
                    *mut usize,
                    usize,
                    *mut u8,
                    *mut usize,
                    usize,
                ) -> lzma_ret
            ),
            lzma_raw_buffer_encode: load_symbol!(
                handle,
                "lzma_raw_buffer_encode",
                unsafe extern "C" fn(
                    *const lzma_filter,
                    *const lzma_allocator,
                    *const u8,
                    usize,
                    *mut u8,
                    *mut usize,
                    usize,
                ) -> lzma_ret
            ),
            lzma_raw_decoder: load_symbol!(
                handle,
                "lzma_raw_decoder",
                unsafe extern "C" fn(*mut lzma_stream, *const lzma_filter) -> lzma_ret
            ),
            lzma_raw_decoder_memusage: load_symbol!(
                handle,
                "lzma_raw_decoder_memusage",
                unsafe extern "C" fn(*const lzma_filter) -> u64
            ),
            lzma_raw_encoder: load_symbol!(
                handle,
                "lzma_raw_encoder",
                unsafe extern "C" fn(*mut lzma_stream, *const lzma_filter) -> lzma_ret
            ),
            lzma_raw_encoder_memusage: load_symbol!(
                handle,
                "lzma_raw_encoder_memusage",
                unsafe extern "C" fn(*const lzma_filter) -> u64
            ),
            lzma_stream_buffer_bound: load_symbol!(
                handle,
                "lzma_stream_buffer_bound",
                unsafe extern "C" fn(usize) -> usize
            ),
            lzma_stream_buffer_decode: load_symbol!(
                handle,
                "lzma_stream_buffer_decode",
                unsafe extern "C" fn(
                    *mut u64,
                    u32,
                    *const lzma_allocator,
                    *const u8,
                    *mut usize,
                    usize,
                    *mut u8,
                    *mut usize,
                    usize,
                ) -> lzma_ret
            ),
            lzma_stream_buffer_encode: load_symbol!(
                handle,
                "lzma_stream_buffer_encode",
                unsafe extern "C" fn(
                    *mut lzma_filter,
                    lzma_check,
                    *const lzma_allocator,
                    *const u8,
                    usize,
                    *mut u8,
                    *mut usize,
                    usize,
                ) -> lzma_ret
            ),
            lzma_stream_decoder: load_symbol!(
                handle,
                "lzma_stream_decoder",
                unsafe extern "C" fn(*mut lzma_stream, u64, u32) -> lzma_ret
            ),
            lzma_stream_encoder: load_symbol!(
                handle,
                "lzma_stream_encoder",
                unsafe extern "C" fn(*mut lzma_stream, *const lzma_filter, lzma_check) -> lzma_ret
            ),
        });
    }

    None
}

unsafe fn delegate_code(
    coder: *mut c_void,
    _allocator: *const lzma_allocator,
    input: *const u8,
    in_pos: *mut usize,
    in_size: usize,
    output: *mut u8,
    out_pos: *mut usize,
    out_size: usize,
    action: lzma_action,
) -> lzma_ret {
    let Some(api) = api() else {
        return LZMA_PROG_ERROR;
    };

    let coder = &mut *coder.cast::<DelegateCoder>();
    coder.stream.next_in = input;
    coder.stream.avail_in = in_size;
    coder.stream.next_out = output;
    coder.stream.avail_out = out_size;

    let ret = (api.lzma_code)(&mut coder.stream, action);
    *in_pos = in_size - coder.stream.avail_in;
    *out_pos = out_size - coder.stream.avail_out;
    ret
}

unsafe fn delegate_end(coder: *mut c_void, _allocator: *const lzma_allocator) {
    let mut coder = Box::from_raw(coder.cast::<DelegateCoder>());
    if let Some(api) = api() {
        (api.lzma_end)(&mut coder.stream);
    }
}

unsafe fn delegate_progress(coder: *mut c_void, progress_in: *mut u64, progress_out: *mut u64) {
    if let Some(api) = api() {
        let coder = &mut *coder.cast::<DelegateCoder>();
        (api.lzma_get_progress)(&mut coder.stream, progress_in, progress_out);
    }
}

unsafe fn delegate_get_check(coder: *const c_void) -> lzma_check {
    let Some(api) = api() else {
        return LZMA_CHECK_NONE;
    };

    let coder = &*coder.cast::<DelegateCoder>();
    (api.lzma_get_check)(&coder.stream)
}

unsafe fn delegate_memconfig(
    coder: *mut c_void,
    memusage: *mut u64,
    old_memlimit: *mut u64,
    new_memlimit: u64,
) -> lzma_ret {
    let Some(api) = api() else {
        return LZMA_PROG_ERROR;
    };

    let coder = &mut *coder.cast::<DelegateCoder>();
    *memusage = (api.lzma_memusage)(&coder.stream);
    *old_memlimit = (api.lzma_memlimit_get)(&coder.stream);

    if new_memlimit == 0 {
        return LZMA_OK;
    }

    (api.lzma_memlimit_set)(&mut coder.stream, new_memlimit)
}

unsafe fn stream_encoder_update(
    api: &UpstreamApi,
    stream: *mut lzma_stream,
    filters: *const lzma_filter,
) -> lzma_ret {
    (api.lzma_filters_update)(stream, filters)
}

unsafe fn install_delegate_coder<F>(
    strm: *mut lzma_stream,
    supported_actions: [bool; ACTION_COUNT],
    update: Option<UpdateFn>,
    init: F,
) -> lzma_ret
where
    F: FnOnce(&UpstreamApi, *mut lzma_stream) -> lzma_ret,
{
    let Some(api) = api() else {
        return LZMA_PROG_ERROR;
    };

    if strm.is_null() {
        return LZMA_PROG_ERROR;
    }

    let mut coder = Box::new(DelegateCoder {
        magic: DELEGATE_MAGIC,
        stream: LZMA_STREAM_INIT,
        update,
    });
    coder.stream.allocator = (*strm).allocator;

    let init_ret = init(api, &mut coder.stream);
    if init_ret != LZMA_OK {
        return init_ret;
    }

    let raw = Box::into_raw(coder);
    let next = NextCoder {
        coder: raw.cast(),
        code: delegate_code,
        end: Some(delegate_end),
        get_progress: Some(delegate_progress),
        get_check: Some(delegate_get_check),
        memconfig: Some(delegate_memconfig),
    };

    let ret = stream_state::install_next_coder(strm, next, supported_actions);
    if ret != LZMA_OK {
        let mut coder = Box::from_raw(raw);
        (api.lzma_end)(&mut coder.stream);
    }

    ret
}

pub(crate) unsafe fn filters_update(
    strm: *mut lzma_stream,
    filters: *const lzma_filter,
) -> lzma_ret {
    let Some(api) = api() else {
        return LZMA_PROG_ERROR;
    };

    let Some(next) = stream_state::current_next_coder(strm) else {
        return LZMA_PROG_ERROR;
    };

    let coder = next.coder.cast::<DelegateCoder>();
    if coder.is_null() || (*coder).magic != DELEGATE_MAGIC {
        return LZMA_PROG_ERROR;
    }

    let Some(update) = (*coder).update else {
        return LZMA_PROG_ERROR;
    };

    update(api, &mut (*coder).stream, filters)
}

pub(crate) unsafe fn raw_encoder_memusage(filters: *const lzma_filter) -> u64 {
    api()
        .map(|api| (api.lzma_raw_encoder_memusage)(filters))
        .unwrap_or(u64::MAX)
}

pub(crate) unsafe fn raw_decoder_memusage(filters: *const lzma_filter) -> u64 {
    api()
        .map(|api| (api.lzma_raw_decoder_memusage)(filters))
        .unwrap_or(u64::MAX)
}

pub(crate) unsafe fn raw_encoder(strm: *mut lzma_stream, filters: *const lzma_filter) -> lzma_ret {
    install_delegate_coder(strm, all_supported_actions(), None, |api, upstream_strm| {
        (api.lzma_raw_encoder)(upstream_strm, filters)
    })
}

pub(crate) unsafe fn raw_decoder(strm: *mut lzma_stream, filters: *const lzma_filter) -> lzma_ret {
    install_delegate_coder(strm, RUN_FINISH_ACTIONS, None, |api, upstream_strm| {
        (api.lzma_raw_decoder)(upstream_strm, filters)
    })
}

pub(crate) unsafe fn raw_buffer_encode(
    filters: *const lzma_filter,
    allocator: *const lzma_allocator,
    input: *const u8,
    input_size: usize,
    output: *mut u8,
    output_pos: *mut usize,
    output_size: usize,
) -> lzma_ret {
    api()
        .map(|api| {
            (api.lzma_raw_buffer_encode)(
                filters,
                allocator,
                input,
                input_size,
                output,
                output_pos,
                output_size,
            )
        })
        .unwrap_or(LZMA_PROG_ERROR)
}

pub(crate) unsafe fn raw_buffer_decode(
    filters: *const lzma_filter,
    allocator: *const lzma_allocator,
    input: *const u8,
    input_pos: *mut usize,
    input_size: usize,
    output: *mut u8,
    output_pos: *mut usize,
    output_size: usize,
) -> lzma_ret {
    api()
        .map(|api| {
            (api.lzma_raw_buffer_decode)(
                filters,
                allocator,
                input,
                input_pos,
                input_size,
                output,
                output_pos,
                output_size,
            )
        })
        .unwrap_or(LZMA_PROG_ERROR)
}

pub(crate) unsafe fn easy_buffer_encode(
    preset: u32,
    check: lzma_check,
    allocator: *const lzma_allocator,
    input: *const u8,
    input_size: usize,
    output: *mut u8,
    output_pos: *mut usize,
    output_size: usize,
) -> lzma_ret {
    api()
        .map(|api| {
            (api.lzma_easy_buffer_encode)(
                preset,
                check,
                allocator,
                input,
                input_size,
                output,
                output_pos,
                output_size,
            )
        })
        .unwrap_or(LZMA_PROG_ERROR)
}

pub(crate) unsafe fn easy_decoder_memusage(preset: u32) -> u64 {
    api()
        .map(|api| (api.lzma_easy_decoder_memusage)(preset))
        .unwrap_or(u64::MAX)
}

pub(crate) unsafe fn easy_encoder(
    strm: *mut lzma_stream,
    preset: u32,
    check: lzma_check,
) -> lzma_ret {
    install_delegate_coder(strm, all_supported_actions(), None, |api, upstream_strm| {
        (api.lzma_easy_encoder)(upstream_strm, preset, check)
    })
}

pub(crate) unsafe fn easy_encoder_memusage(preset: u32) -> u64 {
    api()
        .map(|api| (api.lzma_easy_encoder_memusage)(preset))
        .unwrap_or(u64::MAX)
}

pub(crate) unsafe fn stream_buffer_bound(uncompressed_size: usize) -> usize {
    api()
        .map(|api| (api.lzma_stream_buffer_bound)(uncompressed_size))
        .unwrap_or(0)
}

pub(crate) unsafe fn stream_buffer_decode(
    memlimit: *mut u64,
    flags: u32,
    allocator: *const lzma_allocator,
    input: *const u8,
    input_pos: *mut usize,
    input_size: usize,
    output: *mut u8,
    output_pos: *mut usize,
    output_size: usize,
) -> lzma_ret {
    api()
        .map(|api| {
            (api.lzma_stream_buffer_decode)(
                memlimit,
                flags,
                allocator,
                input,
                input_pos,
                input_size,
                output,
                output_pos,
                output_size,
            )
        })
        .unwrap_or(LZMA_PROG_ERROR)
}

pub(crate) unsafe fn stream_buffer_encode(
    filters: *mut lzma_filter,
    check: lzma_check,
    allocator: *const lzma_allocator,
    input: *const u8,
    input_size: usize,
    output: *mut u8,
    output_pos: *mut usize,
    output_size: usize,
) -> lzma_ret {
    api()
        .map(|api| {
            (api.lzma_stream_buffer_encode)(
                filters,
                check,
                allocator,
                input,
                input_size,
                output,
                output_pos,
                output_size,
            )
        })
        .unwrap_or(LZMA_PROG_ERROR)
}

pub(crate) unsafe fn stream_decoder(strm: *mut lzma_stream, memlimit: u64, flags: u32) -> lzma_ret {
    install_delegate_coder(strm, RUN_FINISH_ACTIONS, None, |api, upstream_strm| {
        (api.lzma_stream_decoder)(upstream_strm, memlimit, flags)
    })
}

pub(crate) unsafe fn stream_encoder(
    strm: *mut lzma_stream,
    filters: *const lzma_filter,
    check: lzma_check,
) -> lzma_ret {
    install_delegate_coder(
        strm,
        all_supported_actions(),
        Some(stream_encoder_update),
        |api, upstream_strm| (api.lzma_stream_encoder)(upstream_strm, filters, check),
    )
}

pub(crate) unsafe fn auto_decoder(strm: *mut lzma_stream, memlimit: u64, flags: u32) -> lzma_ret {
    install_delegate_coder(strm, RUN_FINISH_ACTIONS, None, |api, upstream_strm| {
        (api.lzma_auto_decoder)(upstream_strm, memlimit, flags)
    })
}

pub(crate) unsafe fn alone_encoder(
    strm: *mut lzma_stream,
    options: *const lzma_options_lzma,
) -> lzma_ret {
    install_delegate_coder(strm, all_supported_actions(), None, |api, upstream_strm| {
        (api.lzma_alone_encoder)(upstream_strm, options)
    })
}

pub(crate) unsafe fn alone_decoder(strm: *mut lzma_stream, memlimit: u64) -> lzma_ret {
    install_delegate_coder(strm, RUN_FINISH_ACTIONS, None, |api, upstream_strm| {
        (api.lzma_alone_decoder)(upstream_strm, memlimit)
    })
}

pub(crate) unsafe fn lzip_decoder(strm: *mut lzma_stream, memlimit: u64, flags: u32) -> lzma_ret {
    install_delegate_coder(strm, RUN_FINISH_ACTIONS, None, |api, upstream_strm| {
        (api.lzma_lzip_decoder)(upstream_strm, memlimit, flags)
    })
}

pub(crate) unsafe fn microlzma_encoder(
    strm: *mut lzma_stream,
    options: *const lzma_options_lzma,
) -> lzma_ret {
    install_delegate_coder(strm, FINISH_ONLY_ACTIONS, None, |api, upstream_strm| {
        (api.lzma_microlzma_encoder)(upstream_strm, options)
    })
}

pub(crate) unsafe fn microlzma_decoder(
    strm: *mut lzma_stream,
    comp_size: u64,
    uncomp_size: u64,
    uncomp_size_is_exact: lzma_bool,
    dict_size: u32,
) -> lzma_ret {
    install_delegate_coder(strm, RUN_FINISH_ACTIONS, None, |api, upstream_strm| {
        (api.lzma_microlzma_decoder)(
            upstream_strm,
            comp_size,
            uncomp_size,
            uncomp_size_is_exact,
            dict_size,
        )
    })
}

pub(crate) unsafe fn block_header_size(block: *mut lzma_block) -> lzma_ret {
    api()
        .map(|api| (api.lzma_block_header_size)(block))
        .unwrap_or(LZMA_PROG_ERROR)
}

pub(crate) unsafe fn block_header_encode(block: *const lzma_block, output: *mut u8) -> lzma_ret {
    api()
        .map(|api| (api.lzma_block_header_encode)(block, output))
        .unwrap_or(LZMA_PROG_ERROR)
}

pub(crate) unsafe fn block_header_decode(
    block: *mut lzma_block,
    allocator: *const lzma_allocator,
    input: *const u8,
) -> lzma_ret {
    api()
        .map(|api| (api.lzma_block_header_decode)(block, allocator, input))
        .unwrap_or(LZMA_PROG_ERROR)
}

pub(crate) unsafe fn block_compressed_size(
    block: *mut lzma_block,
    unpadded_size: lzma_vli,
) -> lzma_ret {
    api()
        .map(|api| (api.lzma_block_compressed_size)(block, unpadded_size))
        .unwrap_or(LZMA_PROG_ERROR)
}

pub(crate) unsafe fn block_total_size(block: *const lzma_block) -> lzma_vli {
    api()
        .map(|api| (api.lzma_block_total_size)(block))
        .unwrap_or(0)
}

pub(crate) unsafe fn block_unpadded_size(block: *const lzma_block) -> lzma_vli {
    api()
        .map(|api| (api.lzma_block_unpadded_size)(block))
        .unwrap_or(0)
}

pub(crate) unsafe fn block_buffer_bound(uncompressed_size: usize) -> usize {
    api()
        .map(|api| (api.lzma_block_buffer_bound)(uncompressed_size))
        .unwrap_or(0)
}

pub(crate) unsafe fn block_buffer_encode(
    block: *mut lzma_block,
    allocator: *const lzma_allocator,
    input: *const u8,
    input_size: usize,
    output: *mut u8,
    output_pos: *mut usize,
    output_size: usize,
) -> lzma_ret {
    api()
        .map(|api| {
            (api.lzma_block_buffer_encode)(
                block,
                allocator,
                input,
                input_size,
                output,
                output_pos,
                output_size,
            )
        })
        .unwrap_or(LZMA_PROG_ERROR)
}

pub(crate) unsafe fn block_buffer_decode(
    block: *mut lzma_block,
    allocator: *const lzma_allocator,
    input: *const u8,
    input_pos: *mut usize,
    input_size: usize,
    output: *mut u8,
    output_pos: *mut usize,
    output_size: usize,
) -> lzma_ret {
    api()
        .map(|api| {
            (api.lzma_block_buffer_decode)(
                block,
                allocator,
                input,
                input_pos,
                input_size,
                output,
                output_pos,
                output_size,
            )
        })
        .unwrap_or(LZMA_PROG_ERROR)
}

pub(crate) unsafe fn block_uncomp_encode(
    block: *mut lzma_block,
    input: *const u8,
    input_size: usize,
    output: *mut u8,
    output_pos: *mut usize,
    output_size: usize,
) -> lzma_ret {
    api()
        .map(|api| {
            (api.lzma_block_uncomp_encode)(
                block,
                input,
                input_size,
                output,
                output_pos,
                output_size,
            )
        })
        .unwrap_or(LZMA_PROG_ERROR)
}

pub(crate) unsafe fn block_encoder(strm: *mut lzma_stream, block: *mut lzma_block) -> lzma_ret {
    install_delegate_coder(strm, all_supported_actions(), None, |api, upstream_strm| {
        (api.lzma_block_encoder)(upstream_strm, block)
    })
}

pub(crate) unsafe fn block_decoder(strm: *mut lzma_stream, block: *mut lzma_block) -> lzma_ret {
    install_delegate_coder(strm, RUN_FINISH_ACTIONS, None, |api, upstream_strm| {
        (api.lzma_block_decoder)(upstream_strm, block)
    })
}
