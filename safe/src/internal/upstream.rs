use core::ffi::c_void;
use core::mem;
use core::ptr;
use std::{
    cell::RefCell,
    io::{self, Cursor, Read, Write},
    rc::Rc,
};

use crate::ffi::types::{
    lzma_action, lzma_allocator, lzma_block, lzma_check, lzma_filter, lzma_mt, lzma_options_lzma,
    lzma_ret, lzma_stream, LZMA_BUF_ERROR, LZMA_GET_CHECK, LZMA_NO_CHECK, LZMA_OK,
    LZMA_OPTIONS_ERROR, LZMA_PROG_ERROR, LZMA_STREAM_END, LZMA_UNSUPPORTED_CHECK, LZMA_VLI_UNKNOWN,
};
use crate::internal::block;
use crate::internal::check::{self, CheckState};
use crate::internal::common::{
    all_supported_actions, ACTION_COUNT, LZMA_FINISH, LZMA_PRESET_LEVEL_MASK, LZMA_RUN,
    LZMA_SYNC_FLUSH,
};
use crate::internal::filter;
use crate::internal::lzma::{
    self, ParsedFilterChain, Prefilter, TerminalFilter, LZMA_MEMUSAGE_BASE,
};
use crate::internal::preset;
use crate::internal::stream_state::{current_next_coder, install_next_coder, NextCoder};
use crate::internal::vli::lzma_vli_encode_impl;

const STREAM_ENCODER_MAGIC: u64 = 0x7366_6c74_5f78_7a31;
const LZMA_TELL_NO_CHECK: u32 = 0x01;
const LZMA_TELL_UNSUPPORTED_CHECK: u32 = 0x02;
const LZMA_TELL_ANY_CHECK: u32 = 0x04;
const LZMA_CONCATENATED: u32 = 0x08;
const LZMA_IGNORE_CHECK: u32 = 0x10;
const LZMA_FAIL_FAST: u32 = 0x20;
const STREAM_DECODER_SUPPORTED_FLAGS: u32 = LZMA_TELL_NO_CHECK
    | LZMA_TELL_UNSUPPORTED_CHECK
    | LZMA_TELL_ANY_CHECK
    | LZMA_CONCATENATED
    | LZMA_IGNORE_CHECK
    | LZMA_FAIL_FAST;
const LZMA_THREADS_MAX: u32 = 16384;

#[derive(Clone, Copy)]
pub(crate) struct IndexRecord {
    pub(crate) unpadded_size: u64,
    pub(crate) uncompressed_size: u64,
}

struct RawCoder {
    filters: [lzma_filter; crate::ffi::types::LZMA_FILTERS_MAX + 1],
    state: RawCoderState,
}

enum RawCoderState {
    Encoder(RawEncoderState),
    Decoder(RawDecoderState),
}

struct RawEncoderState {
    writer: Option<Box<dyn FinishableWrite>>,
    sink: SharedSink,
    supports_sync_flush: bool,
    finished: bool,
    pending_stream_end: bool,
}

struct RawDecoderState {
    reader: Box<dyn Read>,
    source: SharedSource,
    pending: Vec<u8>,
    pending_pos: usize,
    finished_input: bool,
    stream_finished: bool,
}

#[derive(Clone, Default)]
struct SharedSink(Rc<RefCell<SinkState>>);

#[derive(Default)]
struct SinkState {
    data: Vec<u8>,
    read_pos: usize,
}

#[derive(Clone, Default)]
struct SharedSource(Rc<RefCell<SourceState>>);

#[derive(Default)]
struct SourceState {
    data: Vec<u8>,
    read_pos: usize,
    finished: bool,
}

struct StreamEncoderCoder {
    magic: u64,
    filters: [lzma_filter; crate::ffi::types::LZMA_FILTERS_MAX + 1],
    check: lzma_check,
    input: Vec<u8>,
    pending: Vec<u8>,
    pending_pos: usize,
    records: Vec<IndexRecord>,
    header_written: bool,
    finished: bool,
}

struct StreamDecoderCoder {
    input: Vec<u8>,
    output: Vec<u8>,
    output_pos: usize,
    memlimit: u64,
    memusage: u64,
    flags: u32,
    check: lzma_check,
    pending_ret: lzma_ret,
    header_parsed: bool,
    decoded: bool,
}

pub(crate) unsafe fn copy_filters(
    src: *const lzma_filter,
) -> Result<[lzma_filter; crate::ffi::types::LZMA_FILTERS_MAX + 1], lzma_ret> {
    let mut dest = [lzma_filter {
        id: LZMA_VLI_UNKNOWN,
        options: ptr::null_mut(),
    }; crate::ffi::types::LZMA_FILTERS_MAX + 1];
    let ret = filter::filters_copy_impl(src, dest.as_mut_ptr(), ptr::null());
    if ret != LZMA_OK {
        return Err(ret);
    }

    Ok(dest)
}

pub(crate) unsafe fn free_filters(
    filters: &mut [lzma_filter; crate::ffi::types::LZMA_FILTERS_MAX + 1],
) {
    filter::filters_free_impl(filters.as_mut_ptr(), ptr::null());
}

unsafe fn copy_output(
    buffer: &[u8],
    state_pos: &mut usize,
    output: *mut u8,
    out_pos: *mut usize,
    out_size: usize,
) -> lzma_ret {
    let copy_size = (buffer.len() - *state_pos).min(out_size - *out_pos);
    ptr::copy_nonoverlapping(
        buffer.as_ptr().add(*state_pos),
        output.add(*out_pos),
        copy_size,
    );
    *state_pos += copy_size;
    *out_pos += copy_size;
    if *state_pos == buffer.len() {
        LZMA_STREAM_END
    } else {
        LZMA_OK
    }
}

trait FinishableWrite: Write {
    fn finish(self: Box<Self>) -> Result<(), lzma_ret>;
}

impl SharedSink {
    fn copy_available(&self, output: *mut u8, out_pos: *mut usize, out_size: usize) -> usize {
        let mut state = self.0.borrow_mut();
        let available = state.data.len().saturating_sub(state.read_pos);
        let copy_size = available.min(out_size - unsafe { *out_pos });
        if copy_size == 0 {
            return 0;
        }

        unsafe {
            ptr::copy_nonoverlapping(
                state.data.as_ptr().add(state.read_pos),
                output.add(*out_pos),
                copy_size,
            );
            *out_pos += copy_size;
        }
        state.read_pos += copy_size;
        if state.read_pos == state.data.len() {
            state.data.clear();
            state.read_pos = 0;
        }

        copy_size
    }
}

impl Write for SharedSink {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.borrow_mut().data.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl SharedSource {
    fn append(&self, buf: &[u8]) {
        if buf.is_empty() {
            return;
        }

        let mut state = self.0.borrow_mut();
        state.data.extend_from_slice(buf);
    }

    fn finish(&self) {
        self.0.borrow_mut().finished = true;
    }
}

impl Read for SharedSource {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }

        let mut state = self.0.borrow_mut();
        let available = state.data.len().saturating_sub(state.read_pos);
        if available == 0 {
            return if state.finished {
                Ok(0)
            } else {
                Err(io::Error::from(io::ErrorKind::WouldBlock))
            };
        }

        let copy_size = available.min(buf.len());
        buf[..copy_size].copy_from_slice(&state.data[state.read_pos..state.read_pos + copy_size]);
        state.read_pos += copy_size;

        if state.read_pos == state.data.len() {
            state.data.clear();
            state.read_pos = 0;
        } else if state.read_pos >= 4096 && state.read_pos * 2 >= state.data.len() {
            let drain_to = state.read_pos;
            state.data.drain(..drain_to);
            state.read_pos = 0;
        }

        Ok(copy_size)
    }
}

struct TerminalLzma1Writer {
    writer: Option<lzma_rust2::LzmaWriter<SharedSink>>,
}

impl Write for TerminalLzma1Writer {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.writer.as_mut().unwrap().write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.writer.as_mut().unwrap().flush()
    }
}

impl FinishableWrite for TerminalLzma1Writer {
    fn finish(mut self: Box<Self>) -> Result<(), lzma_ret> {
        self.writer
            .take()
            .unwrap()
            .finish()
            .map(|_| ())
            .map_err(|error| lzma::io_error_to_ret(&error))
    }
}

struct TerminalLzma2Writer {
    writer: Option<lzma_rust2::Lzma2Writer<SharedSink>>,
}

impl Write for TerminalLzma2Writer {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.writer.as_mut().unwrap().write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.writer.as_mut().unwrap().flush()
    }
}

impl FinishableWrite for TerminalLzma2Writer {
    fn finish(mut self: Box<Self>) -> Result<(), lzma_ret> {
        self.writer
            .take()
            .unwrap()
            .finish()
            .map(|_| ())
            .map_err(|error| lzma::io_error_to_ret(&error))
    }
}

struct DeltaWriterWrapper {
    writer: Option<lzma_rust2::filter::delta::DeltaWriter<Box<dyn FinishableWrite>>>,
}

impl Write for DeltaWriterWrapper {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.writer.as_mut().unwrap().write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.writer.as_mut().unwrap().flush()
    }
}

impl FinishableWrite for DeltaWriterWrapper {
    fn finish(mut self: Box<Self>) -> Result<(), lzma_ret> {
        self.writer.take().unwrap().into_inner().finish()
    }
}

struct BcjWriterWrapper {
    writer: Option<lzma_rust2::filter::bcj::BcjWriter<Box<dyn FinishableWrite>>>,
}

impl Write for BcjWriterWrapper {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.writer.as_mut().unwrap().write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.writer.as_mut().unwrap().flush()
    }
}

impl FinishableWrite for BcjWriterWrapper {
    fn finish(mut self: Box<Self>) -> Result<(), lzma_ret> {
        let inner = self
            .writer
            .take()
            .unwrap()
            .finish()
            .map_err(|error| lzma::io_error_to_ret(&error))?;
        inner.finish()
    }
}

fn wrap_bcj_writer(
    kind: crate::internal::simple::SimpleFilterKind,
    start_offset: u32,
    inner: Box<dyn FinishableWrite>,
) -> Box<dyn FinishableWrite> {
    let writer = match kind {
        crate::internal::simple::SimpleFilterKind::X86 => {
            lzma_rust2::filter::bcj::BcjWriter::new_x86(inner, start_offset as usize)
        }
        crate::internal::simple::SimpleFilterKind::PowerPc => {
            lzma_rust2::filter::bcj::BcjWriter::new_ppc(inner, start_offset as usize)
        }
        crate::internal::simple::SimpleFilterKind::Ia64 => {
            lzma_rust2::filter::bcj::BcjWriter::new_ia64(inner, start_offset as usize)
        }
        crate::internal::simple::SimpleFilterKind::Arm => {
            lzma_rust2::filter::bcj::BcjWriter::new_arm(inner, start_offset as usize)
        }
        crate::internal::simple::SimpleFilterKind::ArmThumb => {
            lzma_rust2::filter::bcj::BcjWriter::new_arm_thumb(inner, start_offset as usize)
        }
        crate::internal::simple::SimpleFilterKind::Arm64 => {
            lzma_rust2::filter::bcj::BcjWriter::new_arm64(inner, start_offset as usize)
        }
        crate::internal::simple::SimpleFilterKind::Sparc => {
            lzma_rust2::filter::bcj::BcjWriter::new_sparc(inner, start_offset as usize)
        }
    };

    Box::new(BcjWriterWrapper {
        writer: Some(writer),
    })
}

fn wrap_bcj_reader(
    kind: crate::internal::simple::SimpleFilterKind,
    start_offset: u32,
    inner: Box<dyn Read>,
) -> Box<dyn Read> {
    match kind {
        crate::internal::simple::SimpleFilterKind::X86 => Box::new(
            lzma_rust2::filter::bcj::BcjReader::new_x86(inner, start_offset as usize),
        ),
        crate::internal::simple::SimpleFilterKind::PowerPc => Box::new(
            lzma_rust2::filter::bcj::BcjReader::new_ppc(inner, start_offset as usize),
        ),
        crate::internal::simple::SimpleFilterKind::Ia64 => Box::new(
            lzma_rust2::filter::bcj::BcjReader::new_ia64(inner, start_offset as usize),
        ),
        crate::internal::simple::SimpleFilterKind::Arm => Box::new(
            lzma_rust2::filter::bcj::BcjReader::new_arm(inner, start_offset as usize),
        ),
        crate::internal::simple::SimpleFilterKind::ArmThumb => Box::new(
            lzma_rust2::filter::bcj::BcjReader::new_arm_thumb(inner, start_offset as usize),
        ),
        crate::internal::simple::SimpleFilterKind::Arm64 => Box::new(
            lzma_rust2::filter::bcj::BcjReader::new_arm64(inner, start_offset as usize),
        ),
        crate::internal::simple::SimpleFilterKind::Sparc => Box::new(
            lzma_rust2::filter::bcj::BcjReader::new_sparc(inner, start_offset as usize),
        ),
    }
}

fn build_raw_encoder(
    chain: &ParsedFilterChain,
    sink: SharedSink,
) -> Result<(Box<dyn FinishableWrite>, bool), lzma_ret> {
    let mut writer: Box<dyn FinishableWrite> = match &chain.terminal {
        TerminalFilter::Lzma1 {
            options,
            allow_eopm,
            ..
        } => Box::new(TerminalLzma1Writer {
            writer: Some(
                lzma_rust2::LzmaWriter::new_no_header(sink.clone(), options, *allow_eopm)
                    .map_err(|error| lzma::io_error_to_ret(&error))?,
            ),
        }),
        TerminalFilter::Lzma2 { options, .. } => Box::new(TerminalLzma2Writer {
            writer: Some(lzma_rust2::Lzma2Writer::new(sink.clone(), options.clone())),
        }),
    };

    let mut supports_sync_flush = matches!(chain.terminal, TerminalFilter::Lzma2 { .. });
    for filter in chain.prefilters.iter().rev() {
        writer = match *filter {
            Prefilter::Delta { distance } => Box::new(DeltaWriterWrapper {
                writer: Some(lzma_rust2::filter::delta::DeltaWriter::new(
                    writer, distance,
                )),
            }),
            Prefilter::Simple { kind, start_offset } => {
                supports_sync_flush = false;
                wrap_bcj_writer(kind, start_offset, writer)
            }
        };
    }

    Ok((writer, supports_sync_flush))
}

fn build_raw_decoder(
    chain: &ParsedFilterChain,
    source: SharedSource,
) -> Result<Box<dyn Read>, lzma_ret> {
    let mut reader: Box<dyn Read> = match &chain.terminal {
        TerminalFilter::Lzma1 {
            options,
            expected_uncompressed_size,
            ..
        } => Box::new(
            lzma_rust2::LzmaReader::new(
                source.clone(),
                expected_uncompressed_size.unwrap_or(u64::MAX),
                options.lc,
                options.lp,
                options.pb,
                options.dict_size,
                options.preset_dict.as_deref(),
            )
            .map_err(|error| lzma::io_error_to_ret(&error))?,
        ),
        TerminalFilter::Lzma2 { options, .. } => Box::new(lzma_rust2::Lzma2Reader::new(
            source.clone(),
            options.lzma_options.dict_size,
            options.lzma_options.preset_dict.as_deref(),
        )),
    };

    for filter in chain.prefilters.iter().rev() {
        reader = match *filter {
            Prefilter::Delta { distance } => Box::new(lzma_rust2::filter::delta::DeltaReader::new(
                reader, distance,
            )),
            Prefilter::Simple { kind, start_offset } => wrap_bcj_reader(kind, start_offset, reader),
        };
    }

    Ok(reader)
}

const fn raw_encoder_actions() -> [bool; ACTION_COUNT] {
    let mut actions = [false; ACTION_COUNT];
    actions[LZMA_RUN as usize] = true;
    actions[LZMA_SYNC_FLUSH as usize] = true;
    actions[LZMA_FINISH as usize] = true;
    actions
}

const fn raw_decoder_actions() -> [bool; ACTION_COUNT] {
    let mut actions = [false; ACTION_COUNT];
    actions[LZMA_RUN as usize] = true;
    actions[LZMA_FINISH as usize] = true;
    actions
}

fn append_vli(output: &mut Vec<u8>, value: u64) {
    let mut temp = [0u8; crate::internal::common::LZMA_VLI_BYTES_MAX];
    let mut pos = 0usize;
    unsafe {
        let ret = lzma_vli_encode_impl(
            value,
            ptr::null_mut(),
            temp.as_mut_ptr(),
            &mut pos,
            temp.len(),
        );
        debug_assert_eq!(ret, LZMA_OK);
    }
    output.extend_from_slice(&temp[..pos]);
}

pub(crate) fn write_xz_stream_header(check: lzma_check, output: &mut Vec<u8>) {
    output.extend_from_slice(&[0xFD, b'7', b'z', b'X', b'Z', 0x00, 0x00, check as u8]);
    let crc = check::crc32::crc32(&output[6..8], 0);
    output.extend_from_slice(&crc.to_le_bytes());
}

pub(crate) fn write_xz_stream_footer(check: lzma_check, backward_size: u32, output: &mut Vec<u8>) {
    let mut footer = Vec::with_capacity(12);
    footer.extend_from_slice(&backward_size.to_le_bytes());
    footer.extend_from_slice(&[0, check as u8]);
    let crc = check::crc32::crc32(&footer, 0);
    output.extend_from_slice(&crc.to_le_bytes());
    output.extend_from_slice(&footer);
    output.extend_from_slice(b"YZ");
}

pub(crate) fn encode_xz_index(records: &[IndexRecord]) -> Vec<u8> {
    let mut output = Vec::new();
    output.push(0x00);
    append_vli(&mut output, records.len() as u64);
    for record in records {
        append_vli(&mut output, record.unpadded_size);
        append_vli(&mut output, record.uncompressed_size);
    }

    while (output.len() + 4) % 4 != 0 {
        output.push(0);
    }
    let crc = check::crc32::crc32(&output, 0);
    output.extend_from_slice(&crc.to_le_bytes());
    output
}

fn decode_vli(input: &[u8], pos: &mut usize) -> Result<u64, lzma_ret> {
    let mut value = 0u64;
    let mut shift = 0u32;

    loop {
        if *pos >= input.len() || shift >= 63 {
            return Err(crate::ffi::types::LZMA_DATA_ERROR);
        }

        let byte = input[*pos];
        *pos += 1;

        value |= u64::from(byte & 0x7F) << shift;
        if (byte & 0x80) == 0 {
            return Ok(value);
        }

        shift += 7;
    }
}

fn parse_index_records(input: &[u8]) -> Result<Vec<IndexRecord>, lzma_ret> {
    if input.len() < 8 {
        return Err(crate::ffi::types::LZMA_DATA_ERROR);
    }

    let payload_len = input.len() - 4;
    let expected_crc = check::crc32::crc32(&input[..payload_len], 0);
    let actual_crc = u32::from_le_bytes([
        input[payload_len],
        input[payload_len + 1],
        input[payload_len + 2],
        input[payload_len + 3],
    ]);
    if expected_crc != actual_crc || input[0] != 0x00 {
        return Err(crate::ffi::types::LZMA_DATA_ERROR);
    }

    let mut pos = 1usize;
    let record_count = decode_vli(&input[..payload_len], &mut pos)?;
    let mut records = Vec::with_capacity(record_count as usize);
    for _ in 0..record_count {
        let unpadded_size = decode_vli(&input[..payload_len], &mut pos)?;
        let uncompressed_size = decode_vli(&input[..payload_len], &mut pos)?;
        records.push(IndexRecord {
            unpadded_size,
            uncompressed_size,
        });
    }

    while pos < payload_len {
        if input[pos] != 0 {
            return Err(crate::ffi::types::LZMA_DATA_ERROR);
        }
        pos += 1;
    }

    Ok(records)
}

fn decode_lzma2_uncompressed_chunks_for_xz(
    input: &[u8],
    mut need_dict_reset: bool,
) -> Result<Vec<u8>, lzma_ret> {
    let mut pos = 0usize;
    let mut output = Vec::new();

    while pos < input.len() {
        let control = input[pos];
        pos += 1;

        if control == 0x00 {
            if pos != input.len() {
                return Err(crate::ffi::types::LZMA_DATA_ERROR);
            }
            return Ok(output);
        }

        match control {
            0x01 => need_dict_reset = false,
            0x02 => {
                if need_dict_reset {
                    return Err(crate::ffi::types::LZMA_DATA_ERROR);
                }
            }
            _ => return Err(crate::ffi::types::LZMA_DATA_ERROR),
        }

        if input.len() - pos < 2 {
            return Err(crate::ffi::types::LZMA_DATA_ERROR);
        }

        let copy_size = (((input[pos] as usize) << 8) | input[pos + 1] as usize) + 1;
        pos += 2;

        if input.len() - pos < copy_size {
            return Err(crate::ffi::types::LZMA_DATA_ERROR);
        }

        output.extend_from_slice(&input[pos..pos + copy_size]);
        pos += copy_size;
    }

    Err(crate::ffi::types::LZMA_DATA_ERROR)
}

unsafe fn decode_validated_xz_block(
    input: &[u8],
    block_start: usize,
    index_start: usize,
    check_id: lzma_check,
    record: IndexRecord,
    ignore_check: bool,
) -> Result<(usize, Vec<u8>, u64), lzma_ret> {
    let mut decoded_filters = [lzma_filter {
        id: LZMA_VLI_UNKNOWN,
        options: ptr::null_mut(),
    }; crate::ffi::types::LZMA_FILTERS_MAX + 1];

    let result = (|| {
        let mut block_options: lzma_block = mem::zeroed();
        block_options.version = 1;
        block_options.check = check_id;
        block_options.header_size = ((input[block_start] as u32) + 1) * 4;
        block_options.filters = decoded_filters.as_mut_ptr();

        let ret = block::block_header_decode(
            &mut block_options,
            ptr::null(),
            input.as_ptr().add(block_start),
        );
        if ret != LZMA_OK {
            return Err(ret);
        }

        if block_options.uncompressed_size != LZMA_VLI_UNKNOWN
            && block_options.uncompressed_size != record.uncompressed_size
        {
            return Err(crate::ffi::types::LZMA_DATA_ERROR);
        }
        block_options.uncompressed_size = record.uncompressed_size;

        let memusage = lzma::decoder_memusage(decoded_filters.as_ptr());
        if memusage == u64::MAX {
            return Err(LZMA_OPTIONS_ERROR);
        }

        let ret = block::block_compressed_size(&mut block_options, record.unpadded_size);
        if ret != LZMA_OK {
            return Err(ret);
        }

        let compressed_size = block_options.compressed_size as usize;
        let check_size = check::check_size(block_options.check) as usize;
        let block_end = block_start + block::block_total_size(&block_options) as usize;
        let data_start = block_start + block_options.header_size as usize;
        let check_start = block_end.saturating_sub(check_size);
        if block_end > index_start || data_start + compressed_size > check_start {
            return Err(crate::ffi::types::LZMA_DATA_ERROR);
        }

        let chain = lzma::parse_filters(decoded_filters.as_ptr())?;
        let compressed = &input[data_start..data_start + compressed_size];
        let (decoded, consumed) = match lzma::decode_raw(&chain, compressed) {
            Ok((decoded, consumed)) => (decoded, consumed),
            Err(ret) => {
                if chain.prefilters.is_empty()
                    && matches!(chain.terminal, TerminalFilter::Lzma2 { .. })
                {
                    let need_dict_reset = match &chain.terminal {
                        TerminalFilter::Lzma2 { options, .. } => {
                            options.lzma_options.preset_dict.is_none()
                        }
                        _ => true,
                    };
                    match decode_lzma2_uncompressed_chunks_for_xz(compressed, need_dict_reset) {
                        Ok(decoded) => (decoded, compressed.len()),
                        Err(_) => return Err(ret),
                    }
                } else {
                    return Err(ret);
                }
            }
        };

        if consumed != compressed_size || decoded.len() as u64 != record.uncompressed_size {
            return Err(crate::ffi::types::LZMA_DATA_ERROR);
        }

        if !ignore_check && check::check_is_supported(block_options.check) != 0 {
            let mut state = CheckState::new(block_options.check).ok_or(LZMA_OPTIONS_ERROR)?;
            state.update(&decoded);
            let expected_check = &input[check_start..block_end];
            if state.finish()[..check_size] != expected_check[..check_size] {
                return Err(crate::ffi::types::LZMA_DATA_ERROR);
            }
        }

        let padding_start = data_start + compressed_size;
        if input[padding_start..check_start]
            .iter()
            .any(|byte| *byte != 0)
        {
            return Err(crate::ffi::types::LZMA_DATA_ERROR);
        }

        Ok((block_end, decoded, memusage))
    })();

    filter::filters_free_impl(decoded_filters.as_mut_ptr(), ptr::null());
    result
}

fn decode_single_xz_stream_fallback(
    input: &[u8],
    ignore_check: bool,
) -> Result<(usize, Vec<u8>), lzma_ret> {
    use crate::ffi::types::lzma_stream_flags;
    use crate::internal::stream_flags::{
        stream_flags_compare_impl, stream_footer_decode_impl, stream_header_decode_impl,
        LZMA_STREAM_HEADER_SIZE,
    };

    if input.len() < LZMA_STREAM_HEADER_SIZE * 2 {
        return Err(crate::ffi::types::LZMA_DATA_ERROR);
    }

    let mut header_flags: lzma_stream_flags = unsafe { mem::zeroed() };
    let mut footer_flags: lzma_stream_flags = unsafe { mem::zeroed() };

    unsafe {
        let ret = stream_header_decode_impl(&mut header_flags, input.as_ptr());
        if ret != LZMA_OK {
            return Err(ret);
        }

        let ret = stream_footer_decode_impl(
            &mut footer_flags,
            input.as_ptr().add(input.len() - LZMA_STREAM_HEADER_SIZE),
        );
        if ret != LZMA_OK {
            return Err(ret);
        }

        let ret = stream_flags_compare_impl(&header_flags, &footer_flags);
        if ret != LZMA_OK {
            return Err(ret);
        }
    }

    let index_size = footer_flags.backward_size as usize;
    if input.len() < LZMA_STREAM_HEADER_SIZE + index_size + LZMA_STREAM_HEADER_SIZE {
        return Err(crate::ffi::types::LZMA_DATA_ERROR);
    }

    let index_start = input.len() - LZMA_STREAM_HEADER_SIZE - index_size;
    let records = parse_index_records(&input[index_start..input.len() - LZMA_STREAM_HEADER_SIZE])?;
    let total_output = records.iter().try_fold(0usize, |acc, record| {
        acc.checked_add(record.uncompressed_size as usize)
            .ok_or(crate::ffi::types::LZMA_DATA_ERROR)
    })?;

    let mut output = Vec::with_capacity(total_output);
    let mut block_start = LZMA_STREAM_HEADER_SIZE;

    for record in records {
        let (block_end, decoded, _) = unsafe {
            decode_validated_xz_block(
                input,
                block_start,
                index_start,
                header_flags.check,
                record,
                ignore_check || check::check_is_supported(header_flags.check) == 0,
            )
        }?;
        output.extend_from_slice(&decoded);
        block_start = block_end;
    }

    if block_start != index_start {
        return Err(crate::ffi::types::LZMA_DATA_ERROR);
    }

    Ok((input.len(), output))
}

fn xz_stream_check(input: &[u8]) -> Result<Option<lzma_check>, lzma_ret> {
    use crate::ffi::types::lzma_stream_flags;
    use crate::internal::stream_flags::{stream_header_decode_impl, LZMA_STREAM_HEADER_SIZE};

    if input.len() < LZMA_STREAM_HEADER_SIZE {
        return Ok(None);
    }

    let mut header_flags: lzma_stream_flags = unsafe { mem::zeroed() };
    let ret = unsafe { stream_header_decode_impl(&mut header_flags, input.as_ptr()) };
    if ret != LZMA_OK {
        return Err(ret);
    }

    Ok(Some(header_flags.check))
}

fn inspect_single_xz_stream(input: &[u8]) -> Result<(lzma_check, u64), lzma_ret> {
    use crate::ffi::types::lzma_stream_flags;
    use crate::internal::stream_flags::{
        stream_flags_compare_impl, stream_footer_decode_impl, stream_header_decode_impl,
        LZMA_STREAM_HEADER_SIZE,
    };

    if input.len() < LZMA_STREAM_HEADER_SIZE * 2 {
        return Err(crate::ffi::types::LZMA_DATA_ERROR);
    }

    let mut header_flags: lzma_stream_flags = unsafe { mem::zeroed() };
    let mut footer_flags: lzma_stream_flags = unsafe { mem::zeroed() };

    unsafe {
        let ret = stream_header_decode_impl(&mut header_flags, input.as_ptr());
        if ret != LZMA_OK {
            return Err(ret);
        }

        let ret = stream_footer_decode_impl(
            &mut footer_flags,
            input.as_ptr().add(input.len() - LZMA_STREAM_HEADER_SIZE),
        );
        if ret != LZMA_OK {
            return Err(ret);
        }

        let ret = stream_flags_compare_impl(&header_flags, &footer_flags);
        if ret != LZMA_OK {
            return Err(ret);
        }
    }

    let index_size = footer_flags.backward_size as usize;
    if input.len() < LZMA_STREAM_HEADER_SIZE + index_size + LZMA_STREAM_HEADER_SIZE {
        return Err(crate::ffi::types::LZMA_DATA_ERROR);
    }

    let index_start = input.len() - LZMA_STREAM_HEADER_SIZE - index_size;
    let records = parse_index_records(&input[index_start..input.len() - LZMA_STREAM_HEADER_SIZE])?;
    let mut block_start = LZMA_STREAM_HEADER_SIZE;
    let mut max_memusage = LZMA_MEMUSAGE_BASE;

    for record in records {
        let (block_end, _decoded, memusage) = unsafe {
            decode_validated_xz_block(
                input,
                block_start,
                index_start,
                header_flags.check,
                record,
                check::check_is_supported(header_flags.check) == 0,
            )
        }?;
        max_memusage = max_memusage.max(memusage);
        block_start = block_end;
    }

    if block_start != index_start {
        return Err(crate::ffi::types::LZMA_DATA_ERROR);
    }

    Ok((header_flags.check, max_memusage))
}

fn find_single_xz_stream_end(input: &[u8]) -> Result<usize, lzma_ret> {
    if input.len() < crate::internal::stream_flags::LZMA_STREAM_HEADER_SIZE * 2 {
        return Err(crate::ffi::types::LZMA_DATA_ERROR);
    }

    for end in crate::internal::stream_flags::LZMA_STREAM_HEADER_SIZE * 2..=input.len() {
        if input[end - 2..end] != *b"YZ" {
            continue;
        }

        if inspect_single_xz_stream(&input[..end]).is_ok() {
            return Ok(end);
        }
    }

    Err(crate::ffi::types::LZMA_DATA_ERROR)
}

fn inspect_xz_stream(input: &[u8], concatenated: bool) -> Result<(lzma_check, u64), lzma_ret> {
    if !concatenated {
        return inspect_single_xz_stream(input);
    }

    let mut offset = 0usize;
    let mut max_memusage = LZMA_MEMUSAGE_BASE;
    let mut first_check = crate::ffi::types::LZMA_CHECK_NONE;
    let mut saw_stream = false;
    let mut padding = 0usize;

    while offset < input.len() {
        while offset < input.len() && input[offset] == 0 {
            offset += 1;
            padding = (padding + 1) & 3;
        }

        if offset == input.len() {
            break;
        }

        if saw_stream && padding != 0 {
            return Err(crate::ffi::types::LZMA_DATA_ERROR);
        }

        let stream_len = find_single_xz_stream_end(&input[offset..])?;
        let (check, memusage) = inspect_single_xz_stream(&input[offset..offset + stream_len])?;
        if first_check == crate::ffi::types::LZMA_CHECK_NONE {
            first_check = check;
        }
        max_memusage = max_memusage.max(memusage);
        offset += stream_len;
        saw_stream = true;
        padding = 0;
    }

    if saw_stream && padding != 0 {
        return Err(crate::ffi::types::LZMA_DATA_ERROR);
    }

    Ok((first_check, max_memusage))
}

unsafe fn encode_block_with_filters(
    filters: *mut lzma_filter,
    check_id: lzma_check,
    input: &[u8],
) -> Result<(Vec<u8>, IndexRecord), lzma_ret> {
    let bound = block::block_buffer_bound(input.len());
    if bound == 0 {
        return Err(LZMA_PROG_ERROR);
    }

    let mut block_options: lzma_block = mem::zeroed();
    block_options.version = 1;
    block_options.check = check_id;
    block_options.filters = filters;

    let mut encoded = vec![0u8; bound];
    let mut pos = 0usize;
    let ret = block::block_buffer_encode(
        &mut block_options,
        ptr::null(),
        input.as_ptr(),
        input.len(),
        encoded.as_mut_ptr(),
        &mut pos,
        encoded.len(),
    );
    if ret != LZMA_OK {
        return Err(ret);
    }

    let record = IndexRecord {
        unpadded_size: block::block_unpadded_size(&block_options),
        uncompressed_size: input.len() as u64,
    };

    encoded.truncate(pos);
    Ok((encoded, record))
}

fn decode_xz_stream_once(
    input: &[u8],
    concatenated: bool,
    ignore_check: bool,
) -> Result<(usize, Vec<u8>), lzma_ret> {
    let cursor = Cursor::new(input);
    let mut reader = lzma_rust2::XzReader::new(cursor, concatenated);
    let mut output = Vec::new();
    match reader.read_to_end(&mut output) {
        Ok(_) => {
            let cursor = reader.into_inner();
            Ok((cursor.position() as usize, output))
        }
        Err(_) => decode_xz_stream_fallback(input, concatenated, ignore_check),
    }
}

fn decode_xz_stream_fallback(
    input: &[u8],
    concatenated: bool,
    ignore_check: bool,
) -> Result<(usize, Vec<u8>), lzma_ret> {
    if !concatenated {
        return decode_single_xz_stream_fallback(input, ignore_check);
    }

    let mut offset = 0usize;
    let mut output = Vec::new();
    let mut saw_stream = false;
    let mut padding = 0usize;

    while offset < input.len() {
        while offset < input.len() && input[offset] == 0 {
            offset += 1;
            padding = (padding + 1) & 3;
        }

        if offset == input.len() {
            break;
        }

        if saw_stream && padding != 0 {
            return Err(crate::ffi::types::LZMA_DATA_ERROR);
        }

        let stream_len = find_single_xz_stream_end(&input[offset..])?;
        let (consumed, decoded) =
            decode_single_xz_stream_fallback(&input[offset..offset + stream_len], ignore_check)?;
        debug_assert_eq!(consumed, stream_len);
        output.extend_from_slice(&decoded);
        offset += stream_len;
        saw_stream = true;
        padding = 0;
    }

    if saw_stream && padding != 0 {
        return Err(crate::ffi::types::LZMA_DATA_ERROR);
    }

    Ok((offset, output))
}

unsafe fn raw_code(
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
    let coder = &mut *coder.cast::<RawCoder>();
    match &mut coder.state {
        RawCoderState::Encoder(state) => {
            let copied = state.sink.copy_available(output, out_pos, out_size);
            if copied != 0 {
                if state.pending_stream_end
                    && state.sink.copy_available(output, out_pos, out_size) == 0
                {
                    if !state.finished {
                        state.pending_stream_end = false;
                    }
                    return LZMA_STREAM_END;
                }
                if *out_pos == out_size {
                    return LZMA_OK;
                }
            }

            if state.finished {
                return LZMA_STREAM_END;
            }

            if in_size != 0 {
                if input.is_null() {
                    return LZMA_PROG_ERROR;
                }
                let input_slice = core::slice::from_raw_parts(input, in_size);
                if let Err(error) = state.writer.as_mut().unwrap().write_all(input_slice) {
                    return lzma::io_error_to_ret(&error);
                }
                *in_pos = in_size;
            }

            match action {
                LZMA_RUN => {}
                LZMA_SYNC_FLUSH => {
                    if !state.supports_sync_flush {
                        return LZMA_OPTIONS_ERROR;
                    }
                    if let Err(error) = state.writer.as_mut().unwrap().flush() {
                        return lzma::io_error_to_ret(&error);
                    }
                    state.pending_stream_end = true;
                }
                LZMA_FINISH => {
                    if let Err(ret) = state.writer.take().unwrap().finish() {
                        return ret;
                    }
                    state.finished = true;
                    state.pending_stream_end = true;
                }
                _ => return LZMA_PROG_ERROR,
            }

            let copied = state.sink.copy_available(output, out_pos, out_size);
            if state.pending_stream_end && copied == 0 {
                if !state.finished {
                    state.pending_stream_end = false;
                }
                LZMA_STREAM_END
            } else {
                LZMA_OK
            }
        }
        RawCoderState::Decoder(state) => {
            if state.pending_pos < state.pending.len() {
                let ret = copy_output(
                    &state.pending,
                    &mut state.pending_pos,
                    output,
                    out_pos,
                    out_size,
                );
                if state.pending_pos == state.pending.len() {
                    state.pending.clear();
                    state.pending_pos = 0;
                } else {
                    return LZMA_OK;
                }
                if ret == LZMA_STREAM_END && state.stream_finished {
                    return LZMA_STREAM_END;
                }
                if *out_pos == out_size {
                    return LZMA_OK;
                }
            }

            if in_size != 0 {
                if input.is_null() {
                    return LZMA_PROG_ERROR;
                }
                state
                    .source
                    .append(core::slice::from_raw_parts(input, in_size));
                *in_pos = in_size;
            }

            if action == LZMA_FINISH && !state.finished_input {
                state.source.finish();
                state.finished_input = true;
            }

            if state.stream_finished {
                return LZMA_STREAM_END;
            }

            let target = (out_size - *out_pos).max(1);
            while state.pending.len().saturating_sub(state.pending_pos) < target
                && !state.stream_finished
            {
                let read_len = (target - state.pending.len().saturating_sub(state.pending_pos))
                    .max(1)
                    .min(8192);
                let mut temp = vec![0u8; read_len];
                match state.reader.read(&mut temp) {
                    Ok(0) => {
                        state.stream_finished = true;
                        break;
                    }
                    Ok(read) => state.pending.extend_from_slice(&temp[..read]),
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                    Err(error) => return lzma::io_error_to_ret(&error),
                }
            }

            if state.finished_input
                && !state.stream_finished
                && state.pending.len().saturating_sub(state.pending_pos) == target
            {
                let mut lookahead = [0u8; 1];
                match state.reader.read(&mut lookahead) {
                    Ok(0) => state.stream_finished = true,
                    Ok(1) => state.pending.push(lookahead[0]),
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
                    Err(error) => return lzma::io_error_to_ret(&error),
                    _ => unreachable!(),
                }
            }

            if state.pending_pos < state.pending.len() {
                let ret = copy_output(
                    &state.pending,
                    &mut state.pending_pos,
                    output,
                    out_pos,
                    out_size,
                );
                if state.pending_pos == state.pending.len() {
                    state.pending.clear();
                    state.pending_pos = 0;
                }
                if ret == LZMA_STREAM_END && state.stream_finished {
                    LZMA_STREAM_END
                } else {
                    LZMA_OK
                }
            } else if state.stream_finished {
                LZMA_STREAM_END
            } else {
                LZMA_OK
            }
        }
    }
}

unsafe fn raw_end(coder: *mut c_void, _allocator: *const lzma_allocator) {
    let mut coder = Box::from_raw(coder.cast::<RawCoder>());
    free_filters(&mut coder.filters);
}

unsafe fn stream_encoder_code(
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
    let coder = &mut *coder.cast::<StreamEncoderCoder>();
    if coder.pending_pos < coder.pending.len() {
        return copy_output(
            &coder.pending,
            &mut coder.pending_pos,
            output,
            out_pos,
            out_size,
        );
    }
    if !coder.pending.is_empty() {
        coder.pending.clear();
        coder.pending_pos = 0;
    }

    if in_size != 0 {
        coder
            .input
            .extend_from_slice(core::slice::from_raw_parts(input, in_size));
        *in_pos = in_size;
    }

    if coder.finished {
        return LZMA_STREAM_END;
    }

    match action {
        crate::internal::common::LZMA_RUN => return LZMA_OK,
        crate::internal::common::LZMA_FULL_FLUSH | crate::internal::common::LZMA_FULL_BARRIER => {
            if coder.input.is_empty() {
                return LZMA_STREAM_END;
            }
            if !coder.header_written {
                write_xz_stream_header(coder.check, &mut coder.pending);
                coder.header_written = true;
            }
            let (encoded, record) = match encode_block_with_filters(
                coder.filters.as_mut_ptr(),
                coder.check,
                &coder.input,
            ) {
                Ok(result) => result,
                Err(ret) => return ret,
            };
            coder.records.push(record);
            coder.pending.extend_from_slice(&encoded);
            coder.pending_pos = 0;
            coder.input.clear();
            copy_output(
                &coder.pending,
                &mut coder.pending_pos,
                output,
                out_pos,
                out_size,
            )
        }
        crate::internal::common::LZMA_FINISH => {
            if !coder.header_written {
                write_xz_stream_header(coder.check, &mut coder.pending);
                coder.header_written = true;
            }
            if !coder.input.is_empty() {
                let (encoded, record) = match encode_block_with_filters(
                    coder.filters.as_mut_ptr(),
                    coder.check,
                    &coder.input,
                ) {
                    Ok(result) => result,
                    Err(ret) => return ret,
                };
                coder.records.push(record);
                coder.pending.extend_from_slice(&encoded);
                coder.input.clear();
            }

            let index = encode_xz_index(&coder.records);
            let backward_size = (index.len() / 4 - 1) as u32;
            coder.pending.extend_from_slice(&index);
            write_xz_stream_footer(coder.check, backward_size, &mut coder.pending);
            coder.pending_pos = 0;
            coder.finished = true;
            copy_output(
                &coder.pending,
                &mut coder.pending_pos,
                output,
                out_pos,
                out_size,
            )
        }
        _ => LZMA_PROG_ERROR,
    }
}

unsafe fn stream_encoder_end(coder: *mut c_void, _allocator: *const lzma_allocator) {
    let mut coder = Box::from_raw(coder.cast::<StreamEncoderCoder>());
    free_filters(&mut coder.filters);
}

unsafe fn stream_get_check(coder: *const c_void) -> lzma_check {
    (*(coder.cast::<StreamEncoderCoder>())).check
}

unsafe fn stream_decoder_get_check(coder: *const c_void) -> lzma_check {
    (*(coder.cast::<StreamDecoderCoder>())).check
}

unsafe fn stream_decoder_code(
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
    let coder = &mut *coder.cast::<StreamDecoderCoder>();
    if coder.output_pos < coder.output.len() {
        return copy_output(
            &coder.output,
            &mut coder.output_pos,
            output,
            out_pos,
            out_size,
        );
    }

    if coder.decoded {
        return LZMA_STREAM_END;
    }

    let input_slice = if in_size == 0 {
        &[][..]
    } else {
        core::slice::from_raw_parts(input, in_size)
    };
    let mut consumed_input = 0usize;

    if !coder.header_parsed {
        let header_size = crate::internal::stream_flags::LZMA_STREAM_HEADER_SIZE;
        let take = header_size
            .saturating_sub(coder.input.len())
            .min(input_slice.len());
        if take != 0 {
            coder.input.extend_from_slice(&input_slice[..take]);
            consumed_input = take;
        }

        match xz_stream_check(&coder.input) {
            Ok(Some(check_id)) => {
                coder.check = check_id;
                coder.header_parsed = true;
                coder.pending_ret = if (coder.flags & LZMA_TELL_NO_CHECK) != 0
                    && check_id == crate::ffi::types::LZMA_CHECK_NONE
                {
                    LZMA_NO_CHECK
                } else if (coder.flags & LZMA_TELL_UNSUPPORTED_CHECK) != 0
                    && check::check_is_supported(check_id) == 0
                {
                    LZMA_UNSUPPORTED_CHECK
                } else if (coder.flags & LZMA_TELL_ANY_CHECK) != 0 {
                    LZMA_GET_CHECK
                } else {
                    LZMA_OK
                };
            }
            Ok(None) => {
                *in_pos = consumed_input;
                return if action == crate::internal::common::LZMA_FINISH {
                    LZMA_BUF_ERROR
                } else {
                    LZMA_OK
                };
            }
            Err(ret) => {
                *in_pos = consumed_input;
                return if action == crate::internal::common::LZMA_FINISH
                    && ret == crate::ffi::types::LZMA_DATA_ERROR
                {
                    LZMA_BUF_ERROR
                } else if action == crate::internal::common::LZMA_FINISH {
                    ret
                } else {
                    LZMA_OK
                };
            }
        }
    }

    if coder.pending_ret != LZMA_OK {
        let ret = coder.pending_ret;
        coder.pending_ret = LZMA_OK;
        *in_pos = consumed_input;
        return ret;
    }

    if consumed_input < input_slice.len() {
        coder.input.extend_from_slice(&input_slice[consumed_input..]);
        consumed_input = input_slice.len();
    }
    *in_pos = consumed_input;

    let concatenated = (coder.flags & LZMA_CONCATENATED) != 0;
    if coder.memusage == LZMA_MEMUSAGE_BASE
        || concatenated
        || action == crate::internal::common::LZMA_FINISH
    {
        match inspect_xz_stream(&coder.input, concatenated) {
            Ok((check_id, memusage)) => {
                coder.check = check_id;
                coder.memusage = memusage.max(LZMA_MEMUSAGE_BASE);
            }
            Err(ret) => {
                return if action == crate::internal::common::LZMA_FINISH {
                    ret
                } else {
                    LZMA_OK
                };
            }
        }
    }

    if coder.memusage > coder.memlimit.max(1) {
        return crate::ffi::types::LZMA_MEMLIMIT_ERROR;
    }

    match decode_xz_stream_once(
        &coder.input,
        (coder.flags & LZMA_CONCATENATED) != 0,
        (coder.flags & LZMA_IGNORE_CHECK) != 0,
    ) {
        Ok((_consumed, decoded)) => {
            coder.output = decoded;
            coder.output_pos = 0;
            coder.decoded = true;
            copy_output(
                &coder.output,
                &mut coder.output_pos,
                output,
                out_pos,
                out_size,
            )
        }
        Err(ret) => {
            if action == crate::internal::common::LZMA_FINISH {
                if ret == crate::ffi::types::LZMA_DATA_ERROR {
                    LZMA_BUF_ERROR
                } else {
                    ret
                }
            } else {
                let _ = coder.memlimit;
                LZMA_OK
            }
        }
    }
}

unsafe fn stream_decoder_end(coder: *mut c_void, _allocator: *const lzma_allocator) {
    drop(Box::from_raw(coder.cast::<StreamDecoderCoder>()));
}

unsafe fn stream_decoder_memconfig(
    coder: *mut c_void,
    memusage: *mut u64,
    old_memlimit: *mut u64,
    new_memlimit: u64,
) -> lzma_ret {
    let coder = &mut *coder.cast::<StreamDecoderCoder>();
    *memusage = coder.memusage.max(LZMA_MEMUSAGE_BASE);
    *old_memlimit = coder.memlimit.max(1);
    if new_memlimit != 0 {
        let new_memlimit = new_memlimit.max(1);
        if new_memlimit < coder.memusage.max(LZMA_MEMUSAGE_BASE) {
            return crate::ffi::types::LZMA_MEMLIMIT_ERROR;
        }
        coder.memlimit = new_memlimit;
    }
    LZMA_OK
}

pub(crate) unsafe fn raw_buffer_encode(
    filters: *const lzma_filter,
    _allocator: *const lzma_allocator,
    input: *const u8,
    input_size: usize,
    output: *mut u8,
    output_pos: *mut usize,
    output_size: usize,
) -> lzma_ret {
    if filters.is_null() || output.is_null() || output_pos.is_null() || *output_pos > output_size {
        return LZMA_PROG_ERROR;
    }
    let chain = match lzma::parse_filters(filters) {
        Ok(chain) => chain,
        Err(ret) => return ret,
    };

    let input_slice = if input_size == 0 {
        &[]
    } else if input.is_null() {
        return LZMA_PROG_ERROR;
    } else {
        core::slice::from_raw_parts(input, input_size)
    };

    let encoded = match lzma::encode_raw(&chain, input_slice) {
        Ok(encoded) => encoded,
        Err(ret) => return ret,
    };
    if output_size - *output_pos < encoded.len() {
        return LZMA_BUF_ERROR;
    }
    ptr::copy_nonoverlapping(encoded.as_ptr(), output.add(*output_pos), encoded.len());
    *output_pos += encoded.len();
    LZMA_OK
}

pub(crate) unsafe fn raw_buffer_decode(
    filters: *const lzma_filter,
    _allocator: *const lzma_allocator,
    input: *const u8,
    input_pos: *mut usize,
    input_size: usize,
    output: *mut u8,
    output_pos: *mut usize,
    output_size: usize,
) -> lzma_ret {
    if filters.is_null()
        || input.is_null()
        || input_pos.is_null()
        || *input_pos > input_size
        || output_pos.is_null()
        || (output.is_null() && *output_pos != output_size)
        || *output_pos > output_size
    {
        return LZMA_PROG_ERROR;
    }

    let chain = match lzma::parse_filters(filters) {
        Ok(chain) => chain,
        Err(ret) => return ret,
    };
    let input_slice = core::slice::from_raw_parts(input.add(*input_pos), input_size - *input_pos);
    let (decoded, consumed) = match lzma::decode_raw(&chain, input_slice) {
        Ok(decoded) => decoded,
        Err(ret) => return ret,
    };
    if output_size - *output_pos < decoded.len() {
        return LZMA_BUF_ERROR;
    }
    ptr::copy_nonoverlapping(decoded.as_ptr(), output.add(*output_pos), decoded.len());
    *output_pos += decoded.len();
    *input_pos += consumed;
    LZMA_OK
}

pub(crate) unsafe fn raw_encoder(strm: *mut lzma_stream, filters: *const lzma_filter) -> lzma_ret {
    if strm.is_null() {
        return LZMA_PROG_ERROR;
    }
    let mut copied = match copy_filters(filters) {
        Ok(copied) => copied,
        Err(ret) => return ret,
    };
    let chain = match lzma::parse_filters(copied.as_ptr()) {
        Ok(chain) => chain,
        Err(ret) => {
            free_filters(&mut copied);
            return ret;
        }
    };
    let sink = SharedSink::default();
    let (writer, supports_sync_flush) = match build_raw_encoder(&chain, sink.clone()) {
        Ok(writer) => writer,
        Err(ret) => {
            free_filters(&mut copied);
            return ret;
        }
    };
    install_next_coder(
        strm,
        NextCoder {
            coder: Box::into_raw(Box::new(RawCoder {
                filters: copied,
                state: RawCoderState::Encoder(RawEncoderState {
                    writer: Some(writer),
                    sink,
                    supports_sync_flush,
                    finished: false,
                    pending_stream_end: false,
                }),
            }))
            .cast(),
            code: raw_code,
            end: Some(raw_end),
            get_progress: None,
            get_check: None,
            memconfig: None,
            update: None,
        },
        raw_encoder_actions(),
    )
}

pub(crate) unsafe fn raw_decoder(strm: *mut lzma_stream, filters: *const lzma_filter) -> lzma_ret {
    if strm.is_null() {
        return LZMA_PROG_ERROR;
    }
    let mut copied = match copy_filters(filters) {
        Ok(copied) => copied,
        Err(ret) => return ret,
    };
    let chain = match lzma::parse_filters(copied.as_ptr()) {
        Ok(chain) => chain,
        Err(ret) => {
            free_filters(&mut copied);
            return ret;
        }
    };
    let source = SharedSource::default();
    let reader = match build_raw_decoder(&chain, source.clone()) {
        Ok(reader) => reader,
        Err(ret) => {
            free_filters(&mut copied);
            return ret;
        }
    };
    install_next_coder(
        strm,
        NextCoder {
            coder: Box::into_raw(Box::new(RawCoder {
                filters: copied,
                state: RawCoderState::Decoder(RawDecoderState {
                    reader,
                    source,
                    pending: Vec::new(),
                    pending_pos: 0,
                    finished_input: false,
                    stream_finished: false,
                }),
            }))
            .cast(),
            code: raw_code,
            end: Some(raw_end),
            get_progress: None,
            get_check: None,
            memconfig: None,
            update: None,
        },
        raw_decoder_actions(),
    )
}

pub(crate) unsafe fn raw_encoder_memusage(filters: *const lzma_filter) -> u64 {
    lzma::encoder_memusage(filters)
}

pub(crate) unsafe fn raw_decoder_memusage(filters: *const lzma_filter) -> u64 {
    lzma::decoder_memusage(filters)
}

pub(crate) unsafe fn stream_buffer_bound(uncompressed_size: usize) -> usize {
    block::block_buffer_bound(uncompressed_size).saturating_add(64)
}

pub(crate) unsafe fn stream_buffer_encode(
    filters: *mut lzma_filter,
    check_id: lzma_check,
    _allocator: *const lzma_allocator,
    input: *const u8,
    input_size: usize,
    output: *mut u8,
    output_pos: *mut usize,
    output_size: usize,
) -> lzma_ret {
    if filters.is_null() || output.is_null() || output_pos.is_null() || *output_pos > output_size {
        return LZMA_PROG_ERROR;
    }
    if check::check_is_supported(check_id) == 0 {
        return LZMA_UNSUPPORTED_CHECK;
    }
    let input_slice = if input_size == 0 {
        &[]
    } else if input.is_null() {
        return LZMA_PROG_ERROR;
    } else {
        core::slice::from_raw_parts(input, input_size)
    };

    let mut temp = Vec::new();
    write_xz_stream_header(check_id, &mut temp);
    let (block_data, record) = match encode_block_with_filters(filters, check_id, input_slice) {
        Ok(result) => result,
        Err(ret) => return ret,
    };
    temp.extend_from_slice(&block_data);
    let index = encode_xz_index(&[record]);
    let backward_size = (index.len() / 4 - 1) as u32;
    temp.extend_from_slice(&index);
    write_xz_stream_footer(check_id, backward_size, &mut temp);

    if output_size - *output_pos < temp.len() {
        return LZMA_BUF_ERROR;
    }
    ptr::copy_nonoverlapping(temp.as_ptr(), output.add(*output_pos), temp.len());
    *output_pos += temp.len();
    LZMA_OK
}

pub(crate) unsafe fn stream_buffer_decode(
    _memlimit: *mut u64,
    flags: u32,
    _allocator: *const lzma_allocator,
    input: *const u8,
    input_pos: *mut usize,
    input_size: usize,
    output: *mut u8,
    output_pos: *mut usize,
    output_size: usize,
) -> lzma_ret {
    if input.is_null()
        || input_pos.is_null()
        || *input_pos > input_size
        || output_pos.is_null()
        || (output.is_null() && *output_pos != output_size)
        || *output_pos > output_size
    {
        return LZMA_PROG_ERROR;
    }

    let (consumed, decoded) = match decode_xz_stream_once(
        core::slice::from_raw_parts(input.add(*input_pos), input_size - *input_pos),
        (flags & LZMA_CONCATENATED) != 0,
        (flags & LZMA_IGNORE_CHECK) != 0,
    ) {
        Ok(result) => result,
        Err(ret) => return ret,
    };
    if output_size - *output_pos < decoded.len() {
        return LZMA_BUF_ERROR;
    }
    ptr::copy_nonoverlapping(decoded.as_ptr(), output.add(*output_pos), decoded.len());
    *output_pos += decoded.len();
    *input_pos += consumed;
    LZMA_OK
}

pub(crate) unsafe fn stream_encoder(
    strm: *mut lzma_stream,
    filters: *const lzma_filter,
    check_id: lzma_check,
) -> lzma_ret {
    if strm.is_null() {
        return LZMA_PROG_ERROR;
    }
    if check::check_is_supported(check_id) == 0 {
        return LZMA_UNSUPPORTED_CHECK;
    }
    let copied = match copy_filters(filters) {
        Ok(copied) => copied,
        Err(ret) => return ret,
    };
    install_next_coder(
        strm,
        NextCoder {
            coder: Box::into_raw(Box::new(StreamEncoderCoder {
                magic: STREAM_ENCODER_MAGIC,
                filters: copied,
                check: check_id,
                input: Vec::new(),
                pending: Vec::new(),
                pending_pos: 0,
                records: Vec::new(),
                header_written: false,
                finished: false,
            }))
            .cast(),
            code: stream_encoder_code,
            end: Some(stream_encoder_end),
            get_progress: None,
            get_check: Some(stream_get_check),
            memconfig: None,
            update: None,
        },
        all_supported_actions(),
    )
}

pub(crate) unsafe fn stream_decoder(strm: *mut lzma_stream, memlimit: u64, flags: u32) -> lzma_ret {
    if strm.is_null() {
        return LZMA_PROG_ERROR;
    }
    if (flags & !STREAM_DECODER_SUPPORTED_FLAGS) != 0 {
        return LZMA_OPTIONS_ERROR;
    }
    install_next_coder(
        strm,
        NextCoder {
            coder: Box::into_raw(Box::new(StreamDecoderCoder {
                input: Vec::new(),
                output: Vec::new(),
                output_pos: 0,
                memlimit: memlimit.max(1),
                memusage: LZMA_MEMUSAGE_BASE,
                flags,
                check: crate::ffi::types::LZMA_CHECK_NONE,
                pending_ret: LZMA_OK,
                header_parsed: false,
                decoded: false,
            }))
            .cast(),
            code: stream_decoder_code,
            end: Some(stream_decoder_end),
            get_progress: None,
            get_check: Some(stream_decoder_get_check),
            memconfig: Some(stream_decoder_memconfig),
            update: None,
        },
        all_supported_actions(),
    )
}

pub(crate) unsafe fn filters_update(
    strm: *mut lzma_stream,
    filters: *const lzma_filter,
) -> lzma_ret {
    let Some(next) = current_next_coder(strm) else {
        return LZMA_PROG_ERROR;
    };
    if next.code as *const () as usize != stream_encoder_code as *const () as usize {
        return LZMA_PROG_ERROR;
    }
    let coder = &mut *next.coder.cast::<StreamEncoderCoder>();
    if coder.magic != STREAM_ENCODER_MAGIC
        || !coder.input.is_empty()
        || coder.pending_pos < coder.pending.len()
        || coder.finished
    {
        return LZMA_PROG_ERROR;
    }

    let new_filters = match copy_filters(filters) {
        Ok(filters) => filters,
        Err(ret) => return ret,
    };
    free_filters(&mut coder.filters);
    coder.filters = new_filters;
    LZMA_OK
}

pub(crate) unsafe fn easy_encoder(
    strm: *mut lzma_stream,
    preset_id: u32,
    check_id: lzma_check,
) -> lzma_ret {
    let mut options: lzma_options_lzma = mem::zeroed();
    if preset::lzma_lzma_preset_impl(&mut options, preset_id) != 0 {
        return LZMA_OPTIONS_ERROR;
    }
    let filters = [
        lzma_filter {
            id: crate::internal::filter::common::LZMA_FILTER_LZMA2,
            options: (&mut options as *mut lzma_options_lzma).cast(),
        },
        lzma_filter {
            id: LZMA_VLI_UNKNOWN,
            options: ptr::null_mut(),
        },
    ];
    stream_encoder(strm, filters.as_ptr(), check_id)
}

pub(crate) unsafe fn easy_buffer_encode(
    preset_id: u32,
    check_id: lzma_check,
    allocator: *const lzma_allocator,
    input: *const u8,
    input_size: usize,
    output: *mut u8,
    output_pos: *mut usize,
    output_size: usize,
) -> lzma_ret {
    let mut options: lzma_options_lzma = mem::zeroed();
    if preset::lzma_lzma_preset_impl(&mut options, preset_id) != 0 {
        return LZMA_OPTIONS_ERROR;
    }
    let mut filters = [
        lzma_filter {
            id: crate::internal::filter::common::LZMA_FILTER_LZMA2,
            options: (&mut options as *mut lzma_options_lzma).cast(),
        },
        lzma_filter {
            id: LZMA_VLI_UNKNOWN,
            options: ptr::null_mut(),
        },
    ];
    stream_buffer_encode(
        filters.as_mut_ptr(),
        check_id,
        allocator,
        input,
        input_size,
        output,
        output_pos,
        output_size,
    )
}

pub(crate) unsafe fn easy_encoder_memusage(preset_id: u32) -> u64 {
    let mut options: lzma_options_lzma = mem::zeroed();
    if preset::lzma_lzma_preset_impl(&mut options, preset_id & LZMA_PRESET_LEVEL_MASK) != 0 {
        return u64::from(u32::MAX);
    }
    let filters = [
        lzma_filter {
            id: crate::internal::filter::common::LZMA_FILTER_LZMA2,
            options: (&mut options as *mut lzma_options_lzma).cast(),
        },
        lzma_filter {
            id: LZMA_VLI_UNKNOWN,
            options: ptr::null_mut(),
        },
    ];
    lzma::encoder_memusage(filters.as_ptr())
}

pub(crate) unsafe fn easy_decoder_memusage(preset_id: u32) -> u64 {
    let mut options: lzma_options_lzma = mem::zeroed();
    if preset::lzma_lzma_preset_impl(&mut options, preset_id & LZMA_PRESET_LEVEL_MASK) != 0 {
        return u64::from(u32::MAX);
    }
    let filters = [
        lzma_filter {
            id: crate::internal::filter::common::LZMA_FILTER_LZMA2,
            options: (&mut options as *mut lzma_options_lzma).cast(),
        },
        lzma_filter {
            id: LZMA_VLI_UNKNOWN,
            options: ptr::null_mut(),
        },
    ];
    lzma::decoder_memusage(filters.as_ptr())
}

pub(crate) unsafe fn auto_decoder(strm: *mut lzma_stream, memlimit: u64, flags: u32) -> lzma_ret {
    stream_decoder(strm, memlimit, flags)
}

pub(crate) unsafe fn stream_decoder_mt(
    strm: *mut lzma_stream,
    options: *const lzma_mt,
) -> lzma_ret {
    if strm.is_null() || options.is_null() {
        return LZMA_PROG_ERROR;
    }

    let options = &*options;
    if options.threads == 0 || options.threads > LZMA_THREADS_MAX {
        return LZMA_OPTIONS_ERROR;
    }
    if (options.flags & !STREAM_DECODER_SUPPORTED_FLAGS) != 0 {
        return LZMA_OPTIONS_ERROR;
    }

    stream_decoder(strm, options.memlimit_stop.max(1), options.flags)
}

#[cfg(test)]
mod tests {
    use core::{mem, ptr};

    use super::*;
    use crate::ffi::types::{lzma_filter, lzma_mt, lzma_options_lzma, LZMA_STREAM_INIT};
    use crate::internal::{
        common::LZMA_CHECK_CRC32,
        filter::common::LZMA_FILTER_LZMA2,
        stream_state::{lzma_code_impl, lzma_end_impl},
    };

    unsafe fn lzma2_filters(
        options: &mut lzma_options_lzma,
    ) -> [lzma_filter; crate::ffi::types::LZMA_FILTERS_MAX + 1] {
        assert_eq!(preset::lzma_lzma_preset_impl(options, 6), 0);
        [
            lzma_filter {
                id: LZMA_FILTER_LZMA2,
                options: (options as *mut lzma_options_lzma).cast(),
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
        ]
    }

    unsafe fn pump(
        strm: &mut lzma_stream,
        action: lzma_action,
        output_chunk: usize,
    ) -> (lzma_ret, Vec<u8>) {
        let mut output = Vec::new();
        loop {
            let mut buffer = vec![0u8; output_chunk];
            strm.next_out = buffer.as_mut_ptr();
            strm.avail_out = buffer.len();
            let ret = lzma_code_impl(strm, action);
            let written = buffer.len() - strm.avail_out;
            output.extend_from_slice(&buffer[..written]);

            if ret == LZMA_STREAM_END {
                return (ret, output);
            }
            if ret != LZMA_OK {
                return (ret, output);
            }
            if action == LZMA_RUN && strm.avail_in == 0 {
                return (ret, output);
            }
        }
    }

    #[test]
    fn raw_encoder_sync_flush_emits_midstream_bytes() {
        let part1 = vec![b'A'; 16 * 1024];
        let part2 = vec![b'B'; 8 * 1024];
        let mut encoded = Vec::new();

        unsafe {
            let mut options: lzma_options_lzma = mem::zeroed();
            let filters = lzma2_filters(&mut options);
            let mut strm = LZMA_STREAM_INIT;
            assert_eq!(raw_encoder(&mut strm, filters.as_ptr()), LZMA_OK);

            strm.next_in = part1.as_ptr();
            strm.avail_in = part1.len();
            let (ret, run_bytes) = pump(&mut strm, LZMA_RUN, 257);
            assert_eq!(ret, LZMA_OK);
            encoded.extend_from_slice(&run_bytes);

            strm.next_in = ptr::null();
            strm.avail_in = 0;
            let (ret, flush_bytes) = pump(&mut strm, LZMA_SYNC_FLUSH, 257);
            assert_eq!(ret, LZMA_STREAM_END);
            assert!(!flush_bytes.is_empty());
            encoded.extend_from_slice(&flush_bytes);

            strm.next_in = part2.as_ptr();
            strm.avail_in = part2.len();
            let (ret, finish_bytes) = pump(&mut strm, LZMA_FINISH, 257);
            assert_eq!(ret, LZMA_STREAM_END);
            encoded.extend_from_slice(&finish_bytes);
            lzma_end_impl(&mut strm);

            let mut decode_options: lzma_options_lzma = mem::zeroed();
            let decode_filters = lzma2_filters(&mut decode_options);
            let mut decode = LZMA_STREAM_INIT;
            assert_eq!(raw_decoder(&mut decode, decode_filters.as_ptr()), LZMA_OK);

            decode.next_in = encoded.as_ptr();
            decode.avail_in = encoded.len();
            let (ret, decoded) = pump(&mut decode, LZMA_FINISH, 1024);
            assert_eq!(ret, LZMA_STREAM_END);
            assert_eq!(decoded, [part1, part2].concat());
            lzma_end_impl(&mut decode);
        }
    }

    #[test]
    fn empty_xz_stream_with_zero_index_records_is_valid() {
        let input = include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/upstream/files/good-0-empty.xz"
        ));

        let (check_id, memusage) = inspect_single_xz_stream(input).expect("inspect empty .xz");
        assert_eq!(check_id, LZMA_CHECK_CRC32);
        assert!(memusage >= LZMA_MEMUSAGE_BASE);

        let (consumed, fallback_output) =
            decode_single_xz_stream_fallback(input, false).expect("fallback decode empty .xz");
        assert_eq!(consumed, input.len());
        assert!(fallback_output.is_empty());

        let (consumed, decoded) =
            decode_xz_stream_once(input, false, false).expect("decode empty .xz");
        assert_eq!(consumed, input.len());
        assert!(decoded.is_empty());
    }

    #[test]
    fn stream_decoder_accepts_empty_xz_stream() {
        let input = include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/upstream/files/good-0-empty.xz"
        ));

        unsafe {
            let mut strm = LZMA_STREAM_INIT;
            assert_eq!(stream_decoder(&mut strm, u64::MAX, 0), LZMA_OK);

            strm.next_in = input.as_ptr();
            strm.avail_in = input.len();
            let (ret, output) = pump(&mut strm, LZMA_FINISH, 32);
            assert_eq!(ret, LZMA_STREAM_END);
            assert!(output.is_empty());
            lzma_end_impl(&mut strm);
        }
    }

    #[test]
    fn stream_decoder_keeps_body_input_after_check_notification() {
        let input = include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/upstream/files/good-1-3delta-lzma2.xz"
        ));

        unsafe {
            let mut strm = LZMA_STREAM_INIT;
            assert_eq!(
                stream_decoder(&mut strm, u64::MAX, LZMA_TELL_ANY_CHECK | LZMA_TELL_NO_CHECK),
                LZMA_OK
            );

            let mut output = [0u8; 4096];
            strm.next_in = input.as_ptr();
            strm.avail_in = input.len();
            strm.next_out = output.as_mut_ptr();
            strm.avail_out = output.len();
            assert_eq!(lzma_code_impl(&mut strm, LZMA_RUN), LZMA_GET_CHECK);
            assert_eq!(
                strm.avail_in,
                input.len() - crate::internal::stream_flags::LZMA_STREAM_HEADER_SIZE
            );
            assert_eq!(strm.total_out, 0);

            strm.next_out = output.as_mut_ptr();
            strm.avail_out = output.len();
            let ret = lzma_code_impl(&mut strm, LZMA_RUN);
            assert!(ret == LZMA_OK || ret == LZMA_STREAM_END);
            assert!(strm.total_out > 0);
            lzma_end_impl(&mut strm);
        }
    }

    #[test]
    fn inspect_rejects_bad_index_size_records() {
        let bad_unpadded = include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/upstream/files/bad-2-index-1.xz"
        ));
        let bad_uncompressed = include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/upstream/files/bad-2-index-2.xz"
        ));

        assert_eq!(
            inspect_single_xz_stream(bad_unpadded).unwrap_err(),
            crate::ffi::types::LZMA_DATA_ERROR
        );
        assert_eq!(
            inspect_single_xz_stream(bad_uncompressed).unwrap_err(),
            crate::ffi::types::LZMA_DATA_ERROR
        );
    }

    #[test]
    fn three_delta_lzma2_stream_is_accepted() {
        let input = include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/upstream/files/good-1-3delta-lzma2.xz"
        ));

        let (consumed, decoded) =
            decode_xz_stream_once(input, false, false).expect("decode three-delta .xz");
        assert_eq!(consumed, input.len());
        assert!(!decoded.is_empty());

        let (_check_id, memusage) =
            inspect_single_xz_stream(input).expect("inspect three-delta .xz");
        assert!(memusage >= LZMA_MEMUSAGE_BASE);
    }

    #[test]
    fn mt_decoder_warns_then_decodes_unsupported_check_stream() {
        let input = include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/upstream/files/unsupported-check.xz"
        ));

        unsafe {
            let mut options: lzma_mt = mem::zeroed();
            options.flags = LZMA_TELL_UNSUPPORTED_CHECK | LZMA_CONCATENATED;
            options.memlimit_stop = u64::MAX;
            options.memlimit_threading = 0;
            options.threads = 2;

            let mut strm = LZMA_STREAM_INIT;
            assert_eq!(stream_decoder_mt(&mut strm, &options), LZMA_OK);

            strm.next_in = input.as_ptr();
            strm.avail_in = input.len();
            strm.next_out = ptr::null_mut();
            strm.avail_out = 0;
            assert_eq!(lzma_code_impl(&mut strm, LZMA_RUN), LZMA_UNSUPPORTED_CHECK);
            assert_eq!(lzma_code_impl(&mut strm, LZMA_RUN), LZMA_OK);

            let mut output = [0u8; 64];
            strm.next_out = output.as_mut_ptr();
            strm.avail_out = output.len();
            assert_eq!(lzma_code_impl(&mut strm, LZMA_FINISH), LZMA_STREAM_END);
            assert_eq!(strm.total_out, 13);
            lzma_end_impl(&mut strm);
        }
    }

    #[test]
    fn inspect_rejects_bad_lzma2_stream_without_initial_dict_reset() {
        let input = include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/upstream/files/bad-1-lzma2-1.xz"
        ));

        let ret = inspect_single_xz_stream(input).unwrap_err();
        assert!(ret == crate::ffi::types::LZMA_DATA_ERROR || ret == LZMA_OPTIONS_ERROR);
    }
}
