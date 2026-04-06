use std::io::{Cursor, Read};

use lzma_rust2::filter::bcj::BcjReader;

use super::common::SimpleFilterKind;
use crate::ffi::types::{lzma_ret, LZMA_DATA_ERROR};

pub(crate) fn decode_all(
    kind: SimpleFilterKind,
    start_offset: u32,
    input: &[u8],
) -> Result<Vec<u8>, lzma_ret> {
    let source = Cursor::new(input);
    let mut reader = match kind {
        SimpleFilterKind::X86 => BcjReader::new_x86(source, start_offset as usize),
        SimpleFilterKind::PowerPc => BcjReader::new_ppc(source, start_offset as usize),
        SimpleFilterKind::Ia64 => BcjReader::new_ia64(source, start_offset as usize),
        SimpleFilterKind::Arm => BcjReader::new_arm(source, start_offset as usize),
        SimpleFilterKind::ArmThumb => BcjReader::new_arm_thumb(source, start_offset as usize),
        SimpleFilterKind::Arm64 => BcjReader::new_arm64(source, start_offset as usize),
        SimpleFilterKind::Sparc => BcjReader::new_sparc(source, start_offset as usize),
    };

    let mut out = Vec::new();
    reader.read_to_end(&mut out).map_err(|_| LZMA_DATA_ERROR)?;
    Ok(out)
}
