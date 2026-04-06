use core::ffi::c_void;
use core::{mem, ptr};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crate::ffi::types::{
    lzma_action, lzma_allocator, lzma_check, lzma_filter, lzma_mt, lzma_options_lzma, lzma_ret,
    lzma_stream, LZMA_FILTERS_MAX, LZMA_MEM_ERROR, LZMA_OK, LZMA_OPTIONS_ERROR, LZMA_PROG_ERROR,
    LZMA_STREAM_END, LZMA_UNSUPPORTED_CHECK, LZMA_VLI_UNKNOWN,
};
use crate::internal::block;
use crate::internal::check;
use crate::internal::common::{
    ACTION_COUNT, LZMA_CHECK_ID_MAX, LZMA_FINISH, LZMA_FULL_BARRIER, LZMA_FULL_FLUSH,
    LZMA_PRESET_LEVEL_MASK, LZMA_RUN, LZMA_TIMED_OUT,
};
use crate::internal::container::outqueue::{OutBuf, OutQueue, ReadResult};
use crate::internal::container::stream::copy_output_buffer;
use crate::internal::filter::common::LZMA_FILTER_LZMA2;
use crate::internal::preset;
use crate::internal::stream_flags::{
    stream_footer_encode_impl, stream_header_encode_impl, LZMA_STREAM_HEADER_SIZE,
};
use crate::internal::stream_state::{install_next_coder, lzma_end_impl, NextCoder};
use crate::internal::upstream::{self, IndexRecord};

const LZMA_THREADS_MAX: u32 = 16_384;
const BLOCK_SIZE_MAX: u64 = u64::MAX / LZMA_THREADS_MAX as u64;

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

    unsafe fn try_clone(&self) -> Result<Self, lzma_ret> {
        Self::copy_from(self.filters.as_ptr())
    }

    fn as_ptr(&self) -> *const lzma_filter {
        self.filters.as_ptr()
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

enum ResolvedFilters {
    Borrowed(*const lzma_filter),
    Preset {
        _options: Box<lzma_options_lzma>,
        filters: FilterArray,
    },
}

impl ResolvedFilters {
    fn as_ptr(&self) -> *const lzma_filter {
        match self {
            Self::Borrowed(ptr) => *ptr,
            Self::Preset { filters, .. } => filters.as_ptr(),
        }
    }
}

#[derive(Copy, Clone, Eq, PartialEq)]
enum EncoderSequence {
    Header,
    Block,
    Index,
    Footer,
}

#[derive(Copy, Clone, Eq, PartialEq)]
enum EncoderCommand {
    Idle,
    Run,
    Finish,
    Exit,
}

struct EncoderWorkerState {
    command: EncoderCommand,
    input: Vec<u8>,
    in_size: usize,
    outbuf: Option<Arc<OutBuf>>,
    filters: Option<OwnedFilters>,
    progress_in: u64,
    progress_out: u64,
}

struct EncoderWorkerShared {
    state: Mutex<EncoderWorkerState>,
    cond: Condvar,
}

struct EncoderWorker {
    shared: Arc<EncoderWorkerShared>,
    handle: Option<JoinHandle<()>>,
}

struct EncoderSharedState {
    outq: OutQueue,
    free_workers: Vec<usize>,
    thread_error: lzma_ret,
    progress_in: u64,
    progress_out: u64,
}

struct EncoderShared {
    state: Mutex<EncoderSharedState>,
    cond: Condvar,
}

struct StreamEncoderMt {
    sequence: EncoderSequence,
    block_size: usize,
    outbuf_alloc_size: usize,
    timeout: u32,
    check: lzma_check,
    filters: OwnedFilters,
    current_worker: Option<usize>,
    header: [u8; LZMA_STREAM_HEADER_SIZE],
    header_pos: usize,
    index: Vec<IndexRecord>,
    index_bytes: Vec<u8>,
    index_pos: usize,
    footer: [u8; LZMA_STREAM_HEADER_SIZE],
    footer_pos: usize,
    shared: Arc<EncoderShared>,
    workers: Vec<EncoderWorker>,
}

fn supported_actions() -> [bool; ACTION_COUNT] {
    let mut actions = [false; ACTION_COUNT];
    actions[LZMA_RUN as usize] = true;
    actions[LZMA_FULL_FLUSH as usize] = true;
    actions[LZMA_FULL_BARRIER as usize] = true;
    actions[LZMA_FINISH as usize] = true;
    actions
}

unsafe fn default_block_size(filters: *const lzma_filter) -> u64 {
    let mut max_size = 0u64;
    let mut index = 0usize;
    loop {
        let filter = *filters.add(index);
        if filter.id == LZMA_VLI_UNKNOWN {
            return max_size;
        }

        if filter.id == LZMA_FILTER_LZMA2 {
            if filter.options.is_null() {
                return 0;
            }

            let options = &*filter.options.cast::<lzma_options_lzma>();
            let size = u64::from(options.dict_size).saturating_mul(3).max(1 << 20);
            max_size = max_size.max(size);
        }

        index += 1;
        if index > LZMA_FILTERS_MAX {
            return 0;
        }
    }
}

unsafe fn resolve_filters(options: &lzma_mt) -> Result<ResolvedFilters, lzma_ret> {
    if !options.filters.is_null() {
        return Ok(ResolvedFilters::Borrowed(options.filters));
    }

    let mut opt = Box::new(mem::zeroed::<lzma_options_lzma>());
    if preset::lzma_lzma_preset_impl(
        (&mut *opt) as *mut _,
        options.preset & LZMA_PRESET_LEVEL_MASK,
    ) != 0
    {
        return Err(LZMA_OPTIONS_ERROR);
    }

    let filters = [
        lzma_filter {
            id: LZMA_FILTER_LZMA2,
            options: (&mut *opt as *mut lzma_options_lzma).cast(),
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

    Ok(ResolvedFilters::Preset {
        _options: opt,
        filters,
    })
}

unsafe fn get_options(
    options: *const lzma_mt,
) -> Result<(OwnedFilters, usize, usize, lzma_check), lzma_ret> {
    if options.is_null() {
        return Err(LZMA_PROG_ERROR);
    }
    let options = &*options;

    if options.flags != 0 || options.threads == 0 || options.threads > LZMA_THREADS_MAX {
        return Err(LZMA_OPTIONS_ERROR);
    }

    if options.check < 0 || options.check as usize > LZMA_CHECK_ID_MAX {
        return Err(LZMA_PROG_ERROR);
    }
    if check::check_is_supported(options.check) == 0 {
        return Err(LZMA_UNSUPPORTED_CHECK);
    }

    let resolved = resolve_filters(options)?;
    let filters_ptr = resolved.as_ptr();
    if crate::internal::lzma::encoder_memusage(filters_ptr) == u64::MAX {
        return Err(LZMA_OPTIONS_ERROR);
    }

    let block_size = if options.block_size != 0 {
        if options.block_size > BLOCK_SIZE_MAX {
            return Err(LZMA_OPTIONS_ERROR);
        }
        options.block_size
    } else {
        let size = default_block_size(filters_ptr);
        if size == 0 {
            return Err(LZMA_OPTIONS_ERROR);
        }
        size
    };

    let outbuf_size = unsafe { block::block_buffer_bound(block_size as usize) };
    if outbuf_size == 0 {
        return Err(LZMA_MEM_ERROR);
    }

    let filters = OwnedFilters::copy_from(filters_ptr)?;
    Ok((filters, block_size as usize, outbuf_size, options.check))
}

fn wait_on_encoder(
    shared: &EncoderShared,
    timeout: u32,
    deadline: &mut Option<Instant>,
    has_input: bool,
) -> bool {
    let mut guard = shared
        .state
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());

    let needs_wait = |state: &EncoderSharedState| {
        (!has_input || state.free_workers.is_empty() || !state.outq.has_buf())
            && !state.outq.is_readable()
            && state.thread_error == LZMA_OK
    };

    if timeout != 0 && deadline.is_none() {
        *deadline = Some(Instant::now() + Duration::from_millis(timeout as u64));
    }

    while needs_wait(&guard) {
        if timeout == 0 {
            guard = shared
                .cond
                .wait(guard)
                .unwrap_or_else(|poison| poison.into_inner());
            continue;
        }

        let Some(end) = *deadline else {
            break;
        };

        let now = Instant::now();
        if now >= end {
            return true;
        }

        let wait = end.saturating_duration_since(now);
        let (next_guard, result) = shared
            .cond
            .wait_timeout(guard, wait)
            .unwrap_or_else(|poison| poison.into_inner());
        guard = next_guard;
        if result.timed_out() && needs_wait(&guard) {
            return true;
        }
    }

    false
}

fn encoder_worker_loop(
    worker_id: usize,
    allocator: AllocatorPtr,
    block_size: usize,
    outbuf_alloc_size: usize,
    check: lzma_check,
    shared: Arc<EncoderShared>,
    worker: Arc<EncoderWorkerShared>,
) {
    const INPUT_CHUNK_MAX: usize = 16 * 1024;

    loop {
        let (mut filters, outbuf, mut command) = {
            let mut state = worker
                .state
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            while state.command == EncoderCommand::Idle {
                state = worker
                    .cond
                    .wait(state)
                    .unwrap_or_else(|poison| poison.into_inner());
            }

            if state.command == EncoderCommand::Exit {
                break;
            }

            (
                state.filters.take().expect("worker filters missing"),
                state.outbuf.clone().expect("worker outbuf missing"),
                state.command,
            )
        };

        let mut block_opts = unsafe { mem::zeroed::<crate::ffi::types::lzma_block>() };
        block_opts.version = 0;
        block_opts.check = check;
        block_opts.compressed_size = outbuf_alloc_size as u64;
        block_opts.uncompressed_size = block_size as u64;
        block_opts.filters = filters.as_mut_ptr();

        let mut job_ret = unsafe { block::block_header_size(&mut block_opts) };
        let mut out_pos = block_opts.header_size as usize;
        let mut consumed = 0usize;

        let mut inner = crate::ffi::types::LZMA_STREAM_INIT;
        inner.allocator = allocator.0;
        if job_ret == LZMA_OK {
            job_ret = unsafe { block::block_encoder(&mut inner, &mut block_opts) };
        }

        if job_ret == LZMA_OK {
            loop {
                let (chunk, snapshot_in_size, snapshot_cmd) = {
                    let mut state = worker
                        .state
                        .lock()
                        .unwrap_or_else(|poison| poison.into_inner());
                    state.progress_in = consumed as u64;
                    state.progress_out = out_pos as u64;

                    while state.command == EncoderCommand::Run && state.in_size == consumed {
                        state = worker
                            .cond
                            .wait(state)
                            .unwrap_or_else(|poison| poison.into_inner());
                    }

                    if state.command == EncoderCommand::Exit {
                        command = EncoderCommand::Exit;
                        (Vec::new(), state.in_size, state.command)
                    } else {
                        let available = state.in_size.saturating_sub(consumed);
                        let take = available.min(INPUT_CHUNK_MAX);
                        (
                            state.input[consumed..consumed + take].to_vec(),
                            state.in_size,
                            state.command,
                        )
                    }
                };

                if snapshot_cmd == EncoderCommand::Exit {
                    break;
                }

                let action = if snapshot_cmd == EncoderCommand::Finish
                    && consumed + chunk.len() == snapshot_in_size
                {
                    LZMA_FINISH
                } else {
                    LZMA_RUN
                };

                let (ret, in_pos) = unsafe {
                    let mut out_state = lock(&outbuf.state);
                    let out_avail = out_state.data.len().saturating_sub(out_pos);
                    inner.next_in = if chunk.is_empty() {
                        ptr::null()
                    } else {
                        chunk.as_ptr()
                    };
                    inner.avail_in = chunk.len();
                    inner.next_out = out_state.data.as_mut_ptr().wrapping_add(out_pos);
                    inner.avail_out = out_avail;
                    let ret = crate::internal::stream_state::lzma_code_impl(&mut inner, action);
                    let in_pos = chunk.len().saturating_sub(inner.avail_in);
                    let out_used = out_avail.saturating_sub(inner.avail_out);
                    out_pos += out_used;
                    (ret, in_pos)
                };

                consumed += in_pos;

                match ret {
                    LZMA_STREAM_END => {
                        let ret = unsafe {
                            let mut out_state = lock(&outbuf.state);
                            let ret = block::block_header_encode(
                                &block_opts,
                                out_state.data.as_mut_ptr(),
                            );
                            out_state.unpadded_size = block::block_unpadded_size(&block_opts);
                            out_state.uncompressed_size = block_opts.uncompressed_size;
                            ret
                        };
                        job_ret = ret;
                        break;
                    }
                    LZMA_OK => {
                        if outbuf.allocated == out_pos {
                            let final_in_size = {
                                let mut state = worker
                                    .state
                                    .lock()
                                    .unwrap_or_else(|poison| poison.into_inner());
                                while state.command == EncoderCommand::Run {
                                    state = worker
                                        .cond
                                        .wait(state)
                                        .unwrap_or_else(|poison| poison.into_inner());
                                }
                                if state.command == EncoderCommand::Exit {
                                    command = EncoderCommand::Exit;
                                    0
                                } else {
                                    state.in_size
                                }
                            };

                            if command != EncoderCommand::Exit {
                                let ret = unsafe {
                                    let state = lock(&worker.state);
                                    let mut out_state = lock(&outbuf.state);
                                    out_pos = 0;
                                    block::block_uncomp_encode(
                                        &mut block_opts,
                                        if final_in_size == 0 {
                                            ptr::null()
                                        } else {
                                            state.input.as_ptr()
                                        },
                                        final_in_size,
                                        out_state.data.as_mut_ptr(),
                                        &mut out_pos,
                                        out_state.data.len(),
                                    )
                                };

                                if ret == LZMA_OK {
                                    let mut out_state = lock(&outbuf.state);
                                    out_state.unpadded_size =
                                        unsafe { block::block_unpadded_size(&block_opts) };
                                    out_state.uncompressed_size = block_opts.uncompressed_size;
                                }

                                job_ret = ret;
                            }
                            break;
                        }
                    }
                    other => {
                        job_ret = other;
                        break;
                    }
                }
            }
        }

        unsafe { lzma_end_impl(&mut inner) };

        if command != EncoderCommand::Exit && job_ret == LZMA_OK {
            let mut out_state = lock(&outbuf.state);
            out_state.pos = out_pos;
            out_state.finished = true;
            out_state.finish_ret = LZMA_STREAM_END;
            drop(out_state);

            let mut shared_state = shared
                .state
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            shared_state.progress_in = shared_state
                .progress_in
                .saturating_add(block_opts.uncompressed_size);
            shared_state.progress_out = shared_state.progress_out.saturating_add(out_pos as u64);
            shared_state.free_workers.push(worker_id);
            drop(shared_state);
            shared.cond.notify_all();
        } else if job_ret != LZMA_OK {
            let mut shared_state = shared
                .state
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            if shared_state.thread_error == LZMA_OK {
                shared_state.thread_error = job_ret;
            }
            drop(shared_state);
            shared.cond.notify_all();
        }

        let mut state = worker
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        state.progress_in = 0;
        state.progress_out = 0;
        state.in_size = 0;
        state.outbuf = None;
        state.filters = None;
        state.command = if command == EncoderCommand::Exit {
            EncoderCommand::Exit
        } else {
            EncoderCommand::Idle
        };
    }
}

impl StreamEncoderMt {
    unsafe fn new(
        allocator: *const lzma_allocator,
        options: *const lzma_mt,
    ) -> Result<Box<Self>, lzma_ret> {
        let options_ref = &*options;
        let (filters, block_size, outbuf_alloc_size, check_id) = get_options(options)?;

        let mut header = [0u8; LZMA_STREAM_HEADER_SIZE];
        let stream_flags = crate::ffi::types::lzma_stream_flags {
            version: 0,
            backward_size: 0,
            check: check_id,
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
        if stream_header_encode_impl(&stream_flags, header.as_mut_ptr()) != LZMA_OK {
            return Err(LZMA_PROG_ERROR);
        }

        let shared = Arc::new(EncoderShared {
            state: Mutex::new(EncoderSharedState {
                outq: OutQueue::default(),
                free_workers: Vec::new(),
                thread_error: LZMA_OK,
                progress_in: 0,
                progress_out: LZMA_STREAM_HEADER_SIZE as u64,
            }),
            cond: Condvar::new(),
        });
        lock(&shared.state).outq.init(options_ref.threads)?;

        let mut workers = Vec::with_capacity(options_ref.threads as usize);
        for worker_id in 0..options_ref.threads as usize {
            let worker_shared = Arc::new(EncoderWorkerShared {
                state: Mutex::new(EncoderWorkerState {
                    command: EncoderCommand::Idle,
                    input: vec![0; block_size],
                    in_size: 0,
                    outbuf: None,
                    filters: None,
                    progress_in: 0,
                    progress_out: 0,
                }),
                cond: Condvar::new(),
            });

            let handle = thread::Builder::new()
                .name(format!("lzma-enc-mt-{worker_id}"))
                .spawn({
                    let shared = shared.clone();
                    let worker = worker_shared.clone();
                    let allocator = AllocatorPtr(allocator);
                    move || {
                        encoder_worker_loop(
                            worker_id,
                            allocator,
                            block_size,
                            outbuf_alloc_size,
                            check_id,
                            shared,
                            worker,
                        );
                    }
                })
                .map_err(|_| LZMA_MEM_ERROR)?;

            workers.push(EncoderWorker {
                shared: worker_shared,
                handle: Some(handle),
            });
            lock(&shared.state).free_workers.push(worker_id);
        }

        Ok(Box::new(Self {
            sequence: EncoderSequence::Header,
            block_size,
            outbuf_alloc_size,
            timeout: options_ref.timeout,
            check: check_id,
            filters,
            current_worker: None,
            header,
            header_pos: 0,
            index: Vec::new(),
            index_bytes: Vec::new(),
            index_pos: 0,
            footer: [0; LZMA_STREAM_HEADER_SIZE],
            footer_pos: 0,
            shared,
            workers,
        }))
    }

    unsafe fn read_ready_output(
        &mut self,
        out: *mut u8,
        out_pos: *mut usize,
        out_size: usize,
    ) -> ReadResult {
        let result = {
            let mut state = lock(&self.shared.state);
            state.outq.read(out, out_pos, out_size)
        };

        if result.ret == LZMA_STREAM_END {
            self.index.push(IndexRecord {
                unpadded_size: result.unpadded_size,
                uncompressed_size: result.uncompressed_size,
            });
        }

        result
    }

    unsafe fn start_worker(&mut self) -> Result<bool, lzma_ret> {
        let (worker_id, outbuf) = {
            let mut shared = lock(&self.shared.state);
            if !shared.outq.has_buf() || shared.free_workers.is_empty() {
                return Ok(false);
            }

            shared.outq.prealloc_buf(self.outbuf_alloc_size)?;
            let worker_id = shared.free_workers.pop().expect("free worker");
            let outbuf = shared.outq.get_buf(None);
            (worker_id, outbuf)
        };

        let filters = self.filters.try_clone()?;
        let worker = &self.workers[worker_id];
        let mut state = lock(&worker.shared.state);
        state.in_size = 0;
        if state.input.len() != self.block_size {
            state.input.resize(self.block_size, 0);
        }
        state.outbuf = Some(outbuf);
        state.filters = Some(filters);
        state.command = EncoderCommand::Run;
        worker.shared.cond.notify_all();
        self.current_worker = Some(worker_id);
        Ok(true)
    }

    unsafe fn stream_encode_in(
        &mut self,
        input: *const u8,
        in_pos: *mut usize,
        in_size: usize,
        action: lzma_action,
    ) -> lzma_ret {
        while *in_pos < in_size || (self.current_worker.is_some() && action != LZMA_RUN) {
            if self.current_worker.is_none() {
                match self.start_worker() {
                    Ok(true) => {}
                    Ok(false) => return LZMA_OK,
                    Err(ret) => return ret,
                }
            }

            let worker_id = self.current_worker.expect("current worker");
            let worker = &self.workers[worker_id];
            let mut state = lock(&worker.shared.state);
            let available = self.block_size.saturating_sub(state.in_size);
            let copy_size = available.min(in_size.saturating_sub(*in_pos));
            if copy_size != 0 {
                ptr::copy_nonoverlapping(
                    input.add(*in_pos),
                    state.input.as_mut_ptr().add(state.in_size),
                    copy_size,
                );
                state.in_size += copy_size;
                *in_pos += copy_size;
            }

            let finish =
                state.in_size == self.block_size || (*in_pos == in_size && action != LZMA_RUN);
            if finish {
                state.command = EncoderCommand::Finish;
                self.current_worker = None;
            }

            worker.shared.cond.notify_all();
        }

        LZMA_OK
    }

    unsafe fn begin_index(&mut self) -> lzma_ret {
        self.index_bytes = upstream::encode_xz_index(&self.index);
        self.index_pos = 0;

        let backward_size = self.index_bytes.len() as u64;
        let stream_flags = crate::ffi::types::lzma_stream_flags {
            version: 0,
            backward_size,
            check: self.check,
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

        if stream_footer_encode_impl(&stream_flags, self.footer.as_mut_ptr()) != LZMA_OK {
            return LZMA_PROG_ERROR;
        }
        self.footer_pos = 0;
        self.sequence = EncoderSequence::Index;
        let mut shared = lock(&self.shared.state);
        shared.progress_out = shared
            .progress_out
            .saturating_add(self.index_bytes.len() as u64 + LZMA_STREAM_HEADER_SIZE as u64);
        LZMA_OK
    }

    unsafe fn code(
        &mut self,
        input: *const u8,
        in_pos: *mut usize,
        in_size: usize,
        output: *mut u8,
        out_pos: *mut usize,
        out_size: usize,
        action: lzma_action,
    ) -> lzma_ret {
        match self.sequence {
            EncoderSequence::Header => {
                let ret = copy_output_buffer(
                    &self.header,
                    &mut self.header_pos,
                    output,
                    out_pos,
                    out_size,
                );
                if ret != LZMA_STREAM_END {
                    return ret;
                }
                self.sequence = EncoderSequence::Block;
            }
            EncoderSequence::Block | EncoderSequence::Index | EncoderSequence::Footer => {}
        }

        if self.sequence == EncoderSequence::Block {
            let mut deadline = None;

            loop {
                let ret = self.read_ready_output(output, out_pos, out_size);
                if ret.ret == LZMA_STREAM_END && *out_pos < out_size {
                    continue;
                }
                if ret.ret != LZMA_OK && ret.ret != LZMA_STREAM_END {
                    return ret.ret;
                }

                let thread_error = lock(&self.shared.state).thread_error;
                if thread_error != LZMA_OK {
                    return thread_error;
                }

                let ret = self.stream_encode_in(input, in_pos, in_size, action);
                if ret != LZMA_OK {
                    return ret;
                }

                if *in_pos == in_size {
                    if action == LZMA_RUN {
                        return LZMA_OK;
                    }
                    if action == LZMA_FULL_BARRIER {
                        return LZMA_STREAM_END;
                    }
                    if lock(&self.shared.state).outq.is_empty() {
                        if action == LZMA_FULL_FLUSH {
                            return LZMA_STREAM_END;
                        }
                        if action == LZMA_FINISH {
                            let ret = self.begin_index();
                            if ret != LZMA_OK {
                                return ret;
                            }
                            break;
                        }
                    }
                }

                if *out_pos == out_size {
                    return LZMA_OK;
                }

                if wait_on_encoder(&self.shared, self.timeout, &mut deadline, *in_pos < in_size) {
                    return LZMA_TIMED_OUT;
                }
            }
        }

        if self.sequence == EncoderSequence::Index {
            let ret = copy_output_buffer(
                &self.index_bytes,
                &mut self.index_pos,
                output,
                out_pos,
                out_size,
            );
            if ret != LZMA_STREAM_END {
                return ret;
            }
            self.sequence = EncoderSequence::Footer;
        }

        debug_assert!(self.sequence == EncoderSequence::Footer);
        copy_output_buffer(
            &self.footer,
            &mut self.footer_pos,
            output,
            out_pos,
            out_size,
        )
    }

    unsafe fn update_filters(&mut self, filters: *const lzma_filter) -> lzma_ret {
        if self.sequence != EncoderSequence::Block || self.current_worker.is_some() {
            return LZMA_PROG_ERROR;
        }

        match OwnedFilters::copy_from(filters) {
            Ok(filters) => {
                self.filters = filters;
                LZMA_OK
            }
            Err(ret) => ret,
        }
    }

    unsafe fn get_progress(&self, progress_in: *mut u64, progress_out: *mut u64) {
        let shared = lock(&self.shared.state);
        let mut in_total = shared.progress_in;
        let mut out_total = shared.progress_out;

        for worker in &self.workers {
            let state = lock(&worker.shared.state);
            in_total = in_total.saturating_add(state.progress_in);
            out_total = out_total.saturating_add(state.progress_out);
        }
        drop(shared);

        *progress_in = in_total;
        *progress_out = out_total;
    }
}

unsafe fn encoder_code(
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
    (*coder.cast::<StreamEncoderMt>())
        .code(input, in_pos, in_size, output, out_pos, out_size, action)
}

unsafe fn encoder_end(coder: *mut c_void, _allocator: *const lzma_allocator) {
    let mut coder = Box::from_raw(coder.cast::<StreamEncoderMt>());
    for worker in &coder.workers {
        let mut state = lock(&worker.shared.state);
        state.command = EncoderCommand::Exit;
        worker.shared.cond.notify_all();
    }

    for worker in &mut coder.workers {
        if let Some(handle) = worker.handle.take() {
            let _ = handle.join();
        }
    }

    lock(&coder.shared.state).outq.end();
}

unsafe fn encoder_get_progress(coder: *mut c_void, progress_in: *mut u64, progress_out: *mut u64) {
    (*coder.cast::<StreamEncoderMt>()).get_progress(progress_in, progress_out);
}

unsafe fn encoder_get_check(coder: *const c_void) -> lzma_check {
    (*(coder.cast::<StreamEncoderMt>())).check
}

unsafe fn encoder_update(
    coder: *mut c_void,
    _allocator: *const lzma_allocator,
    filters: *const lzma_filter,
) -> lzma_ret {
    (*coder.cast::<StreamEncoderMt>()).update_filters(filters)
}

pub(crate) unsafe fn stream_encoder_mt_memusage(options: *const lzma_mt) -> u64 {
    let options = match options.as_ref() {
        Some(options) => options,
        None => return u64::MAX,
    };

    if options.flags != 0 || options.threads == 0 || options.threads > LZMA_THREADS_MAX {
        return u64::MAX;
    }

    if options.check < 0 || options.check as usize > LZMA_CHECK_ID_MAX {
        return u64::MAX;
    }
    if check::check_is_supported(options.check) == 0 {
        return u64::MAX;
    }

    let resolved = match resolve_filters(options) {
        Ok(filters) => filters,
        Err(_) => return u64::MAX,
    };
    let filters_ptr = resolved.as_ptr();

    let block_size = if options.block_size != 0 {
        if options.block_size > BLOCK_SIZE_MAX {
            return u64::MAX;
        }
        options.block_size
    } else {
        let size = default_block_size(filters_ptr);
        if size == 0 {
            return u64::MAX;
        }
        size
    };

    let outbuf_size = block::block_buffer_bound(block_size as usize);
    if outbuf_size == 0 {
        return u64::MAX;
    }

    let filters_memusage = crate::internal::lzma::encoder_memusage(filters_ptr);
    if filters_memusage == u64::MAX {
        return u64::MAX;
    }

    let inbuf_memusage = block_size.saturating_mul(options.threads as u64);
    let outq_memusage = OutQueue::memusage(outbuf_size as u64, options.threads);
    if outq_memusage == u64::MAX {
        return u64::MAX;
    }

    crate::internal::lzma::LZMA_MEMUSAGE_BASE
        .saturating_add(core::mem::size_of::<StreamEncoderMt>() as u64)
        .saturating_add(core::mem::size_of::<EncoderWorker>() as u64 * options.threads as u64)
        .saturating_add(inbuf_memusage)
        .saturating_add(filters_memusage.saturating_mul(options.threads as u64))
        .saturating_add(outq_memusage)
}

pub(crate) unsafe fn stream_encoder_mt(
    strm: *mut lzma_stream,
    options: *const lzma_mt,
) -> lzma_ret {
    if strm.is_null() {
        return LZMA_PROG_ERROR;
    }

    let coder = match StreamEncoderMt::new((*strm).allocator, options) {
        Ok(coder) => coder,
        Err(ret) => return ret,
    };
    install_next_coder(
        strm,
        NextCoder {
            coder: Box::into_raw(coder).cast(),
            code: encoder_code,
            end: Some(encoder_end),
            get_progress: Some(encoder_get_progress),
            get_check: Some(encoder_get_check),
            memconfig: None,
            update: Some(encoder_update),
        },
        supported_actions(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ffi::types::{lzma_filter, LZMA_STREAM_INIT};
    use crate::internal::{
        common::LZMA_CHECK_CRC32, container::stream, filter::common::LZMA_FILTER_LZMA2,
        stream_state::lzma_code_impl,
    };

    unsafe fn collect(
        strm: &mut lzma_stream,
        action: lzma_action,
        chunk: usize,
    ) -> (lzma_ret, Vec<u8>) {
        let mut output = Vec::new();
        loop {
            let mut buffer = vec![0u8; chunk];
            strm.next_out = buffer.as_mut_ptr();
            strm.avail_out = buffer.len();
            let ret = lzma_code_impl(strm, action);
            let written = buffer.len() - strm.avail_out;
            output.extend_from_slice(&buffer[..written]);
            if ret != LZMA_OK {
                return (ret, output);
            }
        }
    }

    unsafe fn filter_update_round_trips(flush_action: lzma_action) {
        let mut input = vec![0u8; 96 * 1024];
        for (i, byte) in input.iter_mut().enumerate() {
            *byte = (i % 251) as u8;
        }

        let mut opt1: lzma_options_lzma = mem::zeroed();
        let mut opt2: lzma_options_lzma = mem::zeroed();
        assert_eq!(preset::lzma_lzma_preset_impl(&mut opt1, 1), 0);
        assert_eq!(preset::lzma_lzma_preset_impl(&mut opt2, 6), 0);

        let filters1 = [
            lzma_filter {
                id: LZMA_FILTER_LZMA2,
                options: (&mut opt1 as *mut lzma_options_lzma).cast(),
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
        let filters2 = [
            lzma_filter {
                id: LZMA_FILTER_LZMA2,
                options: (&mut opt2 as *mut lzma_options_lzma).cast(),
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

        let mt = lzma_mt {
            flags: 0,
            threads: 2,
            block_size: 32 * 1024,
            timeout: 0,
            preset: 0,
            filters: filters1.as_ptr(),
            check: LZMA_CHECK_CRC32,
            reserved_enum1: 0,
            reserved_enum2: 0,
            reserved_enum3: 0,
            reserved_int1: 0,
            reserved_int2: 0,
            reserved_int3: 0,
            reserved_int4: 0,
            memlimit_threading: 0,
            memlimit_stop: 0,
            reserved_int7: 0,
            reserved_int8: 0,
            reserved_ptr1: ptr::null_mut(),
            reserved_ptr2: ptr::null_mut(),
            reserved_ptr3: ptr::null_mut(),
            reserved_ptr4: ptr::null_mut(),
        };

        let split = input.len() / 2;
        let mut enc = LZMA_STREAM_INIT;
        assert_eq!(stream_encoder_mt(&mut enc, &mt), LZMA_OK);

        enc.next_in = input.as_ptr();
        enc.avail_in = split;
        let (ret, mut encoded) = collect(&mut enc, flush_action, 4096);
        assert_eq!(ret, LZMA_STREAM_END);

        assert_eq!(stream::filters_update(&mut enc, filters2.as_ptr()), LZMA_OK);

        enc.next_in = input.as_ptr().add(split);
        enc.avail_in = input.len() - split;
        let (ret, tail) = collect(&mut enc, LZMA_FINISH, 4096);
        assert_eq!(ret, LZMA_STREAM_END);
        encoded.extend_from_slice(&tail);
        lzma_end_impl(&mut enc);

        let mut dec = LZMA_STREAM_INIT;
        assert_eq!(stream::stream_decoder(&mut dec, u64::MAX, 0), LZMA_OK);
        dec.next_in = encoded.as_ptr();
        dec.avail_in = encoded.len();
        let (ret, decoded) = collect(&mut dec, LZMA_FINISH, 4096);
        assert_eq!(ret, LZMA_STREAM_END);
        assert_eq!(decoded, input);
        lzma_end_impl(&mut dec);
    }

    #[test]
    fn filter_update_after_barrier_round_trips() {
        unsafe { filter_update_round_trips(LZMA_FULL_BARRIER) }
    }

    #[test]
    fn filter_update_after_flush_round_trips() {
        unsafe { filter_update_round_trips(LZMA_FULL_FLUSH) }
    }
}
