use std::collections::VecDeque;
use std::sync::{Arc, Mutex, MutexGuard};

use crate::ffi::types::{lzma_ret, lzma_vli, LZMA_MEM_ERROR, LZMA_OK, LZMA_STREAM_END};

const LZMA_THREADS_MAX: u32 = 16_384;

#[derive(Debug)]
pub(crate) struct OutBuf {
    pub(crate) allocated: usize,
    pub(crate) state: Mutex<OutBufState>,
}

#[derive(Debug)]
pub(crate) struct OutBufState {
    pub(crate) worker_id: Option<usize>,
    pub(crate) pos: usize,
    pub(crate) decoder_in_pos: usize,
    pub(crate) finished: bool,
    pub(crate) finish_ret: lzma_ret,
    pub(crate) unpadded_size: lzma_vli,
    pub(crate) uncompressed_size: lzma_vli,
    pub(crate) data: Vec<u8>,
}

#[derive(Debug, Default)]
pub(crate) struct OutQueue {
    active: VecDeque<Arc<OutBuf>>,
    cache: Vec<Arc<OutBuf>>,
    read_pos: usize,
    pub(crate) mem_allocated: u64,
    pub(crate) mem_in_use: u64,
    bufs_limit: u32,
}

#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct ReadResult {
    pub(crate) ret: lzma_ret,
    pub(crate) unpadded_size: lzma_vli,
    pub(crate) uncompressed_size: lzma_vli,
}

fn bufs_limit(threads: u32) -> u32 {
    threads.saturating_mul(2)
}

fn buf_memusage(size: usize) -> u64 {
    (core::mem::size_of::<OutBuf>() + size) as u64
}

fn lock_state(buf: &Arc<OutBuf>) -> MutexGuard<'_, OutBufState> {
    buf.state
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
}

impl OutBuf {
    fn new(size: usize) -> Self {
        Self {
            allocated: size,
            state: Mutex::new(OutBufState {
                worker_id: None,
                pos: 0,
                decoder_in_pos: 0,
                finished: false,
                finish_ret: LZMA_STREAM_END,
                unpadded_size: 0,
                uncompressed_size: 0,
                data: vec![0; size],
            }),
        }
    }

    pub(crate) fn reset(&self, worker_id: Option<usize>) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        state.worker_id = worker_id;
        state.pos = 0;
        state.decoder_in_pos = 0;
        state.finished = false;
        state.finish_ret = LZMA_STREAM_END;
        state.unpadded_size = 0;
        state.uncompressed_size = 0;
    }
}

impl OutQueue {
    pub(crate) fn memusage(buf_size_max: u64, threads: u32) -> u64 {
        let limit = u64::MAX / bufs_limit(LZMA_THREADS_MAX) as u64 / 2;
        if threads > LZMA_THREADS_MAX || buf_size_max > limit {
            return u64::MAX;
        }

        bufs_limit(threads) as u64 * (core::mem::size_of::<OutBuf>() as u64 + buf_size_max)
    }

    pub(crate) fn init(&mut self, threads: u32) -> Result<(), lzma_ret> {
        if threads > LZMA_THREADS_MAX {
            return Err(crate::ffi::types::LZMA_OPTIONS_ERROR);
        }

        self.end();
        self.bufs_limit = bufs_limit(threads);
        self.read_pos = 0;
        Ok(())
    }

    pub(crate) fn end(&mut self) {
        self.active.clear();
        self.cache.clear();
        self.read_pos = 0;
        self.mem_allocated = 0;
        self.mem_in_use = 0;
    }

    pub(crate) fn clear_cache(&mut self) {
        while let Some(buf) = self.cache.pop() {
            self.mem_allocated = self
                .mem_allocated
                .saturating_sub(buf_memusage(buf.allocated));
        }
    }

    pub(crate) fn clear_cache_keep(&mut self, keep_size: usize) {
        if self.cache.is_empty() {
            return;
        }

        let keep = self
            .cache
            .iter()
            .position(|buf| buf.allocated == keep_size)
            .map(|index| self.cache.swap_remove(index));
        let keep_memusage = keep
            .as_ref()
            .map(|buf| buf_memusage(buf.allocated))
            .unwrap_or(0);
        self.mem_allocated = self.mem_allocated.saturating_sub(keep_memusage);

        self.clear_cache();

        if let Some(buf) = keep {
            self.mem_allocated = self.mem_allocated.saturating_add(keep_memusage);
            self.cache.push(buf);
        }
    }

    pub(crate) fn has_buf(&self) -> bool {
        self.active.len() < self.bufs_limit as usize
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.active.is_empty()
    }

    pub(crate) fn is_readable(&self) -> bool {
        let Some(buf) = self.active.front() else {
            return false;
        };

        let state = lock_state(buf);
        self.read_pos < state.pos || state.finished
    }

    pub(crate) fn prealloc_buf(&mut self, size: usize) -> Result<(), lzma_ret> {
        debug_assert!(self.has_buf());

        if size > usize::MAX - core::mem::size_of::<OutBuf>() {
            return Err(LZMA_MEM_ERROR);
        }

        if self
            .cache
            .last()
            .map(|buf| buf.allocated == size)
            .unwrap_or(false)
        {
            return Ok(());
        }

        self.clear_cache();

        let buf = Arc::new(OutBuf::new(size));
        self.mem_allocated = self.mem_allocated.saturating_add(buf_memusage(size));
        self.cache.push(buf);
        Ok(())
    }

    pub(crate) fn get_buf(&mut self, worker_id: Option<usize>) -> Arc<OutBuf> {
        debug_assert!(self.has_buf());
        let buf = self.cache.pop().expect("prealloc_buf must succeed first");
        buf.reset(worker_id);
        self.mem_in_use = self.mem_in_use.saturating_add(buf_memusage(buf.allocated));
        self.active.push_back(buf.clone());
        buf
    }

    pub(crate) fn take_head_worker(&mut self) -> Option<usize> {
        let buf = self.active.front()?;
        let mut state = lock_state(buf);
        if state.finished {
            return None;
        }
        state.worker_id.take()
    }

    pub(crate) unsafe fn read(
        &mut self,
        out: *mut u8,
        out_pos: *mut usize,
        out_size: usize,
    ) -> ReadResult {
        let Some(buf) = self.active.front().cloned() else {
            return ReadResult {
                ret: LZMA_OK,
                ..ReadResult::default()
            };
        };

        let state = lock_state(&buf);
        let available = state.pos.saturating_sub(self.read_pos);
        let copy_size = available.min(out_size.saturating_sub(*out_pos));
        if copy_size != 0 {
            core::ptr::copy_nonoverlapping(
                state.data.as_ptr().add(self.read_pos),
                out.add(*out_pos),
                copy_size,
            );
            self.read_pos += copy_size;
            *out_pos += copy_size;
        }

        if !state.finished || self.read_pos < state.pos {
            return ReadResult {
                ret: LZMA_OK,
                ..ReadResult::default()
            };
        }

        let result = ReadResult {
            ret: state.finish_ret,
            unpadded_size: state.unpadded_size,
            uncompressed_size: state.uncompressed_size,
        };
        drop(state);

        let finished = self.active.pop_front().expect("head exists");
        self.mem_in_use = self
            .mem_in_use
            .saturating_sub(buf_memusage(finished.allocated));
        self.read_pos = 0;

        if self
            .cache
            .last()
            .map(|buf| buf.allocated != finished.allocated)
            .unwrap_or(false)
        {
            self.clear_cache();
        }

        self.cache.push(finished);
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memusage_rejects_invalid_threads() {
        assert_eq!(OutQueue::memusage(16, LZMA_THREADS_MAX + 1), u64::MAX);
    }

    #[test]
    fn queue_reuses_finished_buffers() {
        let mut outq = OutQueue::default();
        outq.init(2).unwrap();
        outq.prealloc_buf(32).unwrap();
        let buf = outq.get_buf(Some(1));
        {
            let mut state = lock_state(&buf);
            state.data[..4].copy_from_slice(b"test");
            state.pos = 4;
            state.finished = true;
        }

        let mut out = [0u8; 4];
        let mut out_pos = 0usize;
        let ret = unsafe { outq.read(out.as_mut_ptr(), &mut out_pos, out.len()) };
        assert_eq!(ret.ret, LZMA_STREAM_END);
        assert_eq!(&out, b"test");
        assert!(outq.is_empty());
        assert_eq!(outq.cache.len(), 1);
    }
}
