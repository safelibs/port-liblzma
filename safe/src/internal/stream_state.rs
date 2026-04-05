use core::ffi::c_void;
use core::mem;
use core::ptr;

use crate::ffi::types::{
    lzma_action, lzma_allocator, lzma_check, lzma_internal, lzma_ret, lzma_stream, LZMA_BUF_ERROR,
    LZMA_GET_CHECK, LZMA_MEMLIMIT_ERROR, LZMA_MEM_ERROR, LZMA_NO_CHECK, LZMA_OK, LZMA_PROG_ERROR,
    LZMA_SEEK_NEEDED, LZMA_STREAM_END, LZMA_UNSUPPORTED_CHECK,
};
use crate::internal::common::{
    action_index, default_supported_actions, lzma_alloc, lzma_free, reserved_members_are_clear,
    ACTION_COUNT, LZMA_FINISH, LZMA_FULL_BARRIER, LZMA_FULL_FLUSH, LZMA_RUN, LZMA_SYNC_FLUSH,
    LZMA_TIMED_OUT,
};

pub(crate) type CodeFn = unsafe fn(
    coder: *mut c_void,
    allocator: *const lzma_allocator,
    input: *const u8,
    in_pos: *mut usize,
    in_size: usize,
    output: *mut u8,
    out_pos: *mut usize,
    out_size: usize,
    action: lzma_action,
) -> lzma_ret;

pub(crate) type EndFn = unsafe fn(coder: *mut c_void, allocator: *const lzma_allocator);
pub(crate) type GetProgressFn =
    unsafe fn(coder: *mut c_void, progress_in: *mut u64, progress_out: *mut u64);
pub(crate) type GetCheckFn = unsafe fn(coder: *const c_void) -> lzma_check;
pub(crate) type MemConfigFn = unsafe fn(
    coder: *mut c_void,
    memusage: *mut u64,
    old_memlimit: *mut u64,
    new_memlimit: u64,
) -> lzma_ret;

#[derive(Copy, Clone)]
pub(crate) struct NextCoder {
    pub(crate) coder: *mut c_void,
    pub(crate) code: CodeFn,
    pub(crate) end: Option<EndFn>,
    pub(crate) get_progress: Option<GetProgressFn>,
    pub(crate) get_check: Option<GetCheckFn>,
    pub(crate) memconfig: Option<MemConfigFn>,
}

pub(crate) enum CoderInterface {
    Uninitialized,
    Registered(NextCoder),
}

#[derive(Copy, Clone, Eq, PartialEq)]
enum Sequence {
    Run,
    SyncFlush,
    FullFlush,
    Finish,
    FullBarrier,
    End,
    Error,
}

pub(crate) struct StreamState {
    next: CoderInterface,
    sequence: Sequence,
    avail_in: usize,
    supported_actions: [bool; ACTION_COUNT],
    allow_buf_error: bool,
}

impl StreamState {
    const fn new() -> Self {
        Self {
            next: CoderInterface::Uninitialized,
            sequence: Sequence::Run,
            avail_in: 0,
            supported_actions: default_supported_actions(),
            allow_buf_error: false,
        }
    }

    fn reset_runtime(&mut self) {
        self.sequence = Sequence::Run;
        self.avail_in = 0;
        self.supported_actions = default_supported_actions();
        self.allow_buf_error = false;
    }

    unsafe fn end_next(&mut self, allocator: *const lzma_allocator) {
        let current = mem::replace(&mut self.next, CoderInterface::Uninitialized);
        if let CoderInterface::Registered(next) = current {
            if let Some(end) = next.end {
                end(next.coder, allocator);
            } else if !next.coder.is_null() {
                lzma_free(next.coder, allocator);
            }
        }
    }
}

#[inline]
unsafe fn state_ptr(strm: *mut lzma_stream) -> *mut StreamState {
    (*strm).internal.cast::<StreamState>()
}

#[inline]
unsafe fn state_ref(strm: *const lzma_stream) -> *const StreamState {
    (*strm).internal.cast::<StreamState>()
}

pub(crate) unsafe fn current_next_coder(strm: *const lzma_stream) -> Option<NextCoder> {
    if strm.is_null() || (*strm).internal.is_null() {
        return None;
    }

    let state = &*state_ref(strm);
    match state.next {
        CoderInterface::Registered(next) => Some(next),
        CoderInterface::Uninitialized => None,
    }
}

pub(crate) unsafe fn lzma_strm_init(strm: *mut lzma_stream) -> lzma_ret {
    if strm.is_null() {
        return LZMA_PROG_ERROR;
    }

    if (*strm).internal.is_null() {
        let raw =
            lzma_alloc(mem::size_of::<StreamState>(), (*strm).allocator).cast::<StreamState>();
        if raw.is_null() {
            return LZMA_MEM_ERROR;
        }

        ptr::write(raw, StreamState::new());
        (*strm).internal = raw.cast::<lzma_internal>();
    }

    let state = &mut *state_ptr(strm);
    state.reset_runtime();
    (*strm).total_in = 0;
    (*strm).total_out = 0;

    LZMA_OK
}

pub(crate) unsafe fn install_next_coder(
    strm: *mut lzma_stream,
    next: NextCoder,
    supported_actions: [bool; ACTION_COUNT],
) -> lzma_ret {
    let ret = lzma_strm_init(strm);
    if ret != LZMA_OK {
        return ret;
    }

    let state = &mut *state_ptr(strm);
    state.end_next((*strm).allocator);
    state.next = CoderInterface::Registered(next);
    state.supported_actions = supported_actions;
    state.allow_buf_error = false;
    LZMA_OK
}

pub(crate) unsafe fn lzma_code_impl(strm: *mut lzma_stream, action: lzma_action) -> lzma_ret {
    if strm.is_null() {
        return LZMA_PROG_ERROR;
    }

    let Some(action_index) = action_index(action) else {
        return LZMA_PROG_ERROR;
    };

    if ((*strm).next_in.is_null() && (*strm).avail_in != 0)
        || ((*strm).next_out.is_null() && (*strm).avail_out != 0)
        || (*strm).internal.is_null()
    {
        return LZMA_PROG_ERROR;
    }

    let state = &mut *state_ptr(strm);
    let next = match state.next {
        CoderInterface::Registered(next) => next,
        CoderInterface::Uninitialized => return LZMA_PROG_ERROR,
    };

    if !reserved_members_are_clear(&*strm) {
        return crate::ffi::types::LZMA_OPTIONS_ERROR;
    }

    if !state.supported_actions[action_index] {
        return LZMA_PROG_ERROR;
    }

    match state.sequence {
        Sequence::Run => match action {
            LZMA_RUN => {}
            LZMA_SYNC_FLUSH => state.sequence = Sequence::SyncFlush,
            LZMA_FULL_FLUSH => state.sequence = Sequence::FullFlush,
            LZMA_FINISH => state.sequence = Sequence::Finish,
            LZMA_FULL_BARRIER => state.sequence = Sequence::FullBarrier,
            _ => unreachable!(),
        },
        Sequence::SyncFlush => {
            if action != LZMA_SYNC_FLUSH || state.avail_in != (*strm).avail_in {
                return LZMA_PROG_ERROR;
            }
        }
        Sequence::FullFlush => {
            if action != LZMA_FULL_FLUSH || state.avail_in != (*strm).avail_in {
                return LZMA_PROG_ERROR;
            }
        }
        Sequence::Finish => {
            if action != LZMA_FINISH || state.avail_in != (*strm).avail_in {
                return LZMA_PROG_ERROR;
            }
        }
        Sequence::FullBarrier => {
            if action != LZMA_FULL_BARRIER || state.avail_in != (*strm).avail_in {
                return LZMA_PROG_ERROR;
            }
        }
        Sequence::End => return LZMA_STREAM_END,
        Sequence::Error => return LZMA_PROG_ERROR,
    }

    let mut in_pos = 0usize;
    let mut out_pos = 0usize;
    let mut ret = (next.code)(
        next.coder,
        (*strm).allocator,
        (*strm).next_in,
        &mut in_pos,
        (*strm).avail_in,
        (*strm).next_out,
        &mut out_pos,
        (*strm).avail_out,
        action,
    );

    if in_pos > (*strm).avail_in || out_pos > (*strm).avail_out {
        state.sequence = Sequence::Error;
        return LZMA_PROG_ERROR;
    }

    if in_pos > 0 {
        (*strm).next_in = (*strm).next_in.add(in_pos);
        (*strm).avail_in -= in_pos;
        (*strm).total_in += in_pos as u64;
    }

    if out_pos > 0 {
        (*strm).next_out = (*strm).next_out.add(out_pos);
        (*strm).avail_out -= out_pos;
        (*strm).total_out += out_pos as u64;
    }

    state.avail_in = (*strm).avail_in;

    match ret {
        LZMA_OK => {
            if out_pos == 0 && in_pos == 0 {
                if state.allow_buf_error {
                    ret = LZMA_BUF_ERROR;
                } else {
                    state.allow_buf_error = true;
                }
            } else {
                state.allow_buf_error = false;
            }
        }
        LZMA_TIMED_OUT => {
            state.allow_buf_error = false;
            ret = LZMA_OK;
        }
        LZMA_SEEK_NEEDED => {
            state.allow_buf_error = false;
            if state.sequence == Sequence::Finish {
                state.sequence = Sequence::Run;
            }
        }
        LZMA_STREAM_END => {
            if matches!(
                state.sequence,
                Sequence::SyncFlush | Sequence::FullFlush | Sequence::FullBarrier
            ) {
                state.sequence = Sequence::Run;
            } else {
                state.sequence = Sequence::End;
            }

            state.allow_buf_error = false;
        }
        LZMA_NO_CHECK | LZMA_UNSUPPORTED_CHECK | LZMA_GET_CHECK | LZMA_MEMLIMIT_ERROR => {
            state.allow_buf_error = false;
        }
        _ => {
            debug_assert_ne!(ret, LZMA_BUF_ERROR);
            state.sequence = Sequence::Error;
        }
    }

    ret
}

pub(crate) unsafe fn lzma_end_impl(strm: *mut lzma_stream) {
    if strm.is_null() || (*strm).internal.is_null() {
        return;
    }

    let state = &mut *state_ptr(strm);
    state.end_next((*strm).allocator);
    ptr::drop_in_place(state);
    lzma_free((*strm).internal.cast(), (*strm).allocator);
    (*strm).internal = ptr::null_mut();
}

pub(crate) unsafe fn lzma_get_progress_impl(
    strm: *mut lzma_stream,
    progress_in: *mut u64,
    progress_out: *mut u64,
) {
    let mut totals = if strm.is_null() {
        (0, 0)
    } else {
        ((*strm).total_in, (*strm).total_out)
    };

    if !strm.is_null() && !(*strm).internal.is_null() {
        let state = &*state_ptr(strm);
        if let CoderInterface::Registered(next) = state.next {
            if let Some(get_progress) = next.get_progress {
                get_progress(next.coder, &mut totals.0, &mut totals.1);
            }
        }
    }

    if !progress_in.is_null() {
        *progress_in = totals.0;
    }
    if !progress_out.is_null() {
        *progress_out = totals.1;
    }
}

pub(crate) unsafe fn lzma_get_check_impl(strm: *const lzma_stream) -> lzma_check {
    if strm.is_null() || (*strm).internal.is_null() {
        return crate::ffi::types::LZMA_CHECK_NONE;
    }

    let state = &*state_ref(strm);
    match state.next {
        CoderInterface::Registered(next) => next
            .get_check
            .map(|get_check| get_check(next.coder.cast_const()))
            .unwrap_or(crate::ffi::types::LZMA_CHECK_NONE),
        CoderInterface::Uninitialized => crate::ffi::types::LZMA_CHECK_NONE,
    }
}

pub(crate) unsafe fn lzma_memusage_impl(strm: *const lzma_stream) -> u64 {
    let mut memusage = 0u64;
    let mut old_memlimit = 0u64;

    if strm.is_null() || (*strm).internal.is_null() {
        return 0;
    }

    let state = &*state_ref(strm);
    let CoderInterface::Registered(next) = state.next else {
        return 0;
    };

    let Some(memconfig) = next.memconfig else {
        return 0;
    };

    if memconfig(next.coder, &mut memusage, &mut old_memlimit, 0) != LZMA_OK {
        return 0;
    }

    memusage
}

pub(crate) unsafe fn lzma_memlimit_get_impl(strm: *const lzma_stream) -> u64 {
    let mut memusage = 0u64;
    let mut old_memlimit = 0u64;

    if strm.is_null() || (*strm).internal.is_null() {
        return 0;
    }

    let state = &*state_ref(strm);
    let CoderInterface::Registered(next) = state.next else {
        return 0;
    };

    let Some(memconfig) = next.memconfig else {
        return 0;
    };

    if memconfig(next.coder, &mut memusage, &mut old_memlimit, 0) != LZMA_OK {
        return 0;
    }

    old_memlimit
}

pub(crate) unsafe fn lzma_memlimit_set_impl(strm: *mut lzma_stream, new_memlimit: u64) -> lzma_ret {
    if strm.is_null() || (*strm).internal.is_null() {
        return LZMA_PROG_ERROR;
    }

    let state = &mut *state_ptr(strm);
    let CoderInterface::Registered(next) = state.next else {
        return LZMA_PROG_ERROR;
    };

    let Some(memconfig) = next.memconfig else {
        return LZMA_PROG_ERROR;
    };

    let mut memusage = 0u64;
    let mut old_memlimit = 0u64;
    let new_memlimit = if new_memlimit == 0 { 1 } else { new_memlimit };

    memconfig(next.coder, &mut memusage, &mut old_memlimit, new_memlimit)
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;

    use super::*;
    use crate::ffi::types::{lzma_allocator, LZMA_STREAM_INIT};
    use crate::internal::common::all_supported_actions;

    struct TestCoder {
        scripted: VecDeque<(lzma_ret, usize, usize)>,
        progress: Option<(u64, u64)>,
        memusage: u64,
        memlimit: u64,
    }

    unsafe fn test_code(
        coder: *mut c_void,
        _allocator: *const lzma_allocator,
        _input: *const u8,
        in_pos: *mut usize,
        _in_size: usize,
        _output: *mut u8,
        out_pos: *mut usize,
        _out_size: usize,
        _action: lzma_action,
    ) -> lzma_ret {
        let coder = &mut *coder.cast::<TestCoder>();
        let (ret, consumed, produced) = coder.scripted.pop_front().unwrap();
        *in_pos = consumed;
        *out_pos = produced;
        ret
    }

    unsafe fn test_end(coder: *mut c_void, _allocator: *const lzma_allocator) {
        drop(Box::from_raw(coder.cast::<TestCoder>()));
    }

    unsafe fn test_progress(coder: *mut c_void, progress_in: *mut u64, progress_out: *mut u64) {
        let coder = &mut *coder.cast::<TestCoder>();
        let (pin, pout) = coder.progress.unwrap();
        *progress_in = pin;
        *progress_out = pout;
    }

    unsafe fn test_memconfig(
        coder: *mut c_void,
        memusage: *mut u64,
        old_memlimit: *mut u64,
        new_memlimit: u64,
    ) -> lzma_ret {
        let coder = &mut *coder.cast::<TestCoder>();
        *memusage = coder.memusage;
        *old_memlimit = coder.memlimit;
        if new_memlimit != 0 {
            coder.memlimit = new_memlimit;
        }
        LZMA_OK
    }

    unsafe fn install_test_coder(strm: *mut lzma_stream, scripted: &[(lzma_ret, usize, usize)]) {
        let coder = Box::new(TestCoder {
            scripted: scripted.iter().copied().collect(),
            progress: Some((123, 456)),
            memusage: 4096,
            memlimit: 8192,
        });
        let next = NextCoder {
            coder: Box::into_raw(coder).cast(),
            code: test_code,
            end: Some(test_end),
            get_progress: Some(test_progress),
            get_check: None,
            memconfig: Some(test_memconfig),
        };
        assert_eq!(
            install_next_coder(strm, next, all_supported_actions()),
            LZMA_OK
        );
    }

    #[test]
    fn buf_error_requires_two_zero_progress_calls() {
        let mut strm = LZMA_STREAM_INIT;
        unsafe {
            install_test_coder(&mut strm, &[(LZMA_OK, 0, 0), (LZMA_OK, 0, 0)]);
            assert_eq!(lzma_code_impl(&mut strm, LZMA_RUN), LZMA_OK);
            assert_eq!(lzma_code_impl(&mut strm, LZMA_RUN), LZMA_BUF_ERROR);
            lzma_end_impl(&mut strm);
        }
    }

    #[test]
    fn finish_sequence_requires_stable_avail_in() {
        let mut strm = LZMA_STREAM_INIT;
        let input = [1u8, 2, 3];
        let mut output = [0u8; 8];

        unsafe {
            install_test_coder(&mut strm, &[(LZMA_OK, 0, 0)]);
            strm.next_in = input.as_ptr();
            strm.avail_in = input.len();
            strm.next_out = output.as_mut_ptr();
            strm.avail_out = output.len();
            assert_eq!(lzma_code_impl(&mut strm, LZMA_FINISH), LZMA_OK);

            strm.avail_in = input.len() - 1;
            assert_eq!(lzma_code_impl(&mut strm, LZMA_FINISH), LZMA_PROG_ERROR);
            lzma_end_impl(&mut strm);
        }
    }

    #[test]
    fn sync_flush_returns_to_run_after_stream_end() {
        let mut strm = LZMA_STREAM_INIT;
        let mut output = [0u8; 1];

        unsafe {
            install_test_coder(&mut strm, &[(LZMA_STREAM_END, 0, 1), (LZMA_OK, 0, 0)]);
            strm.next_out = output.as_mut_ptr();
            strm.avail_out = output.len();
            assert_eq!(lzma_code_impl(&mut strm, LZMA_SYNC_FLUSH), LZMA_STREAM_END);
            strm.avail_out = output.len();
            assert_eq!(lzma_code_impl(&mut strm, LZMA_RUN), LZMA_OK);
            lzma_end_impl(&mut strm);
        }
    }

    #[test]
    fn reserved_members_fail_with_options_error() {
        let mut strm = LZMA_STREAM_INIT;
        let mut output = [0u8; 1];

        unsafe {
            install_test_coder(&mut strm, &[(LZMA_OK, 0, 0)]);
            strm.next_out = output.as_mut_ptr();
            strm.avail_out = output.len();
            strm.reserved_int2 = 1;
            assert_eq!(
                lzma_code_impl(&mut strm, LZMA_RUN),
                crate::ffi::types::LZMA_OPTIONS_ERROR
            );
            lzma_end_impl(&mut strm);
        }
    }

    #[test]
    fn progress_and_memconfig_use_registered_callbacks() {
        let mut strm = LZMA_STREAM_INIT;

        unsafe {
            install_test_coder(&mut strm, &[(LZMA_OK, 0, 0)]);

            let mut pin = 0;
            let mut pout = 0;
            lzma_get_progress_impl(&mut strm, &mut pin, &mut pout);
            assert_eq!((pin, pout), (123, 456));

            assert_eq!(lzma_memusage_impl(&strm), 4096);
            assert_eq!(lzma_memlimit_get_impl(&strm), 8192);
            assert_eq!(lzma_memlimit_set_impl(&mut strm, 0), LZMA_OK);
            assert_eq!(lzma_memlimit_get_impl(&strm), 1);

            lzma_end_impl(&mut strm);
        }
    }
}
