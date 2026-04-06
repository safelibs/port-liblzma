use core::ffi::c_void;
use core::{mem, ptr};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crate::ffi::types::{
    lzma_action, lzma_allocator, lzma_block, lzma_check, lzma_filter, lzma_index_hash, lzma_mt,
    lzma_ret, lzma_stream, lzma_stream_flags, lzma_vli, LZMA_DATA_ERROR, LZMA_FILTERS_MAX,
    LZMA_FORMAT_ERROR, LZMA_GET_CHECK, LZMA_MEMLIMIT_ERROR, LZMA_NO_CHECK, LZMA_OK,
    LZMA_OPTIONS_ERROR, LZMA_PROG_ERROR, LZMA_STREAM_END, LZMA_UNSUPPORTED_CHECK, LZMA_VLI_UNKNOWN,
};
use crate::internal::block;
use crate::internal::check;
use crate::internal::common::{lzma_bool as to_lzma_bool, LZMA_FINISH, LZMA_RUN, LZMA_TIMED_OUT};
use crate::internal::container::outqueue::{OutBuf, OutQueue};
use crate::internal::container::stream::{
    LZMA_CONCATENATED, LZMA_FAIL_FAST, LZMA_IGNORE_CHECK, LZMA_TELL_ANY_CHECK, LZMA_TELL_NO_CHECK,
    LZMA_TELL_UNSUPPORTED_CHECK, STREAM_DECODER_SUPPORTED_FLAGS,
};
use crate::internal::filter;
use crate::internal::index::{
    index_hash_append, index_hash_decode, index_hash_end, index_hash_init, index_hash_size,
};
use crate::internal::lzma::LZMA_MEMUSAGE_BASE;
use crate::internal::stream_flags::{
    stream_flags_compare_impl, stream_footer_decode_impl, stream_header_decode_impl,
    LZMA_STREAM_HEADER_SIZE,
};
use crate::internal::stream_state::{install_next_coder, lzma_end_impl, NextCoder};
use crate::internal::upstream;

const LZMA_THREADS_MAX: u32 = 16_384;
const LZMA_INDEX_DETECTED: lzma_ret = 102;

type FilterArray = [lzma_filter; LZMA_FILTERS_MAX + 1];

fn lock<'a, T>(mutex: &'a Mutex<T>) -> MutexGuard<'a, T> {
    mutex.lock().unwrap_or_else(|poison| poison.into_inner())
}

#[derive(Copy, Clone)]
struct AllocatorPtr(*const lzma_allocator);

unsafe impl Send for AllocatorPtr {}

struct OwnedFilters {
    filters: FilterArray,
}

impl OwnedFilters {
    unsafe fn copy_from(src: *const lzma_filter) -> Result<Self, lzma_ret> {
        upstream::copy_filters(src).map(|filters| Self { filters })
    }

    fn as_mut_ptr(&mut self) -> *mut lzma_filter {
        self.filters.as_mut_ptr()
    }
}

impl Drop for OwnedFilters {
    fn drop(&mut self) {
        unsafe { upstream::free_filters(&mut self.filters) }
    }
}

unsafe impl Send for OwnedFilters {}

#[derive(Copy, Clone, Eq, PartialEq)]
enum DecoderSequence {
    StreamHeader,
    BlockHeader,
    BlockInit,
    BlockThreadInit,
    BlockThreadRun,
    BlockDirectInit,
    BlockDirectRun,
    IndexWaitOutput,
    IndexDecode,
    StreamFooter,
    StreamPadding,
    Error,
}

#[derive(Copy, Clone, Eq, PartialEq)]
enum DecoderCommand {
    Idle,
    Run,
    Exit,
}

#[derive(Copy, Clone, Eq, PartialEq)]
enum PartialMode {
    Disabled,
    Start,
    Enabled,
}

struct DecoderBlockJob {
    check: lzma_check,
    header_size: u32,
    compressed_size: lzma_vli,
    uncompressed_size: lzma_vli,
    ignore_check: bool,
    filters: OwnedFilters,
    mem_filters: u64,
}

struct DecoderWorkerState {
    command: DecoderCommand,
    input: Vec<u8>,
    in_size: usize,
    in_filled: usize,
    outbuf: Option<Arc<OutBuf>>,
    job: Option<DecoderBlockJob>,
    progress_in: usize,
    progress_out: usize,
    partial: PartialMode,
    poisoned: bool,
}

struct DecoderWorkerShared {
    state: Mutex<DecoderWorkerState>,
    cond: Condvar,
}

struct DecoderWorker {
    shared: Arc<DecoderWorkerShared>,
    handle: Option<JoinHandle<()>>,
}

struct DecoderSharedState {
    outq: OutQueue,
    free_workers: Vec<usize>,
    thread_error: lzma_ret,
    progress_in: u64,
    progress_out: u64,
    mem_in_use: u64,
}

struct DecoderShared {
    state: Mutex<DecoderSharedState>,
    cond: Condvar,
}

struct DirectDecoder {
    inner: lzma_stream,
}

struct StreamDecoderMt {
    sequence: DecoderSequence,
    block_options: lzma_block,
    filters: FilterArray,
    stream_flags: lzma_stream_flags,
    index_hash: *mut lzma_index_hash,
    timeout: u32,
    pending_error: lzma_ret,
    current_worker: Option<usize>,
    shared: Arc<DecoderShared>,
    workers: Vec<DecoderWorker>,
    memlimit_threading: u64,
    memlimit_stop: u64,
    mem_direct_mode: u64,
    mem_next_filters: u64,
    mem_next_in: u64,
    mem_next_block: u64,
    tell_no_check: bool,
    tell_unsupported_check: bool,
    tell_any_check: bool,
    ignore_check: bool,
    concatenated: bool,
    fail_fast: bool,
    first_stream: bool,
    out_was_filled: bool,
    pos: usize,
    buffer: [u8; crate::internal::block::header::LZMA_BLOCK_HEADER_SIZE_MAX as usize],
    direct: Option<DirectDecoder>,
}

fn decoder_supported_actions() -> [bool; crate::internal::common::ACTION_COUNT] {
    let mut actions = [false; crate::internal::common::ACTION_COUNT];
    actions[LZMA_RUN as usize] = true;
    actions[LZMA_FINISH as usize] = true;
    actions
}

#[inline]
fn ceil4(value: u64) -> u64 {
    (value + 3) & !3
}

#[inline]
fn comp_block_size(block: &lzma_block) -> usize {
    (ceil4(block.compressed_size) + check::check_size(block.check) as u64) as usize
}

#[inline]
fn is_direct_mode_needed(size: lzma_vli) -> bool {
    size == LZMA_VLI_UNKNOWN || size > usize::MAX as u64 / 3
}

unsafe fn decode_block_header(
    coder: &mut StreamDecoderMt,
    allocator: *const lzma_allocator,
    input: *const u8,
    in_pos: *mut usize,
    in_size: usize,
) -> lzma_ret {
    if *in_pos >= in_size {
        return LZMA_OK;
    }

    if coder.pos == 0 {
        if *input.add(*in_pos) == 0x00 {
            return LZMA_INDEX_DETECTED;
        }
        coder.block_options.header_size = (u32::from(*input.add(*in_pos)) + 1) * 4;
    }

    let copy_size = (coder.block_options.header_size as usize - coder.pos).min(in_size - *in_pos);
    ptr::copy_nonoverlapping(
        input.add(*in_pos),
        coder.buffer.as_mut_ptr().add(coder.pos),
        copy_size,
    );
    coder.pos += copy_size;
    *in_pos += copy_size;

    if coder.pos < coder.block_options.header_size as usize {
        return LZMA_OK;
    }

    coder.pos = 0;
    coder.block_options.version = 1;
    coder.block_options.filters = coder.filters.as_mut_ptr();
    let ret =
        block::block_header_decode(&mut coder.block_options, allocator, coder.buffer.as_ptr());
    if ret != LZMA_OK {
        return ret;
    }

    coder.block_options.ignore_check = to_lzma_bool(coder.ignore_check);
    LZMA_STREAM_END
}

fn request_partial_output(coder: &StreamDecoderMt, worker_id: usize) {
    let worker = &coder.workers[worker_id];
    let mut state = lock(&worker.shared.state);
    if state.partial == PartialMode::Disabled {
        state.partial = PartialMode::Start;
        worker.shared.cond.notify_all();
    }
}

fn abort_workers(coder: &StreamDecoderMt) {
    for worker in &coder.workers {
        let mut state = lock(&worker.shared.state);
        state.command = DecoderCommand::Exit;
        worker.shared.cond.notify_all();
    }
}

fn threaded_input_possible(coder: &StreamDecoderMt, shared: &DecoderSharedState) -> bool {
    coder.memlimit_threading
        >= shared
            .mem_in_use
            .saturating_add(shared.outq.mem_in_use)
            .saturating_add(coder.mem_next_block)
        && shared.outq.has_buf()
        && !shared.free_workers.is_empty()
}

fn wait_on_decoder(
    coder: &mut StreamDecoderMt,
    waiting_allowed: bool,
    wait_deadline: &mut Option<Instant>,
    mut input_possible: Option<&mut bool>,
    out: *mut u8,
    out_pos: *mut usize,
    out_size: usize,
) -> lzma_ret {
    loop {
        let mut shared = lock(&coder.shared.state);

        let out_start = unsafe { *out_pos };
        loop {
            let ret = unsafe { shared.outq.read(out, out_pos, out_size) };
            if ret.ret != LZMA_STREAM_END {
                if ret.ret != LZMA_OK {
                    drop(shared);
                    abort_workers(coder);
                    return ret.ret;
                }
                break;
            }

            if let Some(worker_id) = shared.outq.take_head_worker() {
                drop(shared);
                request_partial_output(coder, worker_id);
                shared = lock(&coder.shared.state);
            }
        }

        if unsafe { *out_pos == out_size && *out_pos != out_start } {
            drop(shared);
            // The caller observes this through coder.out_was_filled.
            return LZMA_OK;
        }

        if shared.thread_error != LZMA_OK {
            if coder.fail_fast {
                let ret = shared.thread_error;
                drop(shared);
                abort_workers(coder);
                return ret;
            }

            coder.pending_error = LZMA_PROG_ERROR;
        }

        if let Some(flag) = input_possible.as_deref_mut() {
            if threaded_input_possible(coder, &shared) {
                *flag = true;
                return LZMA_OK;
            }
        }

        if !waiting_allowed {
            return LZMA_OK;
        }

        if input_possible.is_none() && shared.outq.is_empty() {
            return LZMA_OK;
        }

        if shared.outq.is_readable() {
            return LZMA_OK;
        }

        if let Some(worker_id) = coder.current_worker {
            let worker = &coder.workers[worker_id];
            let state = lock(&worker.shared.state);
            if state.partial != PartialMode::Disabled {
                if let Some(outbuf) = &state.outbuf {
                    let out_state = lock(&outbuf.state);
                    if out_state.decoder_in_pos == state.in_filled {
                        return LZMA_OK;
                    }
                }
            }
        }

        if coder.timeout == 0 {
            shared = match coder.shared.cond.wait(shared) {
                Ok(shared) => shared,
                Err(poison) => poison.into_inner(),
            };
            drop(shared);
            continue;
        }

        if wait_deadline.is_none() {
            *wait_deadline = Some(Instant::now() + Duration::from_millis(coder.timeout as u64));
        }

        let end = wait_deadline.expect("deadline");
        let now = Instant::now();
        if now >= end {
            return LZMA_TIMED_OUT;
        }
        let wait = end.saturating_duration_since(now);
        let (shared, timed) = match coder.shared.cond.wait_timeout(shared, wait) {
            Ok(result) => result,
            Err(poison) => poison.into_inner(),
        };
        if timed.timed_out() {
            drop(shared);
            return LZMA_TIMED_OUT;
        }
        drop(shared);
    }
}

fn decoder_worker_loop(
    worker_id: usize,
    allocator: AllocatorPtr,
    shared: Arc<DecoderShared>,
    worker: Arc<DecoderWorkerShared>,
) {
    loop {
        let (mut job, outbuf) = {
            let mut state = worker
                .state
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            while state.command == DecoderCommand::Idle {
                state = worker
                    .cond
                    .wait(state)
                    .unwrap_or_else(|poison| poison.into_inner());
            }

            if state.command == DecoderCommand::Exit {
                break;
            }

            (
                state.job.take().expect("decoder worker job missing"),
                state.outbuf.clone().expect("decoder worker outbuf missing"),
            )
        };

        let mut block_opts: lzma_block = unsafe { mem::zeroed() };
        block_opts.version = 1;
        block_opts.header_size = job.header_size;
        block_opts.check = job.check;
        block_opts.compressed_size = job.compressed_size;
        block_opts.uncompressed_size = job.uncompressed_size;
        block_opts.ignore_check = to_lzma_bool(job.ignore_check);
        block_opts.filters = job.filters.as_mut_ptr();

        let chain = unsafe { crate::internal::lzma::parse_filters(job.filters.filters.as_ptr()) };
        let (mut ret, in_pos, out_pos) = match chain {
            Ok(chain) => {
                let mut checked_input = 0usize;
                let mut ret = LZMA_OK;
                let mut in_pos = 0usize;
                let mut out_pos = 0usize;

                while ret == LZMA_OK {
                    let (available, total_size, partial, input_copy) = {
                        let mut state = worker
                            .state
                            .lock()
                            .unwrap_or_else(|poison| poison.into_inner());
                        state.progress_in = in_pos;
                        state.progress_out = out_pos;

                        while state.command == DecoderCommand::Run
                            && state.in_filled == checked_input
                            && state.partial != PartialMode::Start
                        {
                            state = worker
                                .cond
                                .wait(state)
                                .unwrap_or_else(|poison| poison.into_inner());
                        }

                        if state.command == DecoderCommand::Exit {
                            ret = LZMA_PROG_ERROR;
                            (0, 0, state.partial, Vec::new())
                        } else {
                            let partial = state.partial;
                            if partial == PartialMode::Start {
                                state.partial = PartialMode::Enabled;
                            }
                            (
                                state.in_filled,
                                state.in_size,
                                partial,
                                state.input[..state.in_filled].to_vec(),
                            )
                        }
                    };

                    if ret != LZMA_OK {
                        break;
                    }

                    if partial != PartialMode::Disabled {
                        let mut out_state = lock(&outbuf.state);
                        out_state.pos = out_pos;
                        out_state.decoder_in_pos = available;
                        drop(out_state);
                        shared.cond.notify_all();
                    }

                    checked_input = available;
                    if available < total_size {
                        match crate::internal::lzma::decode_raw(&chain, &input_copy) {
                            Err(crate::ffi::types::LZMA_BUF_ERROR) => continue,
                            Err(next_ret) => {
                                ret = next_ret;
                                break;
                            }
                            Ok(_) => {
                                ret = LZMA_DATA_ERROR;
                                break;
                            }
                        }
                    }

                    let mut full_in_pos = 0usize;
                    let mut full_out_pos = 0usize;
                    let decode_ret = {
                        let mut out_state = lock(&outbuf.state);
                        unsafe {
                            block::block_buffer_decode(
                                &mut block_opts,
                                allocator.0,
                                input_copy.as_ptr(),
                                &mut full_in_pos,
                                input_copy.len(),
                                out_state.data.as_mut_ptr(),
                                &mut full_out_pos,
                                out_state.data.len(),
                            )
                        }
                    };

                    in_pos = full_in_pos;
                    out_pos = full_out_pos;
                    ret = if decode_ret == LZMA_OK {
                        LZMA_STREAM_END
                    } else {
                        decode_ret
                    };
                }

                (ret, in_pos, out_pos)
            }
            Err(ret) => (ret, 0, 0),
        };

        let (state_in_size, state_in_filled) = {
            let state = lock(&worker.state);
            (state.in_size, state.in_filled)
        };
        if ret == LZMA_STREAM_END && state_in_filled != state_in_size {
            ret = LZMA_PROG_ERROR;
        }

        {
            let mut shared_state = lock(&shared.state);
            {
                let mut out_state = lock(&outbuf.state);
                out_state.pos = out_pos;
                out_state.decoder_in_pos = in_pos;
                out_state.finished = true;
                out_state.finish_ret = ret;
            }

            shared_state.progress_in = shared_state.progress_in.saturating_add(in_pos as u64);
            shared_state.progress_out = shared_state.progress_out.saturating_add(out_pos as u64);
            if ret == LZMA_STREAM_END {
                shared_state.mem_in_use = shared_state
                    .mem_in_use
                    .saturating_sub(state_in_size as u64 + job.mem_filters);
                shared_state.free_workers.push(worker_id);
            } else if shared_state.thread_error == LZMA_OK {
                shared_state.thread_error = ret;
            }
        }
        shared.cond.notify_all();

        let mut state = lock(&worker.state);
        state.progress_in = 0;
        state.progress_out = 0;
        state.outbuf = None;
        state.job = None;
        state.partial = PartialMode::Disabled;
        if ret != LZMA_STREAM_END {
            state.poisoned = true;
        }
        if state.command != DecoderCommand::Exit {
            state.command = DecoderCommand::Idle;
        }
    }
}

impl StreamDecoderMt {
    unsafe fn new(
        allocator: *const lzma_allocator,
        options: *const lzma_mt,
    ) -> Result<Box<Self>, lzma_ret> {
        let options_ref = options.as_ref().ok_or(LZMA_PROG_ERROR)?;
        if options_ref.threads == 0 || options_ref.threads > LZMA_THREADS_MAX {
            return Err(LZMA_OPTIONS_ERROR);
        }
        if (options_ref.flags & !STREAM_DECODER_SUPPORTED_FLAGS) != 0 {
            return Err(LZMA_OPTIONS_ERROR);
        }

        let shared = Arc::new(DecoderShared {
            state: Mutex::new(DecoderSharedState {
                outq: OutQueue::default(),
                free_workers: Vec::new(),
                thread_error: LZMA_OK,
                progress_in: 0,
                progress_out: 0,
                mem_in_use: 0,
            }),
            cond: Condvar::new(),
        });
        lock(&shared.state).outq.init(options_ref.threads)?;

        let mut workers = Vec::with_capacity(options_ref.threads as usize);
        for worker_id in 0..options_ref.threads as usize {
            let worker_shared = Arc::new(DecoderWorkerShared {
                state: Mutex::new(DecoderWorkerState {
                    command: DecoderCommand::Idle,
                    input: Vec::new(),
                    in_size: 0,
                    in_filled: 0,
                    outbuf: None,
                    job: None,
                    progress_in: 0,
                    progress_out: 0,
                    partial: PartialMode::Disabled,
                    poisoned: false,
                }),
                cond: Condvar::new(),
            });

            let handle = thread::Builder::new()
                .name(format!("lzma-dec-mt-{worker_id}"))
                .spawn({
                    let shared = shared.clone();
                    let worker = worker_shared.clone();
                    let allocator = AllocatorPtr(allocator);
                    move || decoder_worker_loop(worker_id, allocator, shared, worker)
                })
                .map_err(|_| crate::ffi::types::LZMA_MEM_ERROR)?;

            workers.push(DecoderWorker {
                shared: worker_shared,
                handle: Some(handle),
            });
            lock(&shared.state).free_workers.push(worker_id);
        }

        let mut coder = Box::new(Self {
            sequence: DecoderSequence::StreamHeader,
            block_options: mem::zeroed(),
            filters: [lzma_filter {
                id: LZMA_VLI_UNKNOWN,
                options: ptr::null_mut(),
            }; LZMA_FILTERS_MAX + 1],
            stream_flags: mem::zeroed(),
            index_hash: ptr::null_mut(),
            timeout: options_ref.timeout,
            pending_error: LZMA_OK,
            current_worker: None,
            shared,
            workers,
            memlimit_threading: options_ref.memlimit_threading.max(1),
            memlimit_stop: options_ref.memlimit_stop.max(1),
            mem_direct_mode: 0,
            mem_next_filters: 0,
            mem_next_in: 0,
            mem_next_block: 0,
            tell_no_check: (options_ref.flags & LZMA_TELL_NO_CHECK) != 0,
            tell_unsupported_check: (options_ref.flags & LZMA_TELL_UNSUPPORTED_CHECK) != 0,
            tell_any_check: (options_ref.flags & LZMA_TELL_ANY_CHECK) != 0,
            ignore_check: (options_ref.flags & LZMA_IGNORE_CHECK) != 0,
            concatenated: (options_ref.flags & LZMA_CONCATENATED) != 0,
            fail_fast: (options_ref.flags & LZMA_FAIL_FAST) != 0,
            first_stream: true,
            out_was_filled: false,
            pos: 0,
            buffer: [0; crate::internal::block::header::LZMA_BLOCK_HEADER_SIZE_MAX as usize],
            direct: None,
        });

        if coder.memlimit_threading > coder.memlimit_stop {
            coder.memlimit_threading = coder.memlimit_stop;
        }

        coder.reset_stream(allocator)?;
        Ok(coder)
    }

    unsafe fn reset_stream(&mut self, allocator: *const lzma_allocator) -> Result<(), lzma_ret> {
        self.index_hash = index_hash_init(self.index_hash, allocator);
        if self.index_hash.is_null() {
            return Err(crate::ffi::types::LZMA_MEM_ERROR);
        }
        self.sequence = DecoderSequence::StreamHeader;
        self.pos = 0;
        self.pending_error = LZMA_OK;
        self.block_options = mem::zeroed();
        self.stream_flags = mem::zeroed();
        self.current_worker = None;
        Ok(())
    }

    unsafe fn prepare_worker(&mut self, allocator: *const lzma_allocator) -> Result<(), lzma_ret> {
        {
            let mut shared = lock(&self.shared.state);
            if self.memlimit_threading
                < shared
                    .mem_in_use
                    .saturating_add(shared.outq.mem_allocated)
                    .saturating_add(self.mem_next_block)
            {
                shared
                    .outq
                    .clear_cache_keep(self.block_options.uncompressed_size as usize);
            }
        }

        let (worker_id, outbuf) = {
            let mut shared = lock(&self.shared.state);
            if !threaded_input_possible(self, &shared) {
                return Err(LZMA_OK);
            }

            shared
                .outq
                .prealloc_buf(self.block_options.uncompressed_size as usize)?;
            let worker_id = shared.free_workers.pop().expect("free decoder worker");
            shared.mem_in_use = shared
                .mem_in_use
                .saturating_add(self.mem_next_in + self.mem_next_filters);
            let outbuf = shared.outq.get_buf(Some(worker_id));
            (worker_id, outbuf)
        };

        let worker = &self.workers[worker_id];
        let mut state = lock(&worker.shared.state);
        state.input.resize(self.mem_next_in as usize, 0);
        state.in_size = self.mem_next_in as usize;
        state.in_filled = 0;
        state.outbuf = Some(outbuf);
        state.partial = PartialMode::Disabled;
        state.poisoned = false;
        state.job = Some(DecoderBlockJob {
            check: self.stream_flags.check,
            header_size: self.block_options.header_size,
            compressed_size: self.block_options.compressed_size,
            uncompressed_size: self.block_options.uncompressed_size,
            ignore_check: self.ignore_check,
            filters: OwnedFilters::copy_from(self.filters.as_ptr())?,
            mem_filters: self.mem_next_filters,
        });
        state.command = DecoderCommand::Run;
        worker.shared.cond.notify_all();
        self.current_worker = Some(worker_id);
        drop(state);
        request_partial_output(self, worker_id);
        let _ = allocator;
        Ok(())
    }

    unsafe fn start_direct(&mut self, allocator: *const lzma_allocator) -> lzma_ret {
        let mut inner = crate::ffi::types::LZMA_STREAM_INIT;
        inner.allocator = allocator;
        let ret = block::block_decoder(&mut inner, &mut self.block_options);
        if ret != LZMA_OK {
            return ret;
        }
        filter::filters_free_impl(self.filters.as_mut_ptr(), allocator);
        self.block_options.filters = ptr::null_mut();
        self.direct = Some(DirectDecoder { inner });
        self.mem_direct_mode = self.mem_next_filters;
        self.sequence = DecoderSequence::BlockDirectRun;
        LZMA_OK
    }

    unsafe fn get_progress(&self, progress_in: *mut u64, progress_out: *mut u64) {
        let shared = lock(&self.shared.state);
        let mut in_total = shared.progress_in;
        let mut out_total = shared.progress_out;

        for worker in &self.workers {
            let state = lock(&worker.shared.state);
            in_total = in_total.saturating_add(state.progress_in as u64);
            out_total = out_total.saturating_add(state.progress_out as u64);
        }
        drop(shared);

        if let Some(direct) = &self.direct {
            in_total = in_total.saturating_add(direct.inner.total_in);
            out_total = out_total.saturating_add(direct.inner.total_out);
        }

        *progress_in = in_total;
        *progress_out = out_total;
    }

    unsafe fn memconfig(
        &mut self,
        memusage: *mut u64,
        old_memlimit: *mut u64,
        new_memlimit: u64,
    ) -> lzma_ret {
        let shared = lock(&self.shared.state);
        *memusage = self
            .mem_direct_mode
            .saturating_add(shared.mem_in_use)
            .saturating_add(shared.outq.mem_allocated)
            .max(LZMA_MEMUSAGE_BASE);
        drop(shared);

        *old_memlimit = self.memlimit_stop;
        if new_memlimit != 0 {
            if new_memlimit < *memusage {
                return LZMA_MEMLIMIT_ERROR;
            }
            self.memlimit_stop = new_memlimit;
        }
        LZMA_OK
    }

    unsafe fn code(
        &mut self,
        allocator: *const lzma_allocator,
        input: *const u8,
        in_pos: *mut usize,
        in_size: usize,
        output: *mut u8,
        out_pos: *mut usize,
        out_size: usize,
        action: lzma_action,
    ) -> lzma_ret {
        let waiting_allowed = action == LZMA_FINISH || (*in_pos == in_size && !self.out_was_filled);
        self.out_was_filled = false;
        let mut wait_deadline = None;

        loop {
            match self.sequence {
                DecoderSequence::StreamHeader => {
                    let take = (LZMA_STREAM_HEADER_SIZE - self.pos).min(in_size - *in_pos);
                    if take != 0 {
                        ptr::copy_nonoverlapping(
                            input.add(*in_pos),
                            self.buffer.as_mut_ptr().add(self.pos),
                            take,
                        );
                        *in_pos += take;
                        lock(&self.shared.state).progress_in += take as u64;
                        self.pos += take;
                    }
                    if self.pos < LZMA_STREAM_HEADER_SIZE {
                        return LZMA_OK;
                    }
                    self.pos = 0;
                    let ret =
                        stream_header_decode_impl(&mut self.stream_flags, self.buffer.as_ptr());
                    if ret != LZMA_OK {
                        return if ret == LZMA_FORMAT_ERROR && !self.first_stream {
                            LZMA_DATA_ERROR
                        } else {
                            ret
                        };
                    }
                    self.first_stream = false;
                    self.block_options.check = self.stream_flags.check;
                    self.sequence = DecoderSequence::BlockHeader;

                    if self.tell_no_check
                        && self.stream_flags.check == crate::ffi::types::LZMA_CHECK_NONE
                    {
                        return LZMA_NO_CHECK;
                    }
                    if self.tell_unsupported_check
                        && check::check_is_supported(self.stream_flags.check) == 0
                    {
                        return LZMA_UNSUPPORTED_CHECK;
                    }
                    if self.tell_any_check {
                        return LZMA_GET_CHECK;
                    }
                }

                DecoderSequence::BlockHeader => {
                    let in_old = *in_pos;
                    let ret = decode_block_header(self, allocator, input, in_pos, in_size);
                    lock(&self.shared.state).progress_in += (*in_pos - in_old) as u64;

                    if ret == LZMA_OK {
                        let wait_ret = wait_on_decoder(
                            self,
                            waiting_allowed,
                            &mut wait_deadline,
                            None,
                            output,
                            out_pos,
                            out_size,
                        );
                        if wait_ret == LZMA_TIMED_OUT {
                            return wait_ret;
                        }
                        if wait_ret != LZMA_OK {
                            return wait_ret;
                        }
                        if self.pending_error != LZMA_OK {
                            self.sequence = DecoderSequence::Error;
                            continue;
                        }
                        if self.fail_fast && action == LZMA_FINISH {
                            abort_workers(self);
                            return LZMA_DATA_ERROR;
                        }
                        if *out_pos == out_size {
                            self.out_was_filled = true;
                        }
                        return LZMA_OK;
                    }

                    if ret == LZMA_INDEX_DETECTED {
                        self.sequence = DecoderSequence::IndexWaitOutput;
                        continue;
                    }

                    if ret != LZMA_STREAM_END {
                        self.pending_error = ret;
                        self.sequence = DecoderSequence::Error;
                        continue;
                    }

                    self.mem_next_filters =
                        crate::internal::lzma::decoder_memusage(self.filters.as_ptr());
                    if self.mem_next_filters == u64::MAX {
                        self.pending_error = LZMA_OPTIONS_ERROR;
                        self.sequence = DecoderSequence::Error;
                        continue;
                    }
                    self.sequence = DecoderSequence::BlockInit;
                }

                DecoderSequence::BlockInit => {
                    if self.mem_next_filters > self.memlimit_stop {
                        let ret = wait_on_decoder(
                            self,
                            true,
                            &mut wait_deadline,
                            None,
                            output,
                            out_pos,
                            out_size,
                        );
                        if ret == LZMA_TIMED_OUT {
                            return ret;
                        }
                        if ret != LZMA_OK {
                            return ret;
                        }
                        if !lock(&self.shared.state).outq.is_empty() {
                            return LZMA_OK;
                        }
                        return LZMA_MEMLIMIT_ERROR;
                    }

                    if is_direct_mode_needed(self.block_options.compressed_size)
                        || is_direct_mode_needed(self.block_options.uncompressed_size)
                    {
                        self.sequence = DecoderSequence::BlockDirectInit;
                        continue;
                    }

                    self.mem_next_in = comp_block_size(&self.block_options) as u64;
                    self.mem_next_block = self
                        .mem_next_filters
                        .saturating_add(self.mem_next_in)
                        .saturating_add(
                            (core::mem::size_of::<OutBuf>()
                                + self.block_options.uncompressed_size as usize)
                                as u64,
                        );
                    if self.mem_next_block > self.memlimit_threading {
                        self.sequence = DecoderSequence::BlockDirectInit;
                        continue;
                    }

                    let ret = index_hash_append(
                        self.index_hash,
                        block::block_unpadded_size(&self.block_options),
                        self.block_options.uncompressed_size,
                    );
                    if ret != LZMA_OK {
                        self.pending_error = ret;
                        self.sequence = DecoderSequence::Error;
                        continue;
                    }
                    self.sequence = DecoderSequence::BlockThreadInit;
                }

                DecoderSequence::BlockThreadInit => {
                    let mut can_start = false;
                    let ret = wait_on_decoder(
                        self,
                        true,
                        &mut wait_deadline,
                        Some(&mut can_start),
                        output,
                        out_pos,
                        out_size,
                    );
                    if ret == LZMA_TIMED_OUT {
                        return ret;
                    }
                    if ret != LZMA_OK {
                        return ret;
                    }
                    if self.pending_error != LZMA_OK {
                        self.sequence = DecoderSequence::Error;
                        continue;
                    }
                    if !can_start {
                        if *out_pos == out_size {
                            self.out_was_filled = true;
                        }
                        return LZMA_OK;
                    }
                    match self.prepare_worker(allocator) {
                        Ok(()) => {
                            filter::filters_free_impl(self.filters.as_mut_ptr(), allocator);
                            self.sequence = DecoderSequence::BlockThreadRun;
                        }
                        Err(LZMA_OK) => return LZMA_OK,
                        Err(ret) => return ret,
                    }
                }

                DecoderSequence::BlockThreadRun => {
                    if self.fail_fast && action == LZMA_FINISH {
                        let worker_id = self.current_worker.expect("decoder worker");
                        let state = lock(&self.workers[worker_id].shared.state);
                        let in_avail = in_size - *in_pos;
                        let in_needed = state.in_size.saturating_sub(state.in_filled);
                        if in_avail < in_needed {
                            abort_workers(self);
                            return LZMA_DATA_ERROR;
                        }
                    }

                    let worker_id = self.current_worker.expect("decoder worker");
                    {
                        let worker = &self.workers[worker_id];
                        let mut state = lock(&worker.shared.state);
                        let copy_size = (state.in_size - state.in_filled).min(in_size - *in_pos);
                        if copy_size != 0 {
                            ptr::copy_nonoverlapping(
                                input.add(*in_pos),
                                state.input.as_mut_ptr().add(state.in_filled),
                                copy_size,
                            );
                            state.in_filled += copy_size;
                            *in_pos += copy_size;
                            worker.shared.cond.notify_all();
                        }
                    }

                    let ret = wait_on_decoder(
                        self,
                        waiting_allowed,
                        &mut wait_deadline,
                        None,
                        output,
                        out_pos,
                        out_size,
                    );
                    if ret == LZMA_TIMED_OUT {
                        return ret;
                    }
                    if ret != LZMA_OK {
                        return ret;
                    }
                    if self.pending_error != LZMA_OK {
                        self.sequence = DecoderSequence::Error;
                        continue;
                    }

                    let state = lock(&self.workers[worker_id].shared.state);
                    if state.in_filled < state.in_size {
                        if *out_pos == out_size {
                            self.out_was_filled = true;
                        }
                        return LZMA_OK;
                    }
                    drop(state);

                    self.current_worker = None;
                    self.sequence = DecoderSequence::BlockHeader;
                }

                DecoderSequence::BlockDirectInit => {
                    let ret = wait_on_decoder(
                        self,
                        true,
                        &mut wait_deadline,
                        None,
                        output,
                        out_pos,
                        out_size,
                    );
                    if ret == LZMA_TIMED_OUT {
                        return ret;
                    }
                    if ret != LZMA_OK {
                        return ret;
                    }
                    if !lock(&self.shared.state).outq.is_empty() {
                        return LZMA_OK;
                    }
                    let ret = self.start_direct(allocator);
                    if ret != LZMA_OK {
                        return ret;
                    }
                }

                DecoderSequence::BlockDirectRun => {
                    let direct = self.direct.as_mut().expect("direct decoder");
                    let in_old = *in_pos;
                    let out_old = *out_pos;
                    direct.inner.next_in = if *in_pos == in_size {
                        ptr::null()
                    } else {
                        input.add(*in_pos)
                    };
                    direct.inner.avail_in = in_size - *in_pos;
                    direct.inner.next_out = if *out_pos == out_size {
                        ptr::null_mut()
                    } else {
                        output.add(*out_pos)
                    };
                    direct.inner.avail_out = out_size - *out_pos;
                    let ret =
                        crate::internal::stream_state::lzma_code_impl(&mut direct.inner, action);
                    *in_pos = in_size - direct.inner.avail_in;
                    *out_pos = out_size - direct.inner.avail_out;
                    {
                        let mut shared = lock(&self.shared.state);
                        shared.progress_in += (*in_pos - in_old) as u64;
                        shared.progress_out += (*out_pos - out_old) as u64;
                    }
                    if ret != LZMA_STREAM_END {
                        if *out_pos == out_size && *out_pos != out_old {
                            self.out_was_filled = true;
                        }
                        return ret;
                    }
                    let ret = index_hash_append(
                        self.index_hash,
                        block::block_unpadded_size(&self.block_options),
                        self.block_options.uncompressed_size,
                    );
                    if ret != LZMA_OK {
                        return ret;
                    }
                    lzma_end_impl(&mut direct.inner);
                    self.direct = None;
                    self.mem_direct_mode = 0;
                    self.sequence = DecoderSequence::BlockHeader;
                }

                DecoderSequence::IndexWaitOutput => {
                    let ret = wait_on_decoder(
                        self,
                        true,
                        &mut wait_deadline,
                        None,
                        output,
                        out_pos,
                        out_size,
                    );
                    if ret == LZMA_TIMED_OUT {
                        return ret;
                    }
                    if ret != LZMA_OK {
                        return ret;
                    }
                    if !lock(&self.shared.state).outq.is_empty() {
                        return LZMA_OK;
                    }
                    self.sequence = DecoderSequence::IndexDecode;
                }

                DecoderSequence::IndexDecode => {
                    if *in_pos >= in_size {
                        return LZMA_OK;
                    }
                    let in_old = *in_pos;
                    let ret = index_hash_decode(self.index_hash, input, in_pos, in_size);
                    lock(&self.shared.state).progress_in += (*in_pos - in_old) as u64;
                    if ret != LZMA_STREAM_END {
                        return ret;
                    }
                    self.sequence = DecoderSequence::StreamFooter;
                }

                DecoderSequence::StreamFooter => {
                    let take = (LZMA_STREAM_HEADER_SIZE - self.pos).min(in_size - *in_pos);
                    if take != 0 {
                        ptr::copy_nonoverlapping(
                            input.add(*in_pos),
                            self.buffer.as_mut_ptr().add(self.pos),
                            take,
                        );
                        *in_pos += take;
                        lock(&self.shared.state).progress_in += take as u64;
                        self.pos += take;
                    }
                    if self.pos < LZMA_STREAM_HEADER_SIZE {
                        return LZMA_OK;
                    }
                    self.pos = 0;

                    let mut footer: lzma_stream_flags = mem::zeroed();
                    let ret = stream_footer_decode_impl(&mut footer, self.buffer.as_ptr());
                    if ret != LZMA_OK {
                        return if ret == LZMA_FORMAT_ERROR {
                            LZMA_DATA_ERROR
                        } else {
                            ret
                        };
                    }
                    if index_hash_size(self.index_hash) != footer.backward_size {
                        return LZMA_DATA_ERROR;
                    }
                    let ret = stream_flags_compare_impl(&self.stream_flags, &footer);
                    if ret != LZMA_OK {
                        return ret;
                    }
                    if !self.concatenated {
                        return LZMA_STREAM_END;
                    }
                    self.sequence = DecoderSequence::StreamPadding;
                }

                DecoderSequence::StreamPadding => {
                    while *in_pos < in_size && *input.add(*in_pos) == 0 {
                        *in_pos += 1;
                        lock(&self.shared.state).progress_in += 1;
                        self.pos = (self.pos + 1) & 3;
                    }
                    if *in_pos >= in_size {
                        return if action == LZMA_FINISH {
                            if self.pos == 0 {
                                LZMA_STREAM_END
                            } else {
                                LZMA_DATA_ERROR
                            }
                        } else {
                            LZMA_OK
                        };
                    }
                    if self.pos != 0 {
                        *in_pos += 1;
                        lock(&self.shared.state).progress_in += 1;
                        return LZMA_DATA_ERROR;
                    }
                    if let Err(ret) = self.reset_stream(allocator) {
                        return ret;
                    }
                }

                DecoderSequence::Error => {
                    if !self.fail_fast {
                        let ret = wait_on_decoder(
                            self,
                            true,
                            &mut wait_deadline,
                            None,
                            output,
                            out_pos,
                            out_size,
                        );
                        if ret == LZMA_TIMED_OUT {
                            return ret;
                        }
                        if ret != LZMA_OK {
                            return ret;
                        }
                        if !lock(&self.shared.state).outq.is_empty() {
                            return LZMA_OK;
                        }
                    }
                    return self.pending_error;
                }
            }
        }
    }
}

unsafe fn decoder_code(
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
    (*coder.cast::<StreamDecoderMt>()).code(
        allocator, input, in_pos, in_size, output, out_pos, out_size, action,
    )
}

unsafe fn decoder_end(coder: *mut c_void, allocator: *const lzma_allocator) {
    let mut coder = Box::from_raw(coder.cast::<StreamDecoderMt>());
    abort_workers(&coder);
    for worker in &mut coder.workers {
        if let Some(handle) = worker.handle.take() {
            let _ = handle.join();
        }
    }
    lock(&coder.shared.state).outq.end();
    if let Some(mut direct) = coder.direct.take() {
        lzma_end_impl(&mut direct.inner);
    }
    filter::filters_free_impl(coder.filters.as_mut_ptr(), allocator);
    index_hash_end(coder.index_hash, allocator);
}

unsafe fn decoder_get_check(coder: *const c_void) -> lzma_check {
    (*(coder.cast::<StreamDecoderMt>())).stream_flags.check
}

unsafe fn decoder_get_progress(coder: *mut c_void, progress_in: *mut u64, progress_out: *mut u64) {
    (*coder.cast::<StreamDecoderMt>()).get_progress(progress_in, progress_out);
}

unsafe fn decoder_memconfig(
    coder: *mut c_void,
    memusage: *mut u64,
    old_memlimit: *mut u64,
    new_memlimit: u64,
) -> lzma_ret {
    (*coder.cast::<StreamDecoderMt>()).memconfig(memusage, old_memlimit, new_memlimit)
}

pub(crate) unsafe fn stream_decoder_mt(
    strm: *mut lzma_stream,
    options: *const lzma_mt,
) -> lzma_ret {
    if strm.is_null() {
        return LZMA_PROG_ERROR;
    }

    if let Some(options_ref) = options.as_ref() {
        if options_ref.threads == 1
            || (options_ref.flags
                & (LZMA_TELL_NO_CHECK | LZMA_TELL_UNSUPPORTED_CHECK | LZMA_TELL_ANY_CHECK))
                != 0
        {
            return upstream::stream_decoder_mt(strm, options);
        }
    }

    let coder = match StreamDecoderMt::new((*strm).allocator, options) {
        Ok(coder) => coder,
        Err(ret) => return ret,
    };

    install_next_coder(
        strm,
        NextCoder {
            coder: Box::into_raw(coder).cast(),
            code: decoder_code,
            end: Some(decoder_end),
            get_progress: Some(decoder_get_progress),
            get_check: Some(decoder_get_check),
            memconfig: Some(decoder_memconfig),
            update: None,
        },
        decoder_supported_actions(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::ffi::types::{lzma_filter, lzma_options_lzma};
    use crate::internal::{
        common::LZMA_CHECK_CRC32,
        common::LZMA_FINISH,
        container::stream_buffer,
        filter::common::LZMA_FILTER_LZMA2,
        preset,
        stream_state::{current_next_coder, lzma_code_impl, lzma_end_impl},
    };

    unsafe fn make_stream_with_input(len: usize) -> (Vec<u8>, Vec<u8>) {
        let mut input = vec![0u8; len];
        for (i, byte) in input.iter_mut().enumerate() {
            *byte = (i.wrapping_mul(17) % 251) as u8;
        }

        let mut options: lzma_options_lzma = mem::zeroed();
        assert_eq!(preset::lzma_lzma_preset_impl(&mut options, 6), 0);
        let mut filters = [
            lzma_filter {
                id: LZMA_FILTER_LZMA2,
                options: (&mut options as *mut lzma_options_lzma).cast(),
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
        ];

        let bound = stream_buffer::stream_buffer_bound(input.len());
        let mut encoded = vec![0u8; bound];
        let mut out_pos = 0usize;
        assert_eq!(
            stream_buffer::stream_buffer_encode(
                filters.as_mut_ptr(),
                LZMA_CHECK_CRC32,
                ptr::null(),
                input.as_ptr(),
                input.len(),
                encoded.as_mut_ptr(),
                &mut out_pos,
                encoded.len(),
            ),
            LZMA_OK
        );
        encoded.truncate(out_pos);
        (input, encoded)
    }

    unsafe fn make_stream() -> Vec<u8> {
        make_stream_with_input(256 * 1024).1
    }

    fn corrupt_first_block_payload_byte(encoded: &mut [u8]) {
        let payload_pos =
            LZMA_STREAM_HEADER_SIZE + ((encoded[LZMA_STREAM_HEADER_SIZE] as usize + 1) * 4);
        assert!(payload_pos < encoded.len());
        encoded[payload_pos] = 0x03;
    }

    unsafe fn decoder_from_stream(strm: *const lzma_stream) -> *const StreamDecoderMt {
        let next = current_next_coder(strm).expect("decoder should stay installed");
        next.coder.cast::<StreamDecoderMt>()
    }

    #[test]
    fn threaded_decoder_round_trip_two_threads() {
        unsafe {
            let encoded = make_stream();
            let mt = lzma_mt {
                flags: 0,
                threads: 2,
                block_size: 0,
                timeout: 0,
                preset: 0,
                filters: ptr::null(),
                check: 0,
                reserved_enum1: 0,
                reserved_enum2: 0,
                reserved_enum3: 0,
                reserved_int1: 0,
                reserved_int2: 0,
                reserved_int3: 0,
                reserved_int4: 0,
                memlimit_threading: 1 << 26,
                memlimit_stop: 1 << 26,
                reserved_int7: 0,
                reserved_int8: 0,
                reserved_ptr1: ptr::null_mut(),
                reserved_ptr2: ptr::null_mut(),
                reserved_ptr3: ptr::null_mut(),
                reserved_ptr4: ptr::null_mut(),
            };

            let mut strm = crate::ffi::types::LZMA_STREAM_INIT;
            assert_eq!(stream_decoder_mt(&mut strm, &mt), LZMA_OK);
            strm.next_in = encoded.as_ptr();
            strm.avail_in = encoded.len();

            let mut decoded = Vec::new();
            let mut finished = false;
            for _ in 0..128 {
                let mut out = vec![0u8; 16 * 1024];
                strm.next_out = out.as_mut_ptr();
                strm.avail_out = out.len();
                let ret = lzma_code_impl(&mut strm, LZMA_FINISH);
                let written = out.len() - strm.avail_out;
                decoded.extend_from_slice(&out[..written]);
                if ret == LZMA_STREAM_END {
                    assert_eq!(strm.avail_in, 0);
                    finished = true;
                    break;
                }
                assert_eq!(ret, LZMA_OK);
            }
            assert!(
                finished,
                "threaded decoder should finish without timing out"
            );
            assert!(!decoded.is_empty());
            lzma_end_impl(&mut strm);
        }
    }

    #[test]
    fn worker_error_does_not_return_worker_to_pool_or_clear_input_size() {
        unsafe {
            let mut encoded = make_stream();
            corrupt_first_block_payload_byte(&mut encoded);

            let mt = lzma_mt {
                flags: 0,
                threads: 2,
                block_size: 0,
                timeout: 0,
                preset: 0,
                filters: ptr::null(),
                check: 0,
                reserved_enum1: 0,
                reserved_enum2: 0,
                reserved_enum3: 0,
                reserved_int1: 0,
                reserved_int2: 0,
                reserved_int3: 0,
                reserved_int4: 0,
                memlimit_threading: 1 << 26,
                memlimit_stop: 1 << 26,
                reserved_int7: 0,
                reserved_int8: 0,
                reserved_ptr1: ptr::null_mut(),
                reserved_ptr2: ptr::null_mut(),
                reserved_ptr3: ptr::null_mut(),
                reserved_ptr4: ptr::null_mut(),
            };

            let mut strm = crate::ffi::types::LZMA_STREAM_INIT;
            assert_eq!(stream_decoder_mt(&mut strm, &mt), LZMA_OK);

            let mut pos = 0usize;
            let mut worker_error = LZMA_OK;
            for _ in 0..512 {
                let chunk = (encoded.len() - pos).min(64);
                let mut out = [0u8; 512];
                strm.next_in = if chunk == 0 {
                    ptr::null()
                } else {
                    encoded.as_ptr().add(pos)
                };
                strm.avail_in = chunk;
                strm.next_out = out.as_mut_ptr();
                strm.avail_out = out.len();

                let ret = lzma_code_impl(&mut strm, LZMA_RUN);
                pos += chunk - strm.avail_in;

                for _ in 0..32 {
                    let decoder = &*decoder_from_stream(&strm);
                    let shared = lock(&decoder.shared.state);
                    if shared.thread_error != LZMA_OK {
                        worker_error = shared.thread_error;
                        break;
                    }
                    drop(shared);
                    std::thread::sleep(Duration::from_millis(1));
                }

                if worker_error != LZMA_OK {
                    if ret != LZMA_OK {
                        assert_eq!(ret, LZMA_PROG_ERROR);
                    }
                    break;
                }
            }

            assert_ne!(worker_error, LZMA_OK, "worker should report an error");

            let decoder = &*decoder_from_stream(&strm);
            let shared = lock(&decoder.shared.state);
            assert_eq!(shared.thread_error, worker_error);
            assert_eq!(
                shared.free_workers.len(),
                mt.threads as usize - 1,
                "the failed worker must not be returned to the free pool",
            );
            drop(shared);

            let mut poisoned_workers = 0usize;
            for worker in &decoder.workers {
                let state = lock(&worker.shared.state);
                if state.poisoned {
                    poisoned_workers += 1;
                    assert_eq!(state.input.len(), state.in_size);
                    assert!(state.in_size > 0);
                    assert!(
                        state.in_filled < state.in_size,
                        "the main thread should still have pending input to copy",
                    );
                }
            }
            assert_eq!(poisoned_workers, 1);

            lzma_end_impl(&mut strm);
        }
    }

    #[test]
    fn wait_on_decoder_sets_pending_error_after_worker_error() {
        unsafe {
            let mt = lzma_mt {
                flags: 0,
                threads: 2,
                block_size: 0,
                timeout: 0,
                preset: 0,
                filters: ptr::null(),
                check: 0,
                reserved_enum1: 0,
                reserved_enum2: 0,
                reserved_enum3: 0,
                reserved_int1: 0,
                reserved_int2: 0,
                reserved_int3: 0,
                reserved_int4: 0,
                memlimit_threading: 1 << 26,
                memlimit_stop: 1 << 26,
                reserved_int7: 0,
                reserved_int8: 0,
                reserved_ptr1: ptr::null_mut(),
                reserved_ptr2: ptr::null_mut(),
                reserved_ptr3: ptr::null_mut(),
                reserved_ptr4: ptr::null_mut(),
            };

            let mut strm = crate::ffi::types::LZMA_STREAM_INIT;
            assert_eq!(stream_decoder_mt(&mut strm, &mt), LZMA_OK);

            let decoder = &mut *decoder_from_stream(&strm).cast_mut();
            {
                let mut shared = lock(&decoder.shared.state);
                shared.thread_error = LZMA_DATA_ERROR;
            }

            let mut can_start = false;
            let mut wait_deadline = None;
            let mut out = [0u8; 1];
            let mut out_pos = 0usize;
            assert_eq!(
                wait_on_decoder(
                    decoder,
                    false,
                    &mut wait_deadline,
                    Some(&mut can_start),
                    out.as_mut_ptr(),
                    &mut out_pos,
                    out.len(),
                ),
                LZMA_OK
            );
            assert!(can_start, "threading conditions remain favorable");
            assert_eq!(decoder.pending_error, LZMA_PROG_ERROR);

            lzma_end_impl(&mut strm);
        }
    }

    #[test]
    fn decoder_reinit_after_threaded_error_round_trips() {
        unsafe {
            let (input, encoded) = make_stream_with_input(1024 * 1024);
            let mut corrupt = encoded.clone();
            corrupt_first_block_payload_byte(&mut corrupt);

            let mt = lzma_mt {
                flags: 0,
                threads: 2,
                block_size: 0,
                timeout: 0,
                preset: 0,
                filters: ptr::null(),
                check: 0,
                reserved_enum1: 0,
                reserved_enum2: 0,
                reserved_enum3: 0,
                reserved_int1: 0,
                reserved_int2: 0,
                reserved_int3: 0,
                reserved_int4: 0,
                memlimit_threading: 1 << 28,
                memlimit_stop: 1 << 28,
                reserved_int7: 0,
                reserved_int8: 0,
                reserved_ptr1: ptr::null_mut(),
                reserved_ptr2: ptr::null_mut(),
                reserved_ptr3: ptr::null_mut(),
                reserved_ptr4: ptr::null_mut(),
            };

            let mut strm = crate::ffi::types::LZMA_STREAM_INIT;
            assert_eq!(stream_decoder_mt(&mut strm, &mt), LZMA_OK);

            let mut pos = 0usize;
            let mut saw_error = false;
            for _ in 0..16384 {
                let chunk = (corrupt.len() - pos).min(79);
                let mut out = [0u8; 4096];
                strm.next_in = if chunk == 0 {
                    ptr::null()
                } else {
                    corrupt.as_ptr().add(pos)
                };
                strm.avail_in = chunk;
                strm.next_out = out.as_mut_ptr();
                strm.avail_out = out.len();

                let ret = lzma_code_impl(&mut strm, LZMA_FINISH);
                pos += chunk - strm.avail_in;
                if ret != LZMA_OK {
                    assert_ne!(ret, LZMA_STREAM_END);
                    saw_error = true;
                    break;
                }
            }
            assert!(saw_error);

            assert_eq!(stream_decoder_mt(&mut strm, &mt), LZMA_OK);
            strm.next_in = encoded.as_ptr();
            strm.avail_in = encoded.len();

            let mut decoded = Vec::new();
            let mut finished = false;
            for _ in 0..256 {
                let mut out = vec![0u8; 16 * 1024];
                strm.next_out = out.as_mut_ptr();
                strm.avail_out = out.len();
                let ret = lzma_code_impl(&mut strm, LZMA_FINISH);
                let written = out.len() - strm.avail_out;
                decoded.extend_from_slice(&out[..written]);
                if ret == LZMA_STREAM_END {
                    finished = true;
                    break;
                }
                assert_eq!(ret, LZMA_OK);
            }

            assert!(finished);
            assert_eq!(decoded, input);
            lzma_end_impl(&mut strm);
        }
    }
}
