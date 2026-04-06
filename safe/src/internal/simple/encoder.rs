use std::io::{Cursor, Write};

use lzma_rust2::filter::bcj::BcjWriter;

use super::common::SimpleFilterKind;
use crate::ffi::types::{lzma_ret, LZMA_PROG_ERROR};

pub(crate) fn encode_all(
    kind: SimpleFilterKind,
    start_offset: u32,
    input: &[u8],
) -> Result<Vec<u8>, lzma_ret> {
    let sink = Cursor::new(Vec::with_capacity(input.len()));
    let mut writer = match kind {
        SimpleFilterKind::X86 => BcjWriter::new_x86(sink, start_offset as usize),
        SimpleFilterKind::PowerPc => BcjWriter::new_ppc(sink, start_offset as usize),
        SimpleFilterKind::Ia64 => BcjWriter::new_ia64(sink, start_offset as usize),
        SimpleFilterKind::Arm => BcjWriter::new_arm(sink, start_offset as usize),
        SimpleFilterKind::ArmThumb => BcjWriter::new_arm_thumb(sink, start_offset as usize),
        SimpleFilterKind::Arm64 => BcjWriter::new_arm64(sink, start_offset as usize),
        SimpleFilterKind::Sparc => BcjWriter::new_sparc(sink, start_offset as usize),
    };

    writer.write_all(input).map_err(|_| LZMA_PROG_ERROR)?;
    let sink = writer.finish().map_err(|_| LZMA_PROG_ERROR)?;
    Ok(sink.into_inner())
}
