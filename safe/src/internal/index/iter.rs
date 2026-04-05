use core::mem;
use core::ptr;

use crate::ffi::types::{
    lzma_index, lzma_index_iter, lzma_index_iter_block, lzma_index_iter_mode,
    lzma_index_iter_stream, lzma_bool, lzma_vli,
};
use crate::internal::common::lzma_bool as to_lzma_bool;
use crate::internal::stream_flags::LZMA_STREAM_HEADER_SIZE;

use super::core::{index_ref, vli_ceil4, Index, IndexStream};

const LZMA_INDEX_ITER_ANY: lzma_index_iter_mode = 0;
const LZMA_INDEX_ITER_STREAM: lzma_index_iter_mode = 1;
const LZMA_INDEX_ITER_BLOCK: lzma_index_iter_mode = 2;
const LZMA_INDEX_ITER_NONEMPTY_BLOCK: lzma_index_iter_mode = 3;

const INTERNAL_INDEX_PTR: usize = 0;
const INTERNAL_STREAM_POS: usize = 1;
const INTERNAL_RECORD_POS: usize = 2;

fn read_stream_pos(iter: &lzma_index_iter) -> Option<usize> {
    let raw = unsafe { iter.internal[INTERNAL_STREAM_POS].v };
    if raw == lzma_vli::MAX {
        None
    } else {
        Some(raw as usize)
    }
}

fn write_stream_pos(iter: &mut lzma_index_iter, value: Option<usize>) {
    iter.internal[INTERNAL_STREAM_POS].v = value.map(|v| v as lzma_vli).unwrap_or(lzma_vli::MAX);
}

fn read_record_pos(iter: &lzma_index_iter) -> Option<usize> {
    let raw = unsafe { iter.internal[INTERNAL_RECORD_POS].v };
    if raw == lzma_vli::MAX {
        None
    } else {
        Some(raw as usize)
    }
}

fn write_record_pos(iter: &mut lzma_index_iter, value: Option<usize>) {
    iter.internal[INTERNAL_RECORD_POS].v = value.map(|v| v as lzma_vli).unwrap_or(lzma_vli::MAX);
}

fn current_index(iter: &lzma_index_iter) -> Option<&'static Index> {
    let ptr = unsafe { iter.internal[INTERNAL_INDEX_PTR].p };
    if ptr.is_null() {
        None
    } else {
        Some(unsafe { index_ref(ptr.cast::<lzma_index>()) })
    }
}

fn fill_stream_info(out: &mut lzma_index_iter_stream, stream: &IndexStream) {
    out.flags = stream
        .stream_flags
        .as_ref()
        .map(|flags| flags as *const _)
        .unwrap_or(ptr::null());
    out.reserved_ptr1 = ptr::null();
    out.reserved_ptr2 = ptr::null();
    out.reserved_ptr3 = ptr::null();
    out.number = stream.number as lzma_vli;
    out.block_count = stream.record_count();
    out.compressed_offset = stream.compressed_base;
    out.uncompressed_offset = stream.uncompressed_base;
    out.compressed_size = stream.compressed_size();
    out.uncompressed_size = stream.uncompressed_size();
    out.padding = stream.stream_padding;
    out.reserved_vli1 = 0;
    out.reserved_vli2 = 0;
    out.reserved_vli3 = 0;
    out.reserved_vli4 = 0;
}

fn fill_block_info(out: &mut lzma_index_iter_block, stream: &IndexStream, record_index: usize) {
    let record = stream.records[record_index];
    let prev_uncompressed = if record_index == 0 {
        0
    } else {
        stream.records[record_index - 1].uncompressed_sum
    };
    let prev_unpadded = if record_index == 0 {
        0
    } else {
        vli_ceil4(stream.records[record_index - 1].unpadded_sum)
    };

    out.number_in_stream = record_index as lzma_vli + 1;
    out.number_in_file = stream.block_number_base + out.number_in_stream;
    out.compressed_stream_offset = prev_unpadded + LZMA_STREAM_HEADER_SIZE as lzma_vli;
    out.uncompressed_stream_offset = prev_uncompressed;
    out.unpadded_size = record.unpadded_sum - prev_unpadded;
    out.uncompressed_size = record.uncompressed_sum - prev_uncompressed;
    out.total_size = vli_ceil4(out.unpadded_size);
    out.compressed_file_offset = stream.compressed_base + out.compressed_stream_offset;
    out.uncompressed_file_offset = stream.uncompressed_base + out.uncompressed_stream_offset;
    out.reserved_vli1 = 0;
    out.reserved_vli2 = 0;
    out.reserved_vli3 = 0;
    out.reserved_vli4 = 0;
    out.reserved_ptr1 = ptr::null();
    out.reserved_ptr2 = ptr::null();
    out.reserved_ptr3 = ptr::null();
    out.reserved_ptr4 = ptr::null();
}

fn clear_block_info(out: &mut lzma_index_iter_block) {
    *out = unsafe { mem::zeroed() };
}

fn has_nonempty_block(stream: &IndexStream, record_index: usize) -> bool {
    let current = stream.records[record_index].uncompressed_sum;
    let previous = if record_index == 0 {
        0
    } else {
        stream.records[record_index - 1].uncompressed_sum
    };
    current > previous
}

fn first_nonempty_stream(index: &Index, start: usize) -> Option<usize> {
    index
        .streams
        .iter()
        .enumerate()
        .skip(start)
        .find(|(_, stream)| !stream.records.is_empty())
        .map(|(pos, _)| pos)
}

fn next_stream_position(index: &Index, start: usize) -> Option<usize> {
    if start < index.streams.len() {
        Some(start)
    } else {
        None
    }
}

fn candidate_next(
    index: &Index,
    current_stream: Option<usize>,
    current_record: Option<usize>,
    mode: lzma_index_iter_mode,
) -> Option<(usize, Option<usize>)> {
    match mode {
        LZMA_INDEX_ITER_STREAM => {
            let stream = current_stream.map_or(0, |pos| pos + 1);
            let stream = next_stream_position(index, stream)?;
            let record = if index.streams[stream].records.is_empty() {
                None
            } else {
                Some(0)
            };
            Some((stream, record))
        }
        LZMA_INDEX_ITER_BLOCK | LZMA_INDEX_ITER_NONEMPTY_BLOCK => {
            let mut stream = current_stream.unwrap_or(usize::MAX);
            let mut record = current_record.unwrap_or(usize::MAX);

            loop {
                if stream == usize::MAX {
                    stream = first_nonempty_stream(index, 0)?;
                    record = 0;
                } else {
                    let records = &index.streams[stream].records;
                    if !records.is_empty() && record + 1 < records.len() {
                        record += 1;
                    } else {
                        stream = first_nonempty_stream(index, stream + 1)?;
                        record = 0;
                    }
                }

                if mode == LZMA_INDEX_ITER_BLOCK
                    || has_nonempty_block(&index.streams[stream], record)
                {
                    return Some((stream, Some(record)));
                }
            }
        }
        LZMA_INDEX_ITER_ANY => {
            if let Some(stream) = current_stream {
                let current = &index.streams[stream];
                if let Some(record) = current_record {
                    if record + 1 < current.records.len() {
                        return Some((stream, Some(record + 1)));
                    }
                }

                let next_stream = next_stream_position(index, stream + 1)?;
                let next_record = if index.streams[next_stream].records.is_empty() {
                    None
                } else {
                    Some(0)
                };
                Some((next_stream, next_record))
            } else {
                let stream = next_stream_position(index, 0)?;
                let record = if index.streams[stream].records.is_empty() {
                    None
                } else {
                    Some(0)
                };
                Some((stream, record))
            }
        }
        _ => None,
    }
}

pub(crate) unsafe fn index_iter_init(iter: *mut lzma_index_iter, index: *const lzma_index) {
    if iter.is_null() {
        return;
    }

    ptr::write_bytes(iter, 0, 1);
    (*iter).internal[INTERNAL_INDEX_PTR].p = index.cast();
    index_iter_rewind(iter);
}

pub(crate) unsafe fn index_iter_rewind(iter: *mut lzma_index_iter) {
    if iter.is_null() {
        return;
    }

    write_stream_pos(&mut *iter, None);
    write_record_pos(&mut *iter, None);
    clear_block_info(&mut (*iter).block);
    (*iter).stream = mem::zeroed();
}

pub(crate) unsafe fn index_iter_next(
    iter: *mut lzma_index_iter,
    mode: lzma_index_iter_mode,
) -> lzma_bool {
    if iter.is_null() {
        return 1;
    }

    if !(LZMA_INDEX_ITER_ANY..=LZMA_INDEX_ITER_NONEMPTY_BLOCK).contains(&mode) {
        return 1;
    }

    let Some(index) = current_index(&*iter) else {
        return 1;
    };

    let current_stream = read_stream_pos(&*iter);
    let current_record = read_record_pos(&*iter);
    let Some((next_stream, next_record)) = candidate_next(index, current_stream, current_record, mode)
    else {
        return 1;
    };

    let stream = &index.streams[next_stream];
    fill_stream_info(&mut (*iter).stream, stream);
    if let Some(record) = next_record {
        fill_block_info(&mut (*iter).block, stream, record);
    } else {
        clear_block_info(&mut (*iter).block);
    }

    write_stream_pos(&mut *iter, Some(next_stream));
    write_record_pos(&mut *iter, next_record);
    to_lzma_bool(false)
}

pub(crate) unsafe fn index_iter_locate(iter: *mut lzma_index_iter, target: lzma_vli) -> lzma_bool {
    if iter.is_null() {
        return 1;
    }

    let Some(index) = current_index(&*iter) else {
        return 1;
    };

    if index.uncompressed_size <= target {
        return 1;
    }

    let stream_pos = match index
        .streams
        .partition_point(|stream| stream.uncompressed_base <= target)
        .checked_sub(1)
    {
        Some(pos) => pos,
        None => return 1,
    };
    let stream = &index.streams[stream_pos];
    let target_in_stream = target - stream.uncompressed_base;

    let record_pos = stream
        .records
        .partition_point(|record| record.uncompressed_sum <= target_in_stream);
    if record_pos >= stream.records.len() {
        return 1;
    }

    fill_stream_info(&mut (*iter).stream, stream);
    fill_block_info(&mut (*iter).block, stream, record_pos);
    write_stream_pos(&mut *iter, Some(stream_pos));
    write_record_pos(&mut *iter, Some(record_pos));
    to_lzma_bool(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::internal::index::core::{Index, IndexRecord, IndexStream};

    fn sample_index() -> Index {
        let mut first = IndexStream::new(0, 0, 1, 0);
        first.records = vec![
            IndexRecord {
                uncompressed_sum: 0,
                unpadded_sum: 16,
            },
            IndexRecord {
                uncompressed_sum: 5,
                unpadded_sum: 48,
            },
            IndexRecord {
                uncompressed_sum: 16,
                unpadded_sum: 88,
            },
        ];
        first.index_list_size = 6;

        let mut second = IndexStream::new(120, 16, 2, 3);
        second.records = vec![IndexRecord {
            uncompressed_sum: 7,
            unpadded_sum: 24,
        }];
        second.index_list_size = 2;

        Index {
            streams: vec![first, second],
            uncompressed_size: 23,
            total_size: 112,
            record_count: 4,
            index_list_size: 8,
            prealloc: 512,
            checks: 0,
        }
    }

    #[test]
    fn locate_skips_zero_size_blocks() {
        let index = sample_index();
        let mut iter: lzma_index_iter = unsafe { mem::zeroed() };
        unsafe {
            index_iter_init(&mut iter, (&index as *const Index).cast::<lzma_index>());
            assert_eq!(index_iter_locate(&mut iter, 0), 0);
            assert_eq!(iter.block.number_in_file, 2);
            assert_eq!(iter.block.uncompressed_size, 5);
        }
    }

    #[test]
    fn nonempty_block_mode_skips_empty_entries() {
        let index = sample_index();
        let mut iter: lzma_index_iter = unsafe { mem::zeroed() };
        unsafe {
            index_iter_init(&mut iter, (&index as *const Index).cast::<lzma_index>());
            assert_eq!(index_iter_next(&mut iter, LZMA_INDEX_ITER_NONEMPTY_BLOCK), 0);
            assert_eq!(iter.block.number_in_file, 2);
            assert_eq!(index_iter_next(&mut iter, LZMA_INDEX_ITER_NONEMPTY_BLOCK), 0);
            assert_eq!(iter.block.number_in_file, 3);
            assert_eq!(index_iter_next(&mut iter, LZMA_INDEX_ITER_NONEMPTY_BLOCK), 0);
            assert_eq!(iter.block.number_in_file, 4);
            assert_eq!(index_iter_next(&mut iter, LZMA_INDEX_ITER_NONEMPTY_BLOCK), 1);
        }
    }
}
