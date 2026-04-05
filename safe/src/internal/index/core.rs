use core::ffi::c_void;
use core::mem::size_of;
use core::ptr;

use crate::ffi::types::{
    lzma_allocator, lzma_index, lzma_ret, lzma_stream_flags, lzma_vli, LZMA_DATA_ERROR,
    LZMA_OK, LZMA_PROG_ERROR, LZMA_VLI_UNKNOWN,
};
use crate::internal::common::{lzma_alloc, lzma_free, LZMA_VLI_MAX};
use crate::internal::stream_flags::{
    self, LZMA_BACKWARD_SIZE_MAX, LZMA_STREAM_HEADER_SIZE,
};
use crate::internal::vli::lzma_vli_size_impl;

pub(crate) const UNPADDED_SIZE_MIN: lzma_vli = 5;
pub(crate) const UNPADDED_SIZE_MAX: lzma_vli = LZMA_VLI_MAX & !3;
pub(crate) const INDEX_INDICATOR: u8 = 0;
pub(crate) const INDEX_GROUP_SIZE: usize = 512;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct IndexRecord {
    pub(crate) uncompressed_sum: lzma_vli,
    pub(crate) unpadded_sum: lzma_vli,
}

#[derive(Clone)]
pub(crate) struct IndexStream {
    pub(crate) compressed_base: lzma_vli,
    pub(crate) uncompressed_base: lzma_vli,
    pub(crate) number: u32,
    pub(crate) block_number_base: lzma_vli,
    pub(crate) records: Vec<IndexRecord>,
    pub(crate) index_list_size: lzma_vli,
    pub(crate) stream_flags: Option<lzma_stream_flags>,
    pub(crate) stream_padding: lzma_vli,
}

#[derive(Clone)]
pub(crate) struct Index {
    pub(crate) streams: Vec<IndexStream>,
    pub(crate) uncompressed_size: lzma_vli,
    pub(crate) total_size: lzma_vli,
    pub(crate) record_count: lzma_vli,
    pub(crate) index_list_size: lzma_vli,
    pub(crate) prealloc: usize,
    pub(crate) checks: u32,
}

#[repr(C)]
struct UpstreamIndexTreeNode {
    uncompressed_base: lzma_vli,
    compressed_base: lzma_vli,
    parent: *mut c_void,
    left: *mut c_void,
    right: *mut c_void,
}

#[repr(C)]
struct UpstreamIndexTree {
    root: *mut c_void,
    leftmost: *mut c_void,
    rightmost: *mut c_void,
    count: u32,
}

#[repr(C)]
struct UpstreamIndexRecord {
    uncompressed_sum: lzma_vli,
    unpadded_sum: lzma_vli,
}

#[repr(C)]
struct UpstreamIndexGroup {
    node: UpstreamIndexTreeNode,
    number_base: lzma_vli,
    allocated: usize,
    last: usize,
}

#[repr(C)]
struct UpstreamIndexStream {
    node: UpstreamIndexTreeNode,
    number: u32,
    _pad: u32,
    block_number_base: lzma_vli,
    groups: UpstreamIndexTree,
    record_count: lzma_vli,
    index_list_size: lzma_vli,
    stream_flags: lzma_stream_flags,
    stream_padding: lzma_vli,
}

#[repr(C)]
struct UpstreamIndex {
    streams: UpstreamIndexTree,
    uncompressed_size: lzma_vli,
    total_size: lzma_vli,
    record_count: lzma_vli,
    index_list_size: lzma_vli,
    prealloc: usize,
    checks: u32,
}

pub(crate) const PREALLOC_MAX: usize =
    (usize::MAX - size_of::<UpstreamIndexGroup>()) / size_of::<UpstreamIndexRecord>();

impl IndexStream {
    pub(crate) fn new(
        compressed_base: lzma_vli,
        uncompressed_base: lzma_vli,
        number: u32,
        block_number_base: lzma_vli,
    ) -> Self {
        Self {
            compressed_base,
            uncompressed_base,
            number,
            block_number_base,
            records: Vec::new(),
            index_list_size: 0,
            stream_flags: None,
            stream_padding: 0,
        }
    }

    #[inline]
    pub(crate) fn record_count(&self) -> lzma_vli {
        self.records.len() as lzma_vli
    }

    #[inline]
    pub(crate) fn last_record(&self) -> Option<&IndexRecord> {
        self.records.last()
    }

    #[inline]
    pub(crate) fn blocks_size(&self) -> lzma_vli {
        self.last_record()
            .map(|record| vli_ceil4(record.unpadded_sum))
            .unwrap_or(0)
    }

    #[inline]
    pub(crate) fn uncompressed_size(&self) -> lzma_vli {
        self.last_record()
            .map(|record| record.uncompressed_sum)
            .unwrap_or(0)
    }

    #[inline]
    pub(crate) fn compressed_size(&self) -> lzma_vli {
        2 * LZMA_STREAM_HEADER_SIZE as lzma_vli
            + self.blocks_size()
            + index_size_from_counts(self.record_count(), self.index_list_size)
    }

    #[inline]
    pub(crate) fn file_end(&self) -> lzma_vli {
        self.compressed_base + self.compressed_size() + self.stream_padding
    }
}

impl Index {
    pub(crate) fn new() -> Self {
        Self {
            streams: vec![IndexStream::new(0, 0, 1, 0)],
            uncompressed_size: 0,
            total_size: 0,
            record_count: 0,
            index_list_size: 0,
            prealloc: INDEX_GROUP_SIZE,
            checks: 0,
        }
    }

    #[inline]
    pub(crate) fn last_stream(&self) -> &IndexStream {
        self.streams
            .last()
            .expect("lzma_index always has at least one stream")
    }

    #[inline]
    pub(crate) fn last_stream_mut(&mut self) -> &mut IndexStream {
        self.streams
            .last_mut()
            .expect("lzma_index always has at least one stream")
    }
}

#[inline]
pub(crate) const fn vli_ceil4(vli: lzma_vli) -> lzma_vli {
    (vli + 3) & !3
}

#[inline]
pub(crate) const fn index_size_unpadded_from_counts(
    count: lzma_vli,
    index_list_size: lzma_vli,
) -> lzma_vli {
    1 + lzma_vli_size_impl(count) as lzma_vli + index_list_size + 4
}

#[inline]
pub(crate) const fn index_size_from_counts(count: lzma_vli, index_list_size: lzma_vli) -> lzma_vli {
    vli_ceil4(index_size_unpadded_from_counts(count, index_list_size))
}

#[inline]
pub(crate) const fn index_stream_size_from_counts(
    blocks_size: lzma_vli,
    count: lzma_vli,
    index_list_size: lzma_vli,
) -> lzma_vli {
    LZMA_STREAM_HEADER_SIZE as lzma_vli
        + blocks_size
        + index_size_from_counts(count, index_list_size)
        + LZMA_STREAM_HEADER_SIZE as lzma_vli
}

#[inline]
pub(crate) unsafe fn index_ref(ptr: *const lzma_index) -> &'static Index {
    &*ptr.cast::<Index>()
}

#[inline]
pub(crate) unsafe fn index_mut(ptr: *mut lzma_index) -> &'static mut Index {
    &mut *ptr.cast::<Index>()
}

pub(crate) unsafe fn alloc_index(index: Index, allocator: *const lzma_allocator) -> *mut lzma_index {
    let raw = lzma_alloc(size_of::<Index>(), allocator).cast::<Index>();
    if raw.is_null() {
        return ptr::null_mut();
    }

    ptr::write(raw, index);
    raw.cast::<lzma_index>()
}

pub(crate) unsafe fn destroy_index(ptr: *mut lzma_index, allocator: *const lzma_allocator) {
    if ptr.is_null() {
        return;
    }

    ptr::drop_in_place(ptr.cast::<Index>());
    lzma_free(ptr.cast(), allocator);
}

pub(crate) fn index_padding_size_of(index: &Index) -> u32 {
    (4u64
        .wrapping_sub(index_size_unpadded_from_counts(
            index.record_count,
            index.index_list_size,
        ))
        & 3) as u32
}

fn index_file_size_inner(
    compressed_base: lzma_vli,
    unpadded_sum: lzma_vli,
    record_count: lzma_vli,
    index_list_size: lzma_vli,
    stream_padding: lzma_vli,
) -> Option<lzma_vli> {
    let mut file_size = compressed_base
        .checked_add(2 * LZMA_STREAM_HEADER_SIZE as lzma_vli)?
        .checked_add(stream_padding)?
        .checked_add(vli_ceil4(unpadded_sum))?;

    if file_size > LZMA_VLI_MAX {
        return None;
    }

    file_size = file_size.checked_add(index_size_from_counts(record_count, index_list_size))?;
    if file_size > LZMA_VLI_MAX {
        return None;
    }

    Some(file_size)
}

fn check_bit(check: i32) -> u32 {
    if (0..32).contains(&check) {
        1u32 << check
    } else {
        0
    }
}

unsafe fn dup_probe_allocations(index: &Index, allocator: *const lzma_allocator) -> *mut Index {
    let base = lzma_alloc(size_of::<Index>(), allocator).cast::<Index>();
    if base.is_null() {
        return ptr::null_mut();
    }

    let mut probes: Vec<*mut c_void> = Vec::new();
    for stream in &index.streams {
        let stream_probe = lzma_alloc(1, allocator);
        if stream_probe.is_null() {
            for probe in probes {
                lzma_free(probe, allocator);
            }
            lzma_free(base.cast(), allocator);
            return ptr::null_mut();
        }
        probes.push(stream_probe);

        if !stream.records.is_empty() {
            let group_probe = lzma_alloc(1, allocator);
            if group_probe.is_null() {
                for probe in probes {
                    lzma_free(probe, allocator);
                }
                lzma_free(base.cast(), allocator);
                return ptr::null_mut();
            }
            probes.push(group_probe);
        }
    }

    for probe in probes {
        lzma_free(probe, allocator);
    }

    base
}

pub(crate) unsafe fn index_init(allocator: *const lzma_allocator) -> *mut lzma_index {
    alloc_index(Index::new(), allocator)
}

pub(crate) unsafe fn index_end(index: *mut lzma_index, allocator: *const lzma_allocator) {
    destroy_index(index, allocator);
}

pub(crate) fn index_memusage(streams: lzma_vli, blocks: lzma_vli) -> u64 {
    let alloc_overhead = 4 * size_of::<*const c_void>();
    let stream_base =
        size_of::<UpstreamIndexStream>() + size_of::<UpstreamIndexGroup>() + 2 * alloc_overhead;
    let group_base =
        size_of::<UpstreamIndexGroup>() + INDEX_GROUP_SIZE * size_of::<UpstreamIndexRecord>()
            + alloc_overhead;
    let index_base = size_of::<UpstreamIndex>() + alloc_overhead;

    let groups = (blocks + INDEX_GROUP_SIZE as lzma_vli - 1) / INDEX_GROUP_SIZE as lzma_vli;
    let limit = u64::MAX - index_base as u64;

    if streams == 0
        || streams > u32::MAX as lzma_vli
        || blocks >= LZMA_VLI_MAX
        || streams > limit / stream_base as u64
        || groups > limit / group_base as u64
    {
        return u64::MAX;
    }

    let streams_mem = streams * stream_base as u64;
    let groups_mem = groups * group_base as u64;
    if limit - streams_mem < groups_mem {
        return u64::MAX;
    }

    index_base as u64 + streams_mem + groups_mem
}

pub(crate) unsafe fn index_memused(index: *const lzma_index) -> u64 {
    if index.is_null() {
        return 0;
    }

    let index = index_ref(index);
    index_memusage(index.streams.len() as lzma_vli, index.record_count)
}

pub(crate) unsafe fn index_block_count(index: *const lzma_index) -> lzma_vli {
    if index.is_null() {
        return 0;
    }

    index_ref(index).record_count
}

pub(crate) unsafe fn index_stream_count(index: *const lzma_index) -> lzma_vli {
    if index.is_null() {
        return 0;
    }

    index_ref(index).streams.len() as lzma_vli
}

pub(crate) unsafe fn index_size(index: *const lzma_index) -> lzma_vli {
    if index.is_null() {
        return 0;
    }

    let index = index_ref(index);
    index_size_from_counts(index.record_count, index.index_list_size)
}

pub(crate) unsafe fn index_total_size(index: *const lzma_index) -> lzma_vli {
    if index.is_null() {
        return 0;
    }

    index_ref(index).total_size
}

pub(crate) unsafe fn index_stream_size(index: *const lzma_index) -> lzma_vli {
    if index.is_null() {
        return 0;
    }

    let index = index_ref(index);
    index_stream_size_from_counts(index.total_size, index.record_count, index.index_list_size)
}

pub(crate) unsafe fn index_file_size(index: *const lzma_index) -> lzma_vli {
    if index.is_null() {
        return 0;
    }

    let stream = index_ref(index).last_stream();
    let unpadded_sum = stream
        .last_record()
        .map(|record| record.unpadded_sum)
        .unwrap_or(0);

    index_file_size_inner(
        stream.compressed_base,
        unpadded_sum,
        stream.record_count(),
        stream.index_list_size,
        stream.stream_padding,
    )
    .unwrap_or(LZMA_VLI_UNKNOWN)
}

pub(crate) unsafe fn index_uncompressed_size(index: *const lzma_index) -> lzma_vli {
    if index.is_null() {
        return 0;
    }

    index_ref(index).uncompressed_size
}

pub(crate) unsafe fn index_checks(index: *const lzma_index) -> u32 {
    if index.is_null() {
        return 0;
    }

    let index = index_ref(index);
    let mut checks = index.checks;
    if let Some(flags) = index.last_stream().stream_flags {
        checks |= check_bit(flags.check);
    }
    checks
}

pub(crate) unsafe fn index_stream_flags(
    index: *mut lzma_index,
    stream_flags: *const lzma_stream_flags,
) -> lzma_ret {
    if index.is_null() || stream_flags.is_null() {
        return LZMA_PROG_ERROR;
    }

    let ret = stream_flags::stream_flags_compare_impl(stream_flags, stream_flags);
    if ret != LZMA_OK {
        return ret;
    }

    index_mut(index).last_stream_mut().stream_flags = Some(*stream_flags);
    LZMA_OK
}

pub(crate) unsafe fn index_stream_padding(
    index: *mut lzma_index,
    stream_padding: lzma_vli,
) -> lzma_ret {
    if index.is_null() || (stream_padding & 3) != 0 || stream_padding > LZMA_VLI_MAX {
        return LZMA_PROG_ERROR;
    }

    let index = index_mut(index);
    let stream = index.last_stream_mut();
    let old_padding = stream.stream_padding;
    stream.stream_padding = 0;

    let unpadded_sum = stream.last_record().map(|record| record.unpadded_sum).unwrap_or(0);
    let ok = index_file_size_inner(
        stream.compressed_base,
        unpadded_sum,
        stream.record_count(),
        stream.index_list_size,
        0,
    );
    if ok.is_none() || ok.unwrap().saturating_add(stream_padding) > LZMA_VLI_MAX {
        stream.stream_padding = old_padding;
        return LZMA_DATA_ERROR;
    }

    stream.stream_padding = stream_padding;
    LZMA_OK
}

pub(crate) unsafe fn index_append(
    index: *mut lzma_index,
    _allocator: *const lzma_allocator,
    unpadded_size: lzma_vli,
    uncompressed_size: lzma_vli,
) -> lzma_ret {
    if index.is_null()
        || unpadded_size < UNPADDED_SIZE_MIN
        || unpadded_size > UNPADDED_SIZE_MAX
        || uncompressed_size > LZMA_VLI_MAX
    {
        return LZMA_PROG_ERROR;
    }

    let index = index_mut(index);
    let (
        compressed_base,
        uncompressed_base,
        stream_compressed_base,
        stream_record_count,
        stream_index_list_size,
        stream_padding,
        needs_reserve,
    ) = {
        let stream = index.last_stream();
        (
            stream
                .last_record()
                .map(|record| vli_ceil4(record.unpadded_sum))
                .unwrap_or(0),
            stream
                .last_record()
                .map(|record| record.uncompressed_sum)
                .unwrap_or(0),
            stream.compressed_base,
            stream.record_count(),
            stream.index_list_size,
            stream.stream_padding,
            stream.records.capacity() == stream.records.len(),
        )
    };
    let index_list_size_add =
        lzma_vli_size_impl(unpadded_size) as lzma_vli + lzma_vli_size_impl(uncompressed_size) as lzma_vli;

    if uncompressed_base.checked_add(uncompressed_size).is_none()
        || uncompressed_base + uncompressed_size > LZMA_VLI_MAX
    {
        return LZMA_DATA_ERROR;
    }

    if compressed_base.checked_add(unpadded_size).is_none()
        || compressed_base + unpadded_size > UNPADDED_SIZE_MAX
    {
        return LZMA_DATA_ERROR;
    }

    if index_file_size_inner(
        stream_compressed_base,
        compressed_base + unpadded_size,
        stream_record_count + 1,
        stream_index_list_size + index_list_size_add,
        stream_padding,
    )
    .is_none()
    {
        return LZMA_DATA_ERROR;
    }

    let combined_index_size = index_size_from_counts(
        index.record_count + 1,
        index.index_list_size + index_list_size_add,
    );
    if combined_index_size > LZMA_BACKWARD_SIZE_MAX {
        return LZMA_DATA_ERROR;
    }

    if needs_reserve {
        let reserve = index.prealloc.max(1);
        index.last_stream_mut().records.reserve(reserve);
        index.prealloc = INDEX_GROUP_SIZE;
    }

    let stream = index.last_stream_mut();
    stream.records.push(IndexRecord {
        uncompressed_sum: uncompressed_base + uncompressed_size,
        unpadded_sum: compressed_base + unpadded_size,
    });
    stream.index_list_size += index_list_size_add;

    index.total_size += vli_ceil4(unpadded_size);
    index.uncompressed_size += uncompressed_size;
    index.record_count += 1;
    index.index_list_size += index_list_size_add;

    LZMA_OK
}

pub(crate) unsafe fn index_cat(
    dest: *mut lzma_index,
    src: *mut lzma_index,
    allocator: *const lzma_allocator,
) -> lzma_ret {
    if dest.is_null() || src.is_null() {
        return LZMA_PROG_ERROR;
    }

    let dest_ref = index_ref(dest);
    let src_ref = index_ref(src);
    let dest_file_size = index_file_size(dest);
    let src_file_size = index_file_size(src);
    if dest_file_size == LZMA_VLI_UNKNOWN
        || src_file_size == LZMA_VLI_UNKNOWN
        || dest_file_size.saturating_add(src_file_size) > LZMA_VLI_MAX
        || dest_ref.uncompressed_size.saturating_add(src_ref.uncompressed_size) > LZMA_VLI_MAX
    {
        return LZMA_DATA_ERROR;
    }

    let dest_size =
        index_size_unpadded_from_counts(dest_ref.record_count, dest_ref.index_list_size);
    let src_size =
        index_size_unpadded_from_counts(src_ref.record_count, src_ref.index_list_size);
    let Some(combined_size) = dest_size.checked_add(src_size) else {
        return LZMA_DATA_ERROR;
    };
    if vli_ceil4(combined_size) > LZMA_BACKWARD_SIZE_MAX {
        return LZMA_DATA_ERROR;
    }

    let old_checks = index_checks(dest);
    let dest_streams_len = dest_ref.streams.len() as u32;
    let dest_uncompressed_size = dest_ref.uncompressed_size;
    let dest_record_count = dest_ref.record_count;

    let mut moved = ptr::read(src.cast::<Index>());
    lzma_free(src.cast(), allocator);

    for stream in &mut moved.streams {
        stream.compressed_base += dest_file_size;
        stream.uncompressed_base += dest_uncompressed_size;
        stream.number = stream.number.saturating_add(dest_streams_len);
        stream.block_number_base += dest_record_count;
    }

    let dest_mut = index_mut(dest);
    dest_mut.checks = old_checks;
    dest_mut.uncompressed_size += moved.uncompressed_size;
    dest_mut.total_size += moved.total_size;
    dest_mut.record_count += moved.record_count;
    dest_mut.index_list_size += moved.index_list_size;
    dest_mut.checks |= moved.checks;
    dest_mut.streams.extend(moved.streams.drain(..));

    LZMA_OK
}

pub(crate) unsafe fn index_dup(
    src: *const lzma_index,
    allocator: *const lzma_allocator,
) -> *mut lzma_index {
    if src.is_null() {
        return ptr::null_mut();
    }

    let src = index_ref(src);
    let base = dup_probe_allocations(src, allocator);
    if base.is_null() {
        return ptr::null_mut();
    }

    ptr::write(base, src.clone());
    base.cast::<lzma_index>()
}

pub(crate) fn index_prealloc(index: &mut Index, records: lzma_vli) {
    index.prealloc = records.min(PREALLOC_MAX as lzma_vli) as usize;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ffi::types::lzma_allocator;
    use crate::internal::common::LZMA_CHECK_CRC32;

    #[test]
    fn memusage_matches_basic_contract() {
        assert_eq!(index_memusage(0, 1), u64::MAX);
        assert_eq!(index_memusage(u32::MAX as u64 + 1, 1), u64::MAX);
        assert_eq!(index_memusage(1, LZMA_VLI_MAX), u64::MAX);
        assert_ne!(index_memusage(1, 0), u64::MAX);
    }

    #[test]
    fn file_size_and_checks_follow_stream_updates() {
        let mut index = Index::new();
        let flags = lzma_stream_flags {
            version: 0,
            backward_size: 8,
            check: LZMA_CHECK_CRC32,
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

        unsafe {
            let raw = alloc_index(index.clone(), ptr::null());
            assert_eq!(index_stream_flags(raw, &flags), LZMA_OK);
            assert_eq!(index_checks(raw), 1u32 << LZMA_CHECK_CRC32);
            assert_eq!(index_file_size(raw), 32);
            destroy_index(raw, ptr::null());
        }

        index.last_stream_mut().stream_flags = Some(flags);
        assert_eq!(index.last_stream().compressed_size(), 32);
    }

    #[test]
    fn dup_uses_allocator_probe_count_like_upstream() {
        unsafe extern "C" fn fail_after_two(
            opaque: *mut c_void,
            _nmemb: usize,
            size: usize,
        ) -> *mut c_void {
            let counter = &mut *opaque.cast::<u32>();
            *counter += 1;
            if *counter > 2 {
                ptr::null_mut()
            } else {
                libc::malloc(size.max(1))
            }
        }

        let mut counter = 0u32;
        let allocator = lzma_allocator {
            alloc: Some(fail_after_two),
            free: None,
            opaque: (&mut counter as *mut u32).cast(),
        };

        let raw = unsafe { index_init(ptr::null()) };
        assert!(!raw.is_null());
        unsafe {
            assert_eq!(index_stream_padding(raw, 4), LZMA_OK);
            let copy = index_dup(raw, &allocator);
            assert!(!copy.is_null());
            destroy_index(copy, &allocator);

            assert_eq!(index_append(raw, ptr::null(), UNPADDED_SIZE_MIN, 1), LZMA_OK);
            counter = 0;
            assert_eq!(counter, 0);
            assert!(index_dup(raw, &allocator).is_null());
            destroy_index(raw, ptr::null());
        }
    }
}
