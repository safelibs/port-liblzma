use core::ffi::c_void;
use core::mem::size_of;
use core::ptr;

use crate::ffi::types::{
    lzma_action, lzma_allocator, lzma_index, lzma_ret, lzma_stream, LZMA_BUF_ERROR,
    LZMA_MEM_ERROR, LZMA_OK, LZMA_PROG_ERROR, LZMA_STREAM_END,
};
use crate::internal::check::crc32;
use crate::internal::common::{default_supported_actions, lzma_alloc, lzma_free};
use crate::internal::index::core::{
    index_padding_size_of, index_ref, index_size_from_counts, vli_ceil4, Index, INDEX_INDICATOR,
};
use crate::internal::stream_state::{install_next_coder, NextCoder};

struct IndexEncoderStream {
    buffer: Vec<u8>,
    position: usize,
}

fn push_vli(buffer: &mut Vec<u8>, mut value: u64) {
    while value >= 0x80 {
        buffer.push((value as u8) | 0x80);
        value >>= 7;
    }
    buffer.push(value as u8);
}

fn encode_index_bytes(index: &Index) -> Vec<u8> {
    let mut out = Vec::with_capacity(index_size_from_counts(index.record_count, index.index_list_size) as usize);
    out.push(INDEX_INDICATOR);
    push_vli(&mut out, index.record_count);

    for stream in index.streams() {
        let records = stream.records();
        for (record_index, record) in records.iter().enumerate() {
            let prev_unpadded = if record_index == 0 {
                0
            } else {
                vli_ceil4(records[record_index - 1].unpadded_sum)
            };
            let prev_uncompressed = if record_index == 0 {
                0
            } else {
                records[record_index - 1].uncompressed_sum
            };

            push_vli(&mut out, record.unpadded_sum - prev_unpadded);
            push_vli(&mut out, record.uncompressed_sum - prev_uncompressed);
        }
    }

    out.resize(out.len() + index_padding_size_of(index) as usize, 0);
    let crc = crc32::crc32(&out, 0);
    out.extend_from_slice(&crc.to_le_bytes());
    debug_assert_eq!(
        out.len() as u64,
        index_size_from_counts(index.record_count, index.index_list_size)
    );
    out
}

unsafe fn encoder_code(
    coder: *mut c_void,
    _allocator: *const lzma_allocator,
    _input: *const u8,
    in_pos: *mut usize,
    _in_size: usize,
    output: *mut u8,
    out_pos: *mut usize,
    out_size: usize,
    _action: lzma_action,
) -> lzma_ret {
    let coder = &mut *coder.cast::<IndexEncoderStream>();
    *in_pos = 0;

    if *out_pos >= out_size || coder.position >= coder.buffer.len() {
        return if coder.position >= coder.buffer.len() {
            LZMA_STREAM_END
        } else {
            LZMA_OK
        };
    }

    let available = out_size - *out_pos;
    let remaining = coder.buffer.len() - coder.position;
    let copy_len = available.min(remaining);
    ptr::copy_nonoverlapping(
        coder.buffer.as_ptr().add(coder.position),
        output.add(*out_pos),
        copy_len,
    );
    coder.position += copy_len;
    *out_pos += copy_len;

    if coder.position == coder.buffer.len() {
        LZMA_STREAM_END
    } else {
        LZMA_OK
    }
}

unsafe fn encoder_end(coder: *mut c_void, allocator: *const lzma_allocator) {
    if coder.is_null() {
        return;
    }

    ptr::drop_in_place(coder.cast::<IndexEncoderStream>());
    lzma_free(coder, allocator);
}

fn encoder_supported_actions() -> [bool; crate::internal::common::ACTION_COUNT] {
    let mut actions = default_supported_actions();
    actions[crate::internal::common::LZMA_RUN as usize] = true;
    actions[crate::internal::common::LZMA_FINISH as usize] = true;
    actions
}

pub(crate) unsafe fn index_encoder(strm: *mut lzma_stream, index: *const lzma_index) -> lzma_ret {
    if strm.is_null() || index.is_null() {
        return LZMA_PROG_ERROR;
    }

    let bytes = encode_index_bytes(index_ref(index));
    let raw = lzma_alloc(size_of::<IndexEncoderStream>(), (*strm).allocator)
        .cast::<IndexEncoderStream>();
    if raw.is_null() {
        return LZMA_MEM_ERROR;
    }

    ptr::write(
        raw,
        IndexEncoderStream {
            buffer: bytes,
            position: 0,
        },
    );

    let next = NextCoder {
        coder: raw.cast(),
        code: encoder_code,
        end: Some(encoder_end),
        get_progress: None,
        get_check: None,
        memconfig: None,
    };

    let ret = install_next_coder(strm, next, encoder_supported_actions());
    if ret != LZMA_OK {
        encoder_end(raw.cast(), (*strm).allocator);
    }
    ret
}

pub(crate) unsafe fn index_buffer_encode(
    index: *const lzma_index,
    out: *mut u8,
    out_pos: *mut usize,
    out_size: usize,
) -> lzma_ret {
    if index.is_null() || out.is_null() || out_pos.is_null() || *out_pos > out_size {
        return LZMA_PROG_ERROR;
    }

    let bytes = encode_index_bytes(index_ref(index));
    if out_size - *out_pos < bytes.len() {
        return LZMA_BUF_ERROR;
    }

    ptr::copy_nonoverlapping(bytes.as_ptr(), out.add(*out_pos), bytes.len());
    *out_pos += bytes.len();
    LZMA_OK
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::internal::index::core::{Index, IndexRecord};

    #[test]
    fn encoder_writes_expected_empty_index() {
        let index = Index::new();
        let encoded = encode_index_bytes(&index);
        assert_eq!(encoded, vec![0x00, 0x00, 0x00, 0x00, 0x1C, 0xDF, 0x44, 0x21]);
    }

    #[test]
    fn encoder_handles_nonempty_records() {
        let mut index = Index::new();
        unsafe {
            let stream = index.last_stream_mut();
            assert!(stream.push_record(
                IndexRecord {
                    uncompressed_sum: 1,
                    unpadded_sum: 5,
                },
                2,
                ptr::null(),
            ));
            assert!(stream.push_record(
                IndexRecord {
                    uncompressed_sum: 10,
                    unpadded_sum: 14,
                },
                0,
                ptr::null(),
            ));
            stream.index_list_size = 4;
        }
        index.record_count = 2;
        index.index_list_size = 4;
        index.uncompressed_size = 10;
        index.total_size = 16;

        let encoded = encode_index_bytes(&index);
        assert_eq!(encoded[0], 0);
        assert_eq!(encoded[1], 2);
        assert_eq!(encoded.len() as u64, index_size_from_counts(2, 4));
    }
}
