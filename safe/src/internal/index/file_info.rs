use core::ffi::c_void;
use core::mem::{self, size_of};
use core::ptr;

use crate::ffi::types::{
    lzma_action, lzma_allocator, lzma_index, lzma_ret, lzma_stream, lzma_stream_flags, lzma_vli,
    LZMA_DATA_ERROR, LZMA_FORMAT_ERROR, LZMA_MEMLIMIT_ERROR, LZMA_MEM_ERROR, LZMA_OK,
    LZMA_PROG_ERROR, LZMA_SEEK_NEEDED, LZMA_STREAM_END,
};
use crate::internal::common::{default_supported_actions, lzma_alloc, lzma_free, LZMA_VLI_MAX};
use crate::internal::index::core::{
    destroy_index, index_cat, index_file_size, index_memusage, index_memused, index_stream_flags,
    index_stream_padding,
};
use crate::internal::index::decode::IndexDecoderState;
use crate::internal::stream_flags::{
    stream_flags_compare_impl, stream_footer_decode_impl, stream_header_decode_impl,
    LZMA_STREAM_HEADER_SIZE,
};
use crate::internal::stream_state::{install_next_coder, NextCoder};

const SEQ_MAGIC_BYTES: u32 = 0;
const SEQ_PADDING_SEEK: u32 = 1;
const SEQ_PADDING_DECODE: u32 = 2;
const SEQ_FOOTER: u32 = 3;
const SEQ_INDEX_INIT: u32 = 4;
const SEQ_INDEX_DECODE: u32 = 5;
const SEQ_HEADER_DECODE: u32 = 6;
const SEQ_HEADER_COMPARE: u32 = 7;

pub(crate) struct FileInfoDecoder {
    sequence: u32,
    file_cur_pos: u64,
    file_target_pos: u64,
    file_size: u64,
    index_decoder: Option<IndexDecoderState>,
    index_remaining: lzma_vli,
    this_index: *mut lzma_index,
    stream_padding: lzma_vli,
    combined_index: *mut lzma_index,
    dest_index: *mut *mut lzma_index,
    external_seek_pos: *mut u64,
    memlimit: u64,
    first_header_flags: lzma_stream_flags,
    header_flags: lzma_stream_flags,
    footer_flags: lzma_stream_flags,
    temp_pos: usize,
    temp_size: usize,
    temp: [u8; 8192],
}

impl FileInfoDecoder {
    unsafe fn fill_temp(&mut self, input: *const u8, in_pos: *mut usize, in_size: usize) -> bool {
        let available = in_size.saturating_sub(*in_pos);
        let needed = self.temp_size.saturating_sub(self.temp_pos);
        let copy_len = available.min(needed);
        if copy_len > 0 {
            ptr::copy_nonoverlapping(
                input.add(*in_pos),
                self.temp.as_mut_ptr().add(self.temp_pos),
                copy_len,
            );
            *in_pos += copy_len;
            self.temp_pos += copy_len;
            self.file_cur_pos += copy_len as u64;
        }
        self.temp_pos < self.temp_size
    }

    unsafe fn seek_to_pos(
        &mut self,
        target_pos: u64,
        in_start: usize,
        in_pos: *mut usize,
        in_size: usize,
    ) -> bool {
        let pos_min = self.file_cur_pos - (*in_pos - in_start) as u64;
        let pos_max = self.file_cur_pos + (in_size - *in_pos) as u64;

        let external_seek_needed = if target_pos >= pos_min && target_pos <= pos_max {
            *in_pos += (target_pos - self.file_cur_pos) as usize;
            false
        } else {
            *self.external_seek_pos = target_pos;
            *in_pos = in_size;
            true
        };

        self.file_cur_pos = target_pos;
        external_seek_needed
    }

    unsafe fn reverse_seek(
        &mut self,
        in_start: usize,
        in_pos: *mut usize,
        in_size: usize,
    ) -> lzma_ret {
        if self.file_target_pos < 2 * LZMA_STREAM_HEADER_SIZE as u64 {
            return LZMA_DATA_ERROR;
        }

        self.temp_pos = 0;
        self.temp_size =
            if self.file_target_pos - (LZMA_STREAM_HEADER_SIZE as u64) < self.temp.len() as u64 {
                (self.file_target_pos - LZMA_STREAM_HEADER_SIZE as u64) as usize
            } else {
                self.temp.len()
            };

        if self.temp_size < LZMA_STREAM_HEADER_SIZE {
            return LZMA_DATA_ERROR;
        }

        if self.seek_to_pos(
            self.file_target_pos - self.temp_size as u64,
            in_start,
            in_pos,
            in_size,
        ) {
            LZMA_SEEK_NEEDED
        } else {
            LZMA_OK
        }
    }

    fn get_padding_size(buf: &[u8]) -> usize {
        let mut padding = 0usize;
        let mut pos = buf.len();
        while pos > 0 {
            pos -= 1;
            if buf[pos] != 0 {
                break;
            }
            padding += 1;
        }
        padding
    }

    fn hide_format_error(ret: lzma_ret) -> lzma_ret {
        if ret == LZMA_FORMAT_ERROR {
            LZMA_DATA_ERROR
        } else {
            ret
        }
    }

    unsafe fn decode_index(
        &mut self,
        allocator: *const lzma_allocator,
        input: *const u8,
        in_pos: *mut usize,
        in_size: usize,
        update_file_cur_pos: bool,
    ) -> lzma_ret {
        let start = *in_pos;
        let ret = self
            .index_decoder
            .as_mut()
            .expect("index decoder must be initialized")
            .step(allocator, input, in_pos, in_size);
        let used = *in_pos - start;
        self.index_remaining -= used as lzma_vli;
        if update_file_cur_pos {
            self.file_cur_pos += used as u64;
        }
        ret
    }

    unsafe fn end(&mut self, allocator: *const lzma_allocator) {
        if let Some(decoder) = &mut self.index_decoder {
            decoder.end(allocator);
        }
        self.index_decoder = None;
        destroy_index(self.this_index, allocator);
        destroy_index(self.combined_index, allocator);
        self.this_index = ptr::null_mut();
        self.combined_index = ptr::null_mut();
    }
}

unsafe fn file_info_code(
    coder: *mut c_void,
    allocator: *const lzma_allocator,
    input: *const u8,
    in_pos: *mut usize,
    mut in_size: usize,
    _output: *mut u8,
    _out_pos: *mut usize,
    _out_size: usize,
    _action: lzma_action,
) -> lzma_ret {
    let coder = &mut *coder.cast::<FileInfoDecoder>();
    let in_start = *in_pos;

    if coder.file_size < coder.file_cur_pos {
        return LZMA_PROG_ERROR;
    }

    let remaining_file = coder.file_size - coder.file_cur_pos;
    if remaining_file < (in_size - in_start) as u64 {
        in_size = in_start + remaining_file as usize;
    }

    loop {
        match coder.sequence {
            SEQ_MAGIC_BYTES => {
                if coder.file_size < LZMA_STREAM_HEADER_SIZE as u64 {
                    return LZMA_FORMAT_ERROR;
                }

                if coder.fill_temp(input, in_pos, in_size) {
                    return LZMA_OK;
                }

                let ret =
                    stream_header_decode_impl(&mut coder.first_header_flags, coder.temp.as_ptr());
                if ret != LZMA_OK {
                    return ret;
                }

                if coder.file_size > LZMA_VLI_MAX || (coder.file_size & 3) != 0 {
                    return LZMA_DATA_ERROR;
                }

                coder.file_target_pos = coder.file_size;
                coder.sequence = SEQ_PADDING_SEEK;
            }
            SEQ_PADDING_SEEK => {
                coder.sequence = SEQ_PADDING_DECODE;
                let ret = coder.reverse_seek(in_start, in_pos, in_size);
                if ret != LZMA_OK {
                    return ret;
                }
            }
            SEQ_PADDING_DECODE => {
                if coder.fill_temp(input, in_pos, in_size) {
                    return LZMA_OK;
                }

                let new_padding = FileInfoDecoder::get_padding_size(&coder.temp[..coder.temp_size]);
                coder.stream_padding += new_padding as lzma_vli;
                coder.file_target_pos -= new_padding as u64;

                if new_padding == coder.temp_size {
                    coder.sequence = SEQ_PADDING_SEEK;
                    continue;
                }

                if (coder.stream_padding & 3) != 0 {
                    return LZMA_DATA_ERROR;
                }

                coder.sequence = SEQ_FOOTER;
                coder.temp_size -= new_padding;
                coder.temp_pos = coder.temp_size;

                if coder.temp_size < LZMA_STREAM_HEADER_SIZE {
                    let ret = coder.reverse_seek(in_start, in_pos, in_size);
                    if ret != LZMA_OK {
                        return ret;
                    }
                }
            }
            SEQ_FOOTER => {
                if coder.fill_temp(input, in_pos, in_size) {
                    return LZMA_OK;
                }

                coder.file_target_pos -= LZMA_STREAM_HEADER_SIZE as u64;
                coder.temp_size -= LZMA_STREAM_HEADER_SIZE;
                let ret = FileInfoDecoder::hide_format_error(stream_footer_decode_impl(
                    &mut coder.footer_flags,
                    coder.temp.as_ptr().add(coder.temp_size),
                ));
                if ret != LZMA_OK {
                    return ret;
                }

                if coder.file_target_pos
                    < coder.footer_flags.backward_size + LZMA_STREAM_HEADER_SIZE as u64
                {
                    return LZMA_DATA_ERROR;
                }

                coder.file_target_pos -= coder.footer_flags.backward_size;
                coder.sequence = SEQ_INDEX_INIT;

                if coder.temp_size >= coder.footer_flags.backward_size as usize {
                    coder.temp_pos = coder.temp_size - coder.footer_flags.backward_size as usize;
                } else {
                    coder.temp_pos = 0;
                    coder.temp_size = 0;
                    if coder.seek_to_pos(coder.file_target_pos, in_start, in_pos, in_size) {
                        return LZMA_SEEK_NEEDED;
                    }
                }
            }
            SEQ_INDEX_INIT => {
                let memused = if coder.combined_index.is_null() {
                    0
                } else {
                    index_memused(coder.combined_index)
                };
                if memused > coder.memlimit {
                    return LZMA_PROG_ERROR;
                }

                if let Some(decoder) = &mut coder.index_decoder {
                    decoder.end(allocator);
                }
                coder.index_decoder = Some(
                    match IndexDecoderState::new(allocator, coder.memlimit - memused) {
                        Ok(state) => state,
                        Err(ret) => return ret,
                    },
                );
                coder.index_remaining = coder.footer_flags.backward_size;
                coder.sequence = SEQ_INDEX_DECODE;
            }
            SEQ_INDEX_DECODE => {
                let ret = if coder.temp_size != 0 {
                    let temp_ptr = coder.temp.as_ptr();
                    let temp_size = coder.temp_size;
                    let mut temp_pos = coder.temp_pos;
                    let ret =
                        coder.decode_index(allocator, temp_ptr, &mut temp_pos, temp_size, false);
                    coder.temp_pos = temp_pos;
                    ret
                } else {
                    let in_stop = if in_size - *in_pos > coder.index_remaining as usize {
                        *in_pos + coder.index_remaining as usize
                    } else {
                        in_size
                    };
                    coder.decode_index(allocator, input, in_pos, in_stop, true)
                };

                match ret {
                    LZMA_OK => {
                        if coder.index_remaining == 0 {
                            return LZMA_DATA_ERROR;
                        }
                        if coder.temp_size == 0 {
                            return LZMA_OK;
                        }
                    }
                    LZMA_STREAM_END => {
                        if coder.index_remaining != 0 {
                            return LZMA_DATA_ERROR;
                        }
                        coder.this_index = coder
                            .index_decoder
                            .as_mut()
                            .expect("decoder exists")
                            .take_index();
                    }
                    _ => return ret,
                }

                let seek_amount = crate::internal::index::core::index_total_size(coder.this_index)
                    + LZMA_STREAM_HEADER_SIZE as u64;
                if coder.file_target_pos < seek_amount {
                    return LZMA_DATA_ERROR;
                }

                coder.file_target_pos -= seek_amount;
                if coder.file_target_pos == 0 {
                    coder.header_flags = coder.first_header_flags;
                    coder.sequence = SEQ_HEADER_COMPARE;
                    continue;
                }

                coder.sequence = SEQ_HEADER_DECODE;
                coder.file_target_pos += LZMA_STREAM_HEADER_SIZE as u64;

                if coder.temp_size != 0
                    && coder.temp_size - coder.footer_flags.backward_size as usize
                        >= seek_amount as usize
                {
                    coder.temp_pos = coder.temp_size
                        - coder.footer_flags.backward_size as usize
                        - seek_amount as usize
                        + LZMA_STREAM_HEADER_SIZE;
                    coder.temp_size = coder.temp_pos;
                } else {
                    let ret = coder.reverse_seek(in_start, in_pos, in_size);
                    if ret != LZMA_OK {
                        return ret;
                    }
                }
            }
            SEQ_HEADER_DECODE => {
                if coder.fill_temp(input, in_pos, in_size) {
                    return LZMA_OK;
                }

                coder.file_target_pos -= LZMA_STREAM_HEADER_SIZE as u64;
                coder.temp_size -= LZMA_STREAM_HEADER_SIZE;
                coder.temp_pos = coder.temp_size;
                let ret = FileInfoDecoder::hide_format_error(stream_header_decode_impl(
                    &mut coder.header_flags,
                    coder.temp.as_ptr().add(coder.temp_size),
                ));
                if ret != LZMA_OK {
                    return ret;
                }
                coder.sequence = SEQ_HEADER_COMPARE;
            }
            SEQ_HEADER_COMPARE => {
                let ret = stream_flags_compare_impl(&coder.header_flags, &coder.footer_flags);
                if ret != LZMA_OK {
                    return ret;
                }

                if index_stream_flags(coder.this_index, &coder.footer_flags) != LZMA_OK {
                    return LZMA_PROG_ERROR;
                }
                if index_stream_padding(coder.this_index, coder.stream_padding) != LZMA_OK {
                    return LZMA_PROG_ERROR;
                }
                coder.stream_padding = 0;

                if !coder.combined_index.is_null() {
                    let ret = index_cat(coder.this_index, coder.combined_index, allocator);
                    if ret != LZMA_OK {
                        return ret;
                    }
                }

                coder.combined_index = coder.this_index;
                coder.this_index = ptr::null_mut();

                if coder.file_target_pos == 0 {
                    if index_file_size(coder.combined_index) != coder.file_size {
                        return LZMA_DATA_ERROR;
                    }
                    *coder.dest_index = coder.combined_index;
                    coder.combined_index = ptr::null_mut();
                    *in_pos = in_size;
                    return LZMA_STREAM_END;
                }

                coder.sequence = if coder.temp_size > 0 {
                    SEQ_PADDING_DECODE
                } else {
                    SEQ_PADDING_SEEK
                };
            }
            _ => return LZMA_PROG_ERROR,
        }
    }
}

unsafe fn file_info_end(coder: *mut c_void, allocator: *const lzma_allocator) {
    if coder.is_null() {
        return;
    }

    let coder = coder.cast::<FileInfoDecoder>();
    (*coder).end(allocator);
    ptr::drop_in_place(coder);
    lzma_free(coder.cast(), allocator);
}

unsafe fn file_info_memconfig(
    coder: *mut c_void,
    memusage: *mut u64,
    old_memlimit: *mut u64,
    new_memlimit: u64,
) -> lzma_ret {
    let coder = &mut *coder.cast::<FileInfoDecoder>();
    let combined_mem = if coder.combined_index.is_null() {
        0
    } else {
        index_memused(coder.combined_index)
    };

    let this_mem = if !coder.this_index.is_null() {
        index_memused(coder.this_index)
    } else if coder.sequence == SEQ_INDEX_DECODE {
        let mut mem = 0u64;
        let mut dummy = 0u64;
        if coder
            .index_decoder
            .as_mut()
            .expect("decoder exists")
            .memconfig(&mut mem, &mut dummy, 0)
            != LZMA_OK
        {
            return LZMA_PROG_ERROR;
        }
        mem
    } else {
        0
    };

    *memusage = combined_mem + this_mem;
    if *memusage == 0 {
        *memusage = index_memusage(1, 0);
    }
    *old_memlimit = coder.memlimit;

    if new_memlimit != 0 {
        if new_memlimit < *memusage {
            return LZMA_MEMLIMIT_ERROR;
        }

        if coder.this_index.is_null() && coder.sequence == SEQ_INDEX_DECODE {
            let new_decoder_limit = new_memlimit - combined_mem;
            let mut dummy1 = 0u64;
            let mut dummy2 = 0u64;
            if coder
                .index_decoder
                .as_mut()
                .expect("decoder exists")
                .memconfig(&mut dummy1, &mut dummy2, new_decoder_limit)
                != LZMA_OK
            {
                return LZMA_PROG_ERROR;
            }
        }

        coder.memlimit = new_memlimit;
    }

    LZMA_OK
}

fn file_info_supported_actions() -> [bool; crate::internal::common::ACTION_COUNT] {
    let mut actions = default_supported_actions();
    actions[crate::internal::common::LZMA_RUN as usize] = true;
    actions[crate::internal::common::LZMA_FINISH as usize] = true;
    actions
}

pub(crate) unsafe fn file_info_decoder(
    strm: *mut lzma_stream,
    dest_index: *mut *mut lzma_index,
    memlimit: u64,
    file_size: u64,
) -> lzma_ret {
    if strm.is_null() || dest_index.is_null() {
        return LZMA_PROG_ERROR;
    }

    *dest_index = ptr::null_mut();
    let raw = lzma_alloc(size_of::<FileInfoDecoder>(), (*strm).allocator).cast::<FileInfoDecoder>();
    if raw.is_null() {
        return LZMA_MEM_ERROR;
    }

    ptr::write(
        raw,
        FileInfoDecoder {
            sequence: SEQ_MAGIC_BYTES,
            file_cur_pos: 0,
            file_target_pos: 0,
            file_size,
            index_decoder: None,
            index_remaining: 0,
            this_index: ptr::null_mut(),
            stream_padding: 0,
            combined_index: ptr::null_mut(),
            dest_index,
            external_seek_pos: &mut (*strm).seek_pos,
            memlimit: memlimit.max(1),
            first_header_flags: mem::zeroed(),
            header_flags: mem::zeroed(),
            footer_flags: mem::zeroed(),
            temp_pos: 0,
            temp_size: LZMA_STREAM_HEADER_SIZE,
            temp: [0; 8192],
        },
    );

    let next = NextCoder {
        coder: raw.cast(),
        code: file_info_code,
        end: Some(file_info_end),
        get_progress: None,
        get_check: None,
        memconfig: Some(file_info_memconfig),
        update: None,
    };

    let ret = install_next_coder(strm, next, file_info_supported_actions());
    if ret != LZMA_OK {
        file_info_end(raw.cast(), (*strm).allocator);
    }
    ret
}
