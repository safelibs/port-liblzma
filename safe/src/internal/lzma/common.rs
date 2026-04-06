use std::io;

use crate::ffi::types::{
    lzma_check, lzma_filter, lzma_match_finder, lzma_options_bcj, lzma_options_delta,
    lzma_options_lzma, lzma_ret, LZMA_BUF_ERROR, LZMA_CHECK_NONE, LZMA_DATA_ERROR, LZMA_MEM_ERROR,
    LZMA_OPTIONS_ERROR, LZMA_PROG_ERROR,
};
use crate::internal::common::{LZMA_MODE_FAST, LZMA_MODE_NORMAL};
use crate::internal::delta;
use crate::internal::filter::common::{
    validate_chain_impl, LZMA_FILTER_DELTA, LZMA_FILTER_LZMA1, LZMA_FILTER_LZMA1EXT,
    LZMA_FILTER_LZMA2,
};
use crate::internal::lz;
use crate::internal::simple::{self, SimpleFilterKind};

pub(crate) const LZMA_LZMA1EXT_ALLOW_EOPM: u32 = 0x01;
pub(crate) const LZMA_MEMUSAGE_BASE: u64 = 1 << 15;
const LZMA_MATCH_LEN_MIN: u32 = 2;
const LZMA_MATCH_LEN_MAX: u32 = 273;
const LZMA_LZMA_ENCODER_OPTS: u32 = 1 << 12;
const LZMA_LZMA_LOOP_INPUT_MAX: u32 = LZMA_LZMA_ENCODER_OPTS + 1;
const LZMA1_ENCODER_OVERHEAD: u64 = 249_792;
const LZMA2_ENCODER_OVERHEAD: u64 = 315_496;
const LZMA1_DECODER_OVERHEAD: u64 = 32_640;
const LZMA2_DECODER_OVERHEAD: u64 = 32_824;

#[derive(Clone)]
pub(crate) enum Prefilter {
    Delta {
        distance: usize,
    },
    Simple {
        kind: SimpleFilterKind,
        start_offset: u32,
    },
}

#[derive(Clone)]
pub(crate) enum TerminalFilter {
    Lzma1 {
        options: lzma_rust2::LzmaOptions,
        match_finder: lzma_match_finder,
        allow_eopm: bool,
        expected_uncompressed_size: Option<u64>,
    },
    Lzma2 {
        options: lzma_rust2::Lzma2Options,
        match_finder: lzma_match_finder,
    },
}

#[derive(Clone)]
pub(crate) struct ParsedFilterChain {
    pub(crate) prefilters: Vec<Prefilter>,
    pub(crate) terminal: TerminalFilter,
}

pub(crate) fn io_error_to_ret(error: &io::Error) -> lzma_ret {
    match error.kind() {
        io::ErrorKind::UnexpectedEof => LZMA_BUF_ERROR,
        io::ErrorKind::InvalidInput => LZMA_OPTIONS_ERROR,
        io::ErrorKind::InvalidData => LZMA_DATA_ERROR,
        io::ErrorKind::OutOfMemory => LZMA_MEM_ERROR,
        _ => LZMA_PROG_ERROR,
    }
}

pub(crate) fn check_to_rust(check: lzma_check) -> Result<lzma_rust2::CheckType, lzma_ret> {
    match check {
        LZMA_CHECK_NONE => Ok(lzma_rust2::CheckType::None),
        crate::internal::common::LZMA_CHECK_CRC32 => Ok(lzma_rust2::CheckType::Crc32),
        crate::internal::common::LZMA_CHECK_CRC64 => Ok(lzma_rust2::CheckType::Crc64),
        crate::internal::common::LZMA_CHECK_SHA256 => Ok(lzma_rust2::CheckType::Sha256),
        _ => Err(LZMA_OPTIONS_ERROR),
    }
}

pub(crate) fn ext_size_from_options(options: &lzma_options_lzma) -> Option<u64> {
    let size = (u64::from(options.ext_size_high) << 32) | u64::from(options.ext_size_low);
    if size == u64::MAX {
        None
    } else {
        Some(size)
    }
}

fn map_mode(mode: i32) -> Result<lzma_rust2::EncodeMode, lzma_ret> {
    match mode {
        LZMA_MODE_FAST => Ok(lzma_rust2::EncodeMode::Fast),
        LZMA_MODE_NORMAL => Ok(lzma_rust2::EncodeMode::Normal),
        _ => Err(LZMA_OPTIONS_ERROR),
    }
}

fn map_lzma_options(options: &lzma_options_lzma) -> Result<lzma_rust2::LzmaOptions, lzma_ret> {
    if options.lc > 8 || options.lp > 4 || options.pb > 4 || options.lc + options.lp > 4 {
        return Err(LZMA_OPTIONS_ERROR);
    }

    let preset_dict = if options.preset_dict.is_null() || options.preset_dict_size == 0 {
        None
    } else {
        Some(unsafe {
            std::slice::from_raw_parts(options.preset_dict, options.preset_dict_size as usize)
                .to_vec()
        })
    };

    Ok(lzma_rust2::LzmaOptions {
        dict_size: options.dict_size,
        lc: options.lc,
        lp: options.lp,
        pb: options.pb,
        mode: if options.mode == 0 {
            lzma_rust2::EncodeMode::Normal
        } else {
            map_mode(options.mode)?
        },
        nice_len: if options.nice_len == 0 {
            64
        } else {
            options.nice_len.max(8)
        },
        mf: if options.mf == 0 {
            lzma_rust2::MfType::Bt4
        } else {
            lz::map_match_finder(options.mf)?
        },
        depth_limit: options.depth.min(i32::MAX as u32) as i32,
        preset_dict,
    })
}

unsafe fn parse_prefilter(filter: &lzma_filter) -> Result<Prefilter, lzma_ret> {
    if filter.id == LZMA_FILTER_DELTA {
        let distance = delta::distance_from_options(filter.options.cast::<lzma_options_delta>())?;
        return Ok(Prefilter::Delta { distance });
    }

    let kind = simple::kind_from_filter_id(filter.id).ok_or(LZMA_OPTIONS_ERROR)?;
    let start_offset =
        simple::options_to_start_offset(kind, filter.options.cast::<lzma_options_bcj>())?;
    Ok(Prefilter::Simple { kind, start_offset })
}

unsafe fn parse_terminal(filter: &lzma_filter) -> Result<TerminalFilter, lzma_ret> {
    let options = filter.options.cast::<lzma_options_lzma>();
    if options.is_null() {
        return Err(LZMA_PROG_ERROR);
    }
    let options = &*options;
    let rust_options = map_lzma_options(options)?;

    match filter.id {
        LZMA_FILTER_LZMA1 => Ok(TerminalFilter::Lzma1 {
            options: rust_options,
            match_finder: options.mf,
            allow_eopm: false,
            expected_uncompressed_size: ext_size_from_options(options),
        }),
        LZMA_FILTER_LZMA1EXT => {
            if (options.ext_flags & !LZMA_LZMA1EXT_ALLOW_EOPM) != 0 {
                return Err(LZMA_OPTIONS_ERROR);
            }

            Ok(TerminalFilter::Lzma1 {
                options: rust_options,
                match_finder: options.mf,
                allow_eopm: (options.ext_flags & LZMA_LZMA1EXT_ALLOW_EOPM) != 0,
                expected_uncompressed_size: ext_size_from_options(options),
            })
        }
        LZMA_FILTER_LZMA2 => Ok(TerminalFilter::Lzma2 {
            options: lzma_rust2::Lzma2Options {
                lzma_options: rust_options,
                ..Default::default()
            },
            match_finder: options.mf,
        }),
        _ => Err(LZMA_OPTIONS_ERROR),
    }
}

pub(crate) unsafe fn parse_filters(
    filters: *const lzma_filter,
) -> Result<ParsedFilterChain, lzma_ret> {
    let mut count = 0usize;
    let ret = validate_chain_impl(filters, &mut count);
    if ret != 0 {
        return Err(ret);
    }

    let mut prefilters = Vec::with_capacity(count.saturating_sub(1));
    for i in 0..count - 1 {
        prefilters.push(parse_prefilter(&*filters.add(i))?);
    }

    let terminal = parse_terminal(&*filters.add(count - 1))?;
    Ok(ParsedFilterChain {
        prefilters,
        terminal,
    })
}

pub(crate) unsafe fn encoder_memusage(filters: *const lzma_filter) -> u64 {
    match parse_filters(filters) {
        Ok(chain) => {
            let prefilter_usage = (chain.prefilters.len() as u64).saturating_mul(1024);
            let terminal_usage = match chain.terminal {
                TerminalFilter::Lzma1 {
                    options,
                    match_finder,
                    ..
                } => {
                    if options.nice_len < LZMA_MATCH_LEN_MIN
                        || options.nice_len > LZMA_MATCH_LEN_MAX
                    {
                        return u64::MAX;
                    }

                    let core_usage = lz::encoder_memusage(
                        options.dict_size,
                        LZMA_LZMA_ENCODER_OPTS,
                        LZMA_LZMA_LOOP_INPUT_MAX,
                        LZMA_MATCH_LEN_MAX,
                        options.nice_len,
                        match_finder,
                    );
                    if core_usage == u64::MAX {
                        return u64::MAX;
                    }

                    core_usage.saturating_add(LZMA1_ENCODER_OVERHEAD)
                }
                TerminalFilter::Lzma2 {
                    options,
                    match_finder,
                } => {
                    let lzma_options = &options.lzma_options;
                    if lzma_options.nice_len < LZMA_MATCH_LEN_MIN
                        || lzma_options.nice_len > LZMA_MATCH_LEN_MAX
                    {
                        return u64::MAX;
                    }

                    let core_usage = lz::encoder_memusage(
                        lzma_options.dict_size,
                        LZMA_LZMA_ENCODER_OPTS,
                        LZMA_LZMA_LOOP_INPUT_MAX,
                        LZMA_MATCH_LEN_MAX,
                        lzma_options.nice_len,
                        match_finder,
                    );
                    if core_usage == u64::MAX {
                        return u64::MAX;
                    }

                    core_usage.saturating_add(LZMA2_ENCODER_OVERHEAD)
                }
            };

            terminal_usage
                .saturating_add(prefilter_usage)
                .saturating_add(LZMA_MEMUSAGE_BASE)
        }
        Err(_) => u64::MAX,
    }
}

pub(crate) unsafe fn decoder_memusage(filters: *const lzma_filter) -> u64 {
    match parse_filters(filters) {
        Ok(chain) => {
            let prefilter_usage = (chain.prefilters.len() as u64).saturating_mul(1024);
            let terminal_usage = match chain.terminal {
                TerminalFilter::Lzma1 { options, .. } => {
                    lz::decoder_memusage(options.dict_size).saturating_add(LZMA1_DECODER_OVERHEAD)
                }
                TerminalFilter::Lzma2 { options, .. } => {
                    lz::decoder_memusage(options.lzma_options.dict_size)
                        .saturating_add(LZMA2_DECODER_OVERHEAD)
                }
            };

            if terminal_usage == u64::MAX {
                u64::MAX
            } else {
                terminal_usage
                    .saturating_add(prefilter_usage)
                    .saturating_add(LZMA_MEMUSAGE_BASE)
            }
        }
        Err(_) => u64::MAX,
    }
}
