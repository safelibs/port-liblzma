use std::io::{Cursor, Read};

use crate::ffi::types::lzma_ret;
use crate::internal::{delta, simple};

use super::common::{io_error_to_ret, ParsedFilterChain, Prefilter, TerminalFilter};

pub(crate) fn decode_terminal(filter: &TerminalFilter, input: &[u8]) -> Result<Vec<u8>, lzma_ret> {
    match filter {
        TerminalFilter::Lzma1 {
            options,
            expected_uncompressed_size,
            ..
        } => {
            let mut reader = lzma_rust2::LzmaReader::new(
                Cursor::new(input),
                expected_uncompressed_size.unwrap_or(u64::MAX),
                options.lc,
                options.lp,
                options.pb,
                options.dict_size,
                options.preset_dict.as_deref(),
            )
            .map_err(|error| io_error_to_ret(&error))?;

            let mut out = Vec::new();
            reader
                .read_to_end(&mut out)
                .map_err(|error| io_error_to_ret(&error))?;
            Ok(out)
        }
        TerminalFilter::Lzma2 { options } => {
            let mut reader = lzma_rust2::Lzma2Reader::new(
                Cursor::new(input),
                options.lzma_options.dict_size,
                options.lzma_options.preset_dict.as_deref(),
            );
            let mut out = Vec::new();
            reader
                .read_to_end(&mut out)
                .map_err(|error| io_error_to_ret(&error))?;
            Ok(out)
        }
    }
}

pub(crate) fn decode_raw(chain: &ParsedFilterChain, input: &[u8]) -> Result<Vec<u8>, lzma_ret> {
    let mut out = decode_terminal(&chain.terminal, input)?;

    for filter in chain.prefilters.iter().rev() {
        out = match *filter {
            Prefilter::Delta { distance } => delta::decode_all(&out, distance),
            Prefilter::Simple { kind, start_offset } => simple::decode_all(kind, start_offset, &out)?,
        };
    }

    Ok(out)
}
