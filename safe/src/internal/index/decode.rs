use core::ffi::c_void;
use core::mem::size_of;
use core::ptr;

use crate::ffi::types::{
    lzma_action, lzma_allocator, lzma_index, lzma_ret, lzma_stream, lzma_vli, LZMA_DATA_ERROR,
    LZMA_MEMLIMIT_ERROR, LZMA_MEM_ERROR, LZMA_OK, LZMA_PROG_ERROR, LZMA_STREAM_END,
};
use crate::internal::check::crc32;
use crate::internal::common::{default_supported_actions, lzma_alloc, lzma_free};
use crate::internal::index::core::{
    destroy_index, index_append, index_init, index_memusage, index_mut, index_padding_size_of,
    index_prealloc, index_ref, INDEX_INDICATOR, UNPADDED_SIZE_MAX, UNPADDED_SIZE_MIN,
};
use crate::internal::stream_state::{install_next_coder, NextCoder};
use crate::internal::vli::lzma_vli_decode_impl;

const SEQ_INDICATOR: u32 = 0;
const SEQ_COUNT: u32 = 1;
const SEQ_MEMUSAGE: u32 = 2;
const SEQ_UNPADDED: u32 = 3;
const SEQ_UNCOMPRESSED: u32 = 4;
const SEQ_PADDING_INIT: u32 = 5;
const SEQ_PADDING: u32 = 6;
const SEQ_CRC32: u32 = 7;

pub(crate) struct IndexDecoderState {
    sequence: u32,
    memlimit: u64,
    index: *mut lzma_index,
    count: lzma_vli,
    unpadded_size: lzma_vli,
    uncompressed_size: lzma_vli,
    pos: usize,
    crc32: u32,
}

pub(crate) struct IndexDecoderStream {
    pub(crate) state: IndexDecoderState,
    pub(crate) dest_index: *mut *mut lzma_index,
}

impl IndexDecoderState {
    pub(crate) unsafe fn new(
        allocator: *const lzma_allocator,
        memlimit: u64,
    ) -> Result<Self, lzma_ret> {
        let index = index_init(allocator);
        if index.is_null() {
            return Err(LZMA_MEM_ERROR);
        }

        Ok(Self {
            sequence: SEQ_INDICATOR,
            memlimit: memlimit.max(1),
            index,
            count: 0,
            unpadded_size: 0,
            uncompressed_size: 0,
            pos: 0,
            crc32: 0,
        })
    }

    pub(crate) unsafe fn end(&mut self, allocator: *const lzma_allocator) {
        destroy_index(self.index, allocator);
        self.index = ptr::null_mut();
    }

    pub(crate) fn memconfig(
        &mut self,
        memusage: *mut u64,
        old_memlimit: *mut u64,
        new_memlimit: u64,
    ) -> lzma_ret {
        unsafe {
            *memusage = index_memusage(1, self.count);
            *old_memlimit = self.memlimit;
        }

        if new_memlimit != 0 {
            unsafe {
                if new_memlimit < *memusage {
                    return LZMA_MEMLIMIT_ERROR;
                }
            }
            self.memlimit = new_memlimit;
        }

        LZMA_OK
    }

    pub(crate) fn required_memusage(&self) -> u64 {
        index_memusage(1, self.count)
    }

    pub(crate) fn take_index(&mut self) -> *mut lzma_index {
        let index = self.index;
        self.index = ptr::null_mut();
        index
    }

    pub(crate) unsafe fn step(
        &mut self,
        allocator: *const lzma_allocator,
        input: *const u8,
        in_pos: *mut usize,
        in_size: usize,
    ) -> lzma_ret {
        while *in_pos < in_size {
            match self.sequence {
                SEQ_INDICATOR => {
                    if *input.add(*in_pos) != INDEX_INDICATOR {
                        return LZMA_DATA_ERROR;
                    }
                    self.crc32 = crc32::crc32(core::slice::from_raw_parts(input.add(*in_pos), 1), self.crc32);
                    *in_pos += 1;
                    self.sequence = SEQ_COUNT;
                }
                SEQ_COUNT => {
                    let start = *in_pos;
                    let ret =
                        lzma_vli_decode_impl(&mut self.count, &mut self.pos, input, in_pos, in_size);
                    if *in_pos > start {
                        self.crc32 = crc32::crc32(
                            core::slice::from_raw_parts(input.add(start), *in_pos - start),
                            self.crc32,
                        );
                    }
                    if ret != LZMA_STREAM_END {
                        return ret;
                    }

                    self.pos = 0;
                    self.sequence = SEQ_MEMUSAGE;
                }
                SEQ_MEMUSAGE => {
                    if self.required_memusage() > self.memlimit {
                        return LZMA_MEMLIMIT_ERROR;
                    }

                    index_prealloc(index_mut(self.index), self.count);
                self.sequence = if self.count == 0 {
                    SEQ_PADDING_INIT
                } else {
                    SEQ_UNPADDED
                };
                }
                SEQ_UNPADDED | SEQ_UNCOMPRESSED => {
                    let start = *in_pos;
                    let target = if self.sequence == SEQ_UNPADDED {
                        &mut self.unpadded_size
                    } else {
                        &mut self.uncompressed_size
                    };
                    let ret = lzma_vli_decode_impl(target, &mut self.pos, input, in_pos, in_size);
                    if *in_pos > start {
                        self.crc32 = crc32::crc32(
                            core::slice::from_raw_parts(input.add(start), *in_pos - start),
                            self.crc32,
                        );
                    }
                    if ret != LZMA_STREAM_END {
                        return ret;
                    }

                    self.pos = 0;
                    if self.sequence == SEQ_UNPADDED {
                        if self.unpadded_size < UNPADDED_SIZE_MIN
                            || self.unpadded_size > UNPADDED_SIZE_MAX
                        {
                            return LZMA_DATA_ERROR;
                        }
                        self.sequence = SEQ_UNCOMPRESSED;
                    } else {
                        let append_ret = index_append(
                            self.index,
                            allocator,
                            self.unpadded_size,
                            self.uncompressed_size,
                        );
                        if append_ret != LZMA_OK {
                            return append_ret;
                        }

                        self.count -= 1;
                        self.sequence = if self.count == 0 {
                            SEQ_PADDING_INIT
                        } else {
                            SEQ_UNPADDED
                        };
                    }
                }
                SEQ_PADDING_INIT => {
                    self.pos = index_padding_size_of(index_ref(self.index)) as usize;
                    self.sequence = SEQ_PADDING;
                }
                SEQ_PADDING => {
                    if self.pos > 0 {
                        self.pos -= 1;
                        if *input.add(*in_pos) != 0 {
                            return LZMA_DATA_ERROR;
                        }
                        self.crc32 =
                            crc32::crc32(core::slice::from_raw_parts(input.add(*in_pos), 1), self.crc32);
                        *in_pos += 1;
                    } else {
                        self.sequence = SEQ_CRC32;
                    }
                }
                SEQ_CRC32 => {
                    while self.pos < 4 {
                        if *in_pos == in_size {
                            return LZMA_OK;
                        }
                        if ((self.crc32 >> (self.pos * 8)) as u8) != *input.add(*in_pos) {
                            return LZMA_DATA_ERROR;
                        }
                        *in_pos += 1;
                        self.pos += 1;
                    }
                    return LZMA_STREAM_END;
                }
                _ => return LZMA_PROG_ERROR,
            }
        }

        LZMA_OK
    }
}

unsafe fn decoder_code(
    coder: *mut c_void,
    allocator: *const lzma_allocator,
    input: *const u8,
    in_pos: *mut usize,
    in_size: usize,
    _output: *mut u8,
    _out_pos: *mut usize,
    _out_size: usize,
    _action: lzma_action,
) -> lzma_ret {
    let coder = &mut *coder.cast::<IndexDecoderStream>();
    let ret = coder.state.step(allocator, input, in_pos, in_size);
    if ret == LZMA_STREAM_END {
        *coder.dest_index = coder.state.take_index();
    }
    ret
}

unsafe fn decoder_end(coder: *mut c_void, allocator: *const lzma_allocator) {
    if coder.is_null() {
        return;
    }

    let coder = coder.cast::<IndexDecoderStream>();
    (*coder).state.end(allocator);
    ptr::drop_in_place(coder);
    lzma_free(coder.cast(), allocator);
}

unsafe fn decoder_memconfig(
    coder: *mut c_void,
    memusage: *mut u64,
    old_memlimit: *mut u64,
    new_memlimit: u64,
) -> lzma_ret {
    (*coder.cast::<IndexDecoderStream>())
        .state
        .memconfig(memusage, old_memlimit, new_memlimit)
}

fn decoder_supported_actions() -> [bool; crate::internal::common::ACTION_COUNT] {
    let mut actions = default_supported_actions();
    actions[crate::internal::common::LZMA_RUN as usize] = true;
    actions[crate::internal::common::LZMA_FINISH as usize] = true;
    actions
}

pub(crate) unsafe fn index_decoder(
    strm: *mut lzma_stream,
    index_ptr: *mut *mut lzma_index,
    memlimit: u64,
) -> lzma_ret {
    if strm.is_null() || index_ptr.is_null() {
        return LZMA_PROG_ERROR;
    }

    *index_ptr = ptr::null_mut();
    let state = match IndexDecoderState::new((*strm).allocator, memlimit) {
        Ok(state) => state,
        Err(ret) => return ret,
    };

    let raw = lzma_alloc(size_of::<IndexDecoderStream>(), (*strm).allocator).cast::<IndexDecoderStream>();
    if raw.is_null() {
        let mut state = state;
        state.end((*strm).allocator);
        return LZMA_MEM_ERROR;
    }

    ptr::write(
        raw,
        IndexDecoderStream {
            state,
            dest_index: index_ptr,
        },
    );

    let next = NextCoder {
        coder: raw.cast(),
        code: decoder_code,
        end: Some(decoder_end),
        get_progress: None,
        get_check: None,
        memconfig: Some(decoder_memconfig),
    };

    let ret = install_next_coder(strm, next, decoder_supported_actions());
    if ret != LZMA_OK {
        decoder_end(raw.cast(), (*strm).allocator);
    }
    ret
}

pub(crate) unsafe fn index_buffer_decode(
    index_ptr: *mut *mut lzma_index,
    memlimit: *mut u64,
    allocator: *const lzma_allocator,
    input: *const u8,
    in_pos: *mut usize,
    in_size: usize,
) -> lzma_ret {
    if index_ptr.is_null()
        || memlimit.is_null()
        || input.is_null()
        || in_pos.is_null()
        || *in_pos > in_size
    {
        return LZMA_PROG_ERROR;
    }

    *index_ptr = ptr::null_mut();
    let mut state = match IndexDecoderState::new(allocator, *memlimit) {
        Ok(state) => state,
        Err(ret) => return ret,
    };

    let in_start = *in_pos;
    let ret = state.step(allocator, input, in_pos, in_size);
    if ret == LZMA_STREAM_END {
        *index_ptr = state.take_index();
        return LZMA_OK;
    }

    state.end(allocator);
    *in_pos = in_start;
    if ret == LZMA_OK {
        LZMA_DATA_ERROR
    } else {
        if ret == LZMA_MEMLIMIT_ERROR {
            *memlimit = state.required_memusage();
        }
        ret
    }
}
