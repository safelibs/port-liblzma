use core::{ffi::c_void, ptr};

use crate::ffi::types::{
    lzma_action, lzma_allocator, lzma_check, lzma_ret, lzma_stream, LZMA_DATA_ERROR,
    LZMA_GET_CHECK, LZMA_MEMLIMIT_ERROR, LZMA_NO_CHECK, LZMA_OK, LZMA_OPTIONS_ERROR,
    LZMA_PROG_ERROR, LZMA_STREAM_END, LZMA_STREAM_INIT,
};
use crate::internal::{
    common::{ACTION_COUNT, LZMA_FINISH, LZMA_RUN},
    lzma::LZMA_MEMUSAGE_BASE,
    stream_state::{
        install_next_coder, lzma_code_impl, lzma_end_impl, lzma_get_check_impl,
        lzma_memlimit_get_impl, lzma_memlimit_set_impl, lzma_memusage_impl, NextCoder,
    },
};

use super::{
    alone, lzip,
    stream::{
        self, LZMA_CONCATENATED, LZMA_IGNORE_CHECK, LZMA_TELL_ANY_CHECK, LZMA_TELL_NO_CHECK,
        STREAM_DECODER_SUPPORTED_FLAGS,
    },
};

#[derive(Copy, Clone, Eq, PartialEq)]
enum AutoSequence {
    Init,
    Code,
    Finish,
}

struct AutoDecoder {
    inner: lzma_stream,
    memlimit: u64,
    flags: u32,
    sequence: AutoSequence,
}

const fn auto_decoder_actions() -> [bool; ACTION_COUNT] {
    let mut actions = [false; ACTION_COUNT];
    actions[LZMA_RUN as usize] = true;
    actions[LZMA_FINISH as usize] = true;
    actions
}

unsafe fn forward_to_inner(
    coder: &mut AutoDecoder,
    input: *const u8,
    in_pos: *mut usize,
    in_size: usize,
    output: *mut u8,
    out_pos: *mut usize,
    out_size: usize,
    action: lzma_action,
) -> lzma_ret {
    let in_remaining = in_size - *in_pos;
    let out_remaining = out_size - *out_pos;

    coder.inner.next_in = if in_remaining == 0 {
        ptr::null()
    } else {
        input.add(*in_pos)
    };
    coder.inner.avail_in = in_remaining;
    coder.inner.next_out = if out_remaining == 0 {
        ptr::null_mut()
    } else {
        output.add(*out_pos)
    };
    coder.inner.avail_out = out_remaining;

    let ret = lzma_code_impl(&mut coder.inner, action);
    *in_pos += in_remaining - coder.inner.avail_in;
    *out_pos += out_remaining - coder.inner.avail_out;
    ret
}

unsafe fn auto_decoder_code(
    coder: *mut c_void,
    allocator: *const lzma_allocator,
    input: *const u8,
    in_pos: *mut usize,
    in_size: usize,
    output: *mut u8,
    out_pos: *mut usize,
    out_size: usize,
    action: lzma_action,
) -> lzma_ret {
    let coder = &mut *coder.cast::<AutoDecoder>();
    match coder.sequence {
        AutoSequence::Init => {
            if *in_pos >= in_size {
                return LZMA_OK;
            }

            coder.sequence = AutoSequence::Code;
            coder.inner.allocator = allocator;

            let ret = match *input.add(*in_pos) {
                0xFD => stream::stream_decoder(&mut coder.inner, coder.memlimit, coder.flags),
                0x4C => {
                    let lzip_flags =
                        coder.flags & (LZMA_TELL_ANY_CHECK | LZMA_IGNORE_CHECK | LZMA_CONCATENATED);
                    lzip::lzip_decoder(&mut coder.inner, coder.memlimit, lzip_flags)
                }
                _ => alone::alone_decoder(&mut coder.inner, coder.memlimit),
            };
            if ret != LZMA_OK {
                return ret;
            }

            if (*input.add(*in_pos) != 0xFD) && (*input.add(*in_pos) != 0x4C) {
                if (coder.flags & LZMA_TELL_NO_CHECK) != 0 {
                    return LZMA_NO_CHECK;
                }
                if (coder.flags & LZMA_TELL_ANY_CHECK) != 0 {
                    return LZMA_GET_CHECK;
                }
            }
        }
        AutoSequence::Code => {}
        AutoSequence::Finish => {
            if *in_pos < in_size {
                return LZMA_DATA_ERROR;
            }

            return if action == LZMA_FINISH {
                LZMA_STREAM_END
            } else {
                LZMA_OK
            };
        }
    }

    let ret = forward_to_inner(
        coder, input, in_pos, in_size, output, out_pos, out_size, action,
    );
    if ret != LZMA_STREAM_END || (coder.flags & LZMA_CONCATENATED) == 0 {
        return ret;
    }

    coder.sequence = AutoSequence::Finish;
    if *in_pos < in_size {
        LZMA_DATA_ERROR
    } else if action == LZMA_FINISH {
        LZMA_STREAM_END
    } else {
        LZMA_OK
    }
}

unsafe fn auto_decoder_end(coder: *mut c_void, _allocator: *const lzma_allocator) {
    let mut coder = Box::from_raw(coder.cast::<AutoDecoder>());
    lzma_end_impl(&mut coder.inner);
}

unsafe fn auto_decoder_get_check(coder: *const c_void) -> lzma_check {
    let coder = &*coder.cast::<AutoDecoder>();
    if coder.inner.internal.is_null() {
        crate::ffi::types::LZMA_CHECK_NONE
    } else {
        lzma_get_check_impl(&coder.inner)
    }
}

unsafe fn auto_decoder_memconfig(
    coder: *mut c_void,
    memusage: *mut u64,
    old_memlimit: *mut u64,
    new_memlimit: u64,
) -> lzma_ret {
    let coder = &mut *coder.cast::<AutoDecoder>();
    if coder.inner.internal.is_null() {
        *memusage = LZMA_MEMUSAGE_BASE;
        *old_memlimit = coder.memlimit;

        if new_memlimit != 0 {
            let new_memlimit = new_memlimit.max(1);
            if new_memlimit < *memusage {
                return LZMA_MEMLIMIT_ERROR;
            }
            coder.memlimit = new_memlimit;
        }

        return LZMA_OK;
    }

    *memusage = lzma_memusage_impl(&coder.inner).max(LZMA_MEMUSAGE_BASE);
    *old_memlimit = lzma_memlimit_get_impl(&coder.inner).max(1);
    if new_memlimit != 0 {
        let new_memlimit = new_memlimit.max(1);
        let ret = lzma_memlimit_set_impl(&mut coder.inner, new_memlimit);
        if ret == LZMA_OK {
            coder.memlimit = new_memlimit;
        }
        return ret;
    }

    LZMA_OK
}

pub(crate) unsafe fn auto_decoder(strm: *mut lzma_stream, memlimit: u64, flags: u32) -> lzma_ret {
    if strm.is_null() {
        return LZMA_PROG_ERROR;
    }
    if (flags & !STREAM_DECODER_SUPPORTED_FLAGS) != 0 {
        return LZMA_OPTIONS_ERROR;
    }

    let mut inner = LZMA_STREAM_INIT;
    inner.allocator = (*strm).allocator;

    install_next_coder(
        strm,
        NextCoder {
            coder: Box::into_raw(Box::new(AutoDecoder {
                inner,
                memlimit: memlimit.max(1),
                flags,
                sequence: AutoSequence::Init,
            }))
            .cast(),
            code: auto_decoder_code,
            end: Some(auto_decoder_end),
            get_progress: None,
            get_check: Some(auto_decoder_get_check),
            memconfig: Some(auto_decoder_memconfig),
            update: None,
        },
        auto_decoder_actions(),
    )
}
