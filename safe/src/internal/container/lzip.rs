use core::ffi::c_void;
use core::ptr;
use std::io::{Cursor, Read};

use crate::ffi::types::{
    lzma_action, lzma_allocator, lzma_check, lzma_ret, lzma_stream, LZMA_BUF_ERROR,
    LZMA_FORMAT_ERROR, LZMA_GET_CHECK, LZMA_MEMLIMIT_ERROR, LZMA_OK, LZMA_OPTIONS_ERROR,
    LZMA_PROG_ERROR, LZMA_STREAM_END,
};
use crate::internal::check;
use crate::internal::common::{all_supported_actions, LZMA_CHECK_CRC32};
use crate::internal::stream_state::{install_next_coder, NextCoder};

const LZIP_MAGIC: [u8; 4] = *b"LZIP";
const LZIP_V0_FOOTER_SIZE: usize = 12;
const LZIP_V1_FOOTER_SIZE: usize = 20;
const LZIP_HEADER_SIZE: usize = 6;
const LZMA_TELL_NO_CHECK: u32 = 0x01;
const LZMA_TELL_UNSUPPORTED_CHECK: u32 = 0x02;
const LZMA_CONCATENATED: u32 = 0x08;
const LZMA_TELL_ANY_CHECK: u32 = 0x04;
const LZMA_IGNORE_CHECK: u32 = 0x10;
const SUPPORTED_FLAGS: u32 = LZMA_TELL_NO_CHECK
    | LZMA_TELL_UNSUPPORTED_CHECK
    | LZMA_TELL_ANY_CHECK
    | LZMA_CONCATENATED
    | LZMA_IGNORE_CHECK;

struct LzipDecoder {
    input: Vec<u8>,
    output: Vec<u8>,
    output_pos: usize,
    memlimit: u64,
    memusage: u64,
    tell_any_check: bool,
    ignore_check: bool,
    concatenated: bool,
    first_member: bool,
    told_check: bool,
    finished: bool,
}

enum ParseResult {
    NeedMore {
        consumed: usize,
        memusage: u64,
    },
    GetCheck {
        consumed: usize,
        memusage: u64,
    },
    Done {
        consumed: usize,
        output: Vec<u8>,
        memusage: u64,
    },
    Error {
        ret: lzma_ret,
        consumed: usize,
        memusage: u64,
    },
}

fn decode_dict_size(byte: u8) -> Result<u32, lzma_ret> {
    let b2log = u32::from(byte & 0x1F);
    let fracnum = u32::from(byte >> 5);
    if b2log < 12 || b2log > 29 || (b2log == 12 && fracnum > 0) {
        return Err(crate::ffi::types::LZMA_DATA_ERROR);
    }
    Ok((1u32 << b2log) - (fracnum << (b2log - 4)))
}

fn lzip_memusage(dict_size: u32) -> u64 {
    u64::from(lzma_rust2::lzma_get_memory_usage(dict_size, 3, 0).unwrap_or(u32::MAX))
        .saturating_mul(1024)
        .saturating_add(1)
}

fn decode_payload(payload: &[u8], dict_size: u32) -> Option<Vec<u8>> {
    let mut reader =
        lzma_rust2::LzmaReader::new(Cursor::new(payload), u64::MAX, 3, 0, 2, dict_size, None)
            .ok()?;
    let mut output = Vec::new();
    reader.read_to_end(&mut output).ok()?;
    Some(output)
}

fn find_member_end(
    input: &[u8],
    version: u8,
    dict_size: u32,
    ignore_check: bool,
) -> Option<(usize, Vec<u8>)> {
    let footer_size = if version == 0 {
        LZIP_V0_FOOTER_SIZE
    } else {
        LZIP_V1_FOOTER_SIZE
    };
    let min_total = LZIP_HEADER_SIZE + footer_size;

    for end in min_total..=input.len() {
        let footer = &input[end - footer_size..end];
        let crc32 = u32::from_le_bytes([footer[0], footer[1], footer[2], footer[3]]);
        let data_size = u64::from_le_bytes([
            footer[4], footer[5], footer[6], footer[7], footer[8], footer[9], footer[10],
            footer[11],
        ]);
        if version == 1 {
            let member_size = u64::from_le_bytes([
                footer[12], footer[13], footer[14], footer[15], footer[16], footer[17], footer[18],
                footer[19],
            ]);
            if member_size != end as u64 {
                continue;
            }
        }

        let payload = &input[LZIP_HEADER_SIZE..end - footer_size];
        let Some(decoded) = decode_payload(payload, dict_size) else {
            continue;
        };
        if decoded.len() as u64 != data_size {
            continue;
        }
        if !ignore_check && check::crc32::crc32(&decoded, 0) != crc32 {
            continue;
        }
        return Some((end, decoded));
    }

    None
}

fn parse_lzip(
    input: &[u8],
    finish: bool,
    tell_any_check: bool,
    told_check: bool,
    ignore_check: bool,
    concatenated: bool,
    first_member: bool,
    memlimit: u64,
) -> ParseResult {
    let mut consumed = 0usize;
    let mut output = Vec::new();
    let mut current_first_member = first_member;
    let mut current_told_check = told_check;
    let mut max_memusage = 1u64;

    loop {
        let remaining = &input[consumed..];
        let available_magic = remaining.len().min(LZIP_MAGIC.len());
        let mut matched = 0usize;
        while matched < available_magic && remaining[matched] == LZIP_MAGIC[matched] {
            matched += 1;
        }

        if matched < available_magic {
            if current_first_member {
                return ParseResult::Error {
                    ret: LZMA_FORMAT_ERROR,
                    consumed,
                    memusage: max_memusage,
                };
            }

            return ParseResult::Done {
                consumed: consumed + matched,
                output,
                memusage: max_memusage,
            };
        }

        if remaining.len() < LZIP_MAGIC.len() {
            if current_first_member {
                return ParseResult::NeedMore {
                    consumed: consumed + matched,
                    memusage: max_memusage,
                };
            }

            if finish {
                return ParseResult::Done {
                    consumed: consumed + matched,
                    output,
                    memusage: max_memusage,
                };
            }

            return ParseResult::NeedMore {
                consumed: consumed + matched,
                memusage: max_memusage,
            };
        }

        if remaining.len() < 5 {
            return ParseResult::NeedMore {
                consumed: consumed + 4,
                memusage: max_memusage,
            };
        }

        let version = remaining[4];
        if version > 1 {
            return ParseResult::Error {
                ret: LZMA_OPTIONS_ERROR,
                consumed: consumed + 5,
                memusage: max_memusage,
            };
        }

        if remaining.len() < LZIP_HEADER_SIZE {
            return ParseResult::NeedMore {
                consumed: consumed + 5,
                memusage: max_memusage,
            };
        }

        let dict_size = match decode_dict_size(remaining[5]) {
            Ok(size) => size,
            Err(ret) => {
                return ParseResult::Error {
                    ret,
                    consumed: consumed + 6,
                    memusage: max_memusage,
                };
            }
        };
        max_memusage = max_memusage.max(lzip_memusage(dict_size));
        if max_memusage > memlimit.max(1) {
            return ParseResult::Error {
                ret: LZMA_MEMLIMIT_ERROR,
                consumed: consumed + 6,
                memusage: max_memusage,
            };
        }

        if tell_any_check && !current_told_check {
            return ParseResult::GetCheck {
                consumed: consumed + 6,
                memusage: max_memusage,
            };
        }

        let Some((member_end, decoded)) =
            find_member_end(remaining, version, dict_size, ignore_check)
        else {
            if finish
                && !current_first_member
                && remaining.starts_with(&LZIP_MAGIC)
                && remaining.len() >= LZIP_MAGIC.len()
                && remaining.len() < LZIP_HEADER_SIZE
            {
                return ParseResult::Error {
                    ret: LZMA_BUF_ERROR,
                    consumed: consumed + remaining.len(),
                    memusage: max_memusage,
                };
            }

            if finish {
                return ParseResult::Error {
                    ret: crate::ffi::types::LZMA_DATA_ERROR,
                    consumed: input.len(),
                    memusage: max_memusage,
                };
            }

            return ParseResult::NeedMore {
                consumed: input.len(),
                memusage: max_memusage,
            };
        };

        output.extend_from_slice(&decoded);
        consumed += member_end;
        current_first_member = false;
        current_told_check = true;

        if !concatenated {
            return ParseResult::Done {
                consumed,
                output,
                memusage: max_memusage,
            };
        }

        if consumed == input.len() {
            return ParseResult::Done {
                consumed,
                output,
                memusage: max_memusage,
            };
        }
    }
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

unsafe fn lzip_decode(
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
    let coder = &mut *coder.cast::<LzipDecoder>();
    if coder.output_pos < coder.output.len() {
        let ret = copy_output(
            &coder.output,
            &mut coder.output_pos,
            output,
            out_pos,
            out_size,
        );
        if ret == LZMA_STREAM_END && coder.finished {
            return LZMA_STREAM_END;
        }
        return ret;
    }

    let current = if in_size == 0 {
        &[][..]
    } else {
        core::slice::from_raw_parts(input, in_size)
    };
    let base_len = coder.input.len();
    let mut combined = coder.input.clone();
    combined.extend_from_slice(current);

    let parsed = parse_lzip(
        &combined,
        action == crate::internal::common::LZMA_FINISH,
        coder.tell_any_check,
        coder.told_check,
        coder.ignore_check,
        coder.concatenated,
        coder.first_member,
        coder.memlimit,
    );

    match parsed {
        ParseResult::NeedMore { consumed, memusage } => {
            let consume_now = consumed.saturating_sub(base_len).min(in_size);
            coder.input.extend_from_slice(&current[..consume_now]);
            coder.memusage = memusage;
            if !in_pos.is_null() {
                *in_pos = consume_now;
            }
            LZMA_OK
        }
        ParseResult::GetCheck { consumed, memusage } => {
            let consume_now = consumed.saturating_sub(base_len).min(in_size);
            coder.input.extend_from_slice(&current[..consume_now]);
            coder.memusage = memusage;
            coder.told_check = true;
            if !in_pos.is_null() {
                *in_pos = consume_now;
            }
            LZMA_GET_CHECK
        }
        ParseResult::Done {
            consumed,
            output: decoded,
            memusage,
        } => {
            let consume_now = consumed.saturating_sub(base_len).min(in_size);
            coder.input.extend_from_slice(&current[..consume_now]);
            coder.memusage = memusage;
            coder.output = decoded;
            coder.output_pos = 0;
            coder.finished = true;
            coder.first_member = false;
            if !in_pos.is_null() {
                *in_pos = consume_now;
            }
            copy_output(
                &coder.output,
                &mut coder.output_pos,
                output,
                out_pos,
                out_size,
            )
        }
        ParseResult::Error {
            ret,
            consumed,
            memusage,
        } => {
            let consume_now = consumed.saturating_sub(base_len).min(in_size);
            coder.input.extend_from_slice(&current[..consume_now]);
            coder.memusage = memusage;
            coder.first_member = false;
            if !in_pos.is_null() {
                *in_pos = consume_now;
            }
            ret
        }
    }
}

unsafe fn lzip_end(coder: *mut c_void, _allocator: *const lzma_allocator) {
    drop(Box::from_raw(coder.cast::<LzipDecoder>()));
}

unsafe fn lzip_get_check(_coder: *const c_void) -> lzma_check {
    LZMA_CHECK_CRC32
}

unsafe fn lzip_memconfig(
    coder: *mut c_void,
    memusage: *mut u64,
    old_memlimit: *mut u64,
    new_memlimit: u64,
) -> lzma_ret {
    let coder = &mut *coder.cast::<LzipDecoder>();
    *memusage = coder.memusage;
    *old_memlimit = coder.memlimit;
    if new_memlimit != 0 {
        if new_memlimit < coder.memusage {
            return LZMA_MEMLIMIT_ERROR;
        }
        coder.memlimit = new_memlimit;
    }
    LZMA_OK
}

pub(crate) unsafe fn lzip_decoder(strm: *mut lzma_stream, memlimit: u64, flags: u32) -> lzma_ret {
    if strm.is_null() {
        return LZMA_PROG_ERROR;
    }
    if (flags & !SUPPORTED_FLAGS) != 0 {
        return LZMA_OPTIONS_ERROR;
    }

    install_next_coder(
        strm,
        NextCoder {
            coder: Box::into_raw(Box::new(LzipDecoder {
                input: Vec::new(),
                output: Vec::new(),
                output_pos: 0,
                memlimit: memlimit.max(1),
                memusage: 1,
                tell_any_check: (flags & LZMA_TELL_ANY_CHECK) != 0,
                ignore_check: (flags & LZMA_IGNORE_CHECK) != 0,
                concatenated: (flags & LZMA_CONCATENATED) != 0,
                first_member: true,
                told_check: false,
                finished: false,
            }))
            .cast(),
            code: lzip_decode,
            end: Some(lzip_end),
            get_progress: None,
            get_check: Some(lzip_get_check),
            memconfig: Some(lzip_memconfig),
        },
        all_supported_actions(),
    )
}

#[cfg(test)]
mod tests {
    use super::parse_lzip;
    use crate::ffi::types::LZMA_DATA_ERROR;

    fn parse_invalid_member(name: &str) -> super::ParseResult {
        let data = std::fs::read(format!(
            "{}/tests/upstream/files/{}",
            env!("CARGO_MANIFEST_DIR"),
            name
        ))
        .unwrap();

        parse_lzip(&data, true, false, false, false, true, true, u64::MAX)
    }

    #[test]
    fn invalid_uncompressed_sizes_fail_in_parser() {
        for name in ["bad-1-v0-uncomp-size.lz", "bad-1-v1-uncomp-size.lz"] {
            match parse_invalid_member(name) {
                super::ParseResult::Error { ret, .. } => assert_eq!(ret, LZMA_DATA_ERROR, "{name}"),
                super::ParseResult::NeedMore { .. } => {
                    panic!("unexpected need-more parse result for {name}")
                }
                super::ParseResult::GetCheck { .. } => {
                    panic!("unexpected get-check parse result for {name}")
                }
                super::ParseResult::Done { .. } => panic!("unexpected successful parse for {name}"),
            }
        }
    }
}
