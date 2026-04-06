use std::io::{Cursor, Write};

use crate::ffi::types::{lzma_ret, LZMA_PROG_ERROR};

use super::common::{ParsedFilterChain, Prefilter, TerminalFilter};
use crate::internal::{delta, simple};

pub(crate) fn encode_terminal(filter: &TerminalFilter, input: &[u8]) -> Result<Vec<u8>, lzma_ret> {
    match filter {
        TerminalFilter::Lzma1 {
            options,
            allow_eopm,
            ..
        } => {
            let sink = Cursor::new(Vec::new());
            let mut writer =
                lzma_rust2::LzmaWriter::new_no_header(sink, options, *allow_eopm)
                    .map_err(|_| LZMA_PROG_ERROR)?;
            writer.write_all(input).map_err(|_| LZMA_PROG_ERROR)?;
            let sink = writer.finish().map_err(|_| LZMA_PROG_ERROR)?;
            Ok(sink.into_inner())
        }
        TerminalFilter::Lzma2 { options } => {
            let sink = Cursor::new(Vec::new());
            let mut writer = lzma_rust2::Lzma2Writer::new(sink, options.clone());
            writer.write_all(input).map_err(|_| LZMA_PROG_ERROR)?;
            let sink = writer.finish().map_err(|_| LZMA_PROG_ERROR)?;
            Ok(sink.into_inner())
        }
    }
}

pub(crate) fn encode_raw(chain: &ParsedFilterChain, input: &[u8]) -> Result<Vec<u8>, lzma_ret> {
    let mut transformed = input.to_vec();

    for filter in &chain.prefilters {
        transformed = match *filter {
            Prefilter::Delta { distance } => delta::encode_all(&transformed, distance),
            Prefilter::Simple { kind, start_offset } => {
                simple::encode_all(kind, start_offset, &transformed)?
            }
        };
    }

    encode_terminal(&chain.terminal, &transformed)
}
