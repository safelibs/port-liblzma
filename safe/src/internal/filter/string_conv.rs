use core::ffi::{c_char, c_int, c_void, CStr};
use core::mem::size_of;
use core::ptr;

use crate::ffi::types::{
    lzma_allocator, lzma_filter, lzma_options_bcj, lzma_options_delta, lzma_options_lzma, lzma_ret,
    lzma_vli, LZMA_FILTERS_MAX, LZMA_MEM_ERROR, LZMA_OK, LZMA_OPTIONS_ERROR, LZMA_PROG_ERROR,
    LZMA_VLI_UNKNOWN,
};
use crate::internal::common::{
    lzma_alloc, lzma_alloc_zero, lzma_free, LZMA_DELTA_DIST_MAX, LZMA_DELTA_DIST_MIN,
    LZMA_DICT_SIZE_MIN, LZMA_LCLP_MAX, LZMA_LCLP_MIN, LZMA_MF_BT2, LZMA_MF_BT3, LZMA_MF_BT4,
    LZMA_MF_HC3, LZMA_MF_HC4, LZMA_MODE_FAST, LZMA_MODE_NORMAL, LZMA_PB_MAX, LZMA_PB_MIN,
    LZMA_PRESET_DEFAULT, LZMA_PRESET_EXTREME,
};
use crate::internal::filter::common::{
    filters_free_impl, validate_chain_impl, LZMA_FILTER_ARM, LZMA_FILTER_ARM64,
    LZMA_FILTER_ARMTHUMB, LZMA_FILTER_DELTA, LZMA_FILTER_IA64, LZMA_FILTER_LZMA1,
    LZMA_FILTER_LZMA2, LZMA_FILTER_POWERPC, LZMA_FILTER_RESERVED_START, LZMA_FILTER_SPARC,
    LZMA_FILTER_X86,
};
use crate::internal::filter::properties::LZMA_DELTA_TYPE_BYTE;
use crate::internal::preset::lzma_lzma_preset_impl;

const STR_ALLOC_SIZE: usize = 800;
const NAME_LEN_MAX: usize = 11;

const OPTMAP_USE_NAME_VALUE_MAP: u8 = 0x01;
const OPTMAP_USE_BYTE_SUFFIX: u8 = 0x02;
const OPTMAP_NO_STRFY_ZERO: u8 = 0x04;

const LZMA_STR_ALL_FILTERS: u32 = 0x01;
const LZMA_STR_NO_VALIDATION: u32 = 0x02;
const LZMA_STR_ENCODER: u32 = 0x10;
const LZMA_STR_DECODER: u32 = 0x20;
const LZMA_STR_GETOPT_LONG: u32 = 0x40;
const LZMA_STR_NO_SPACES: u32 = 0x80;

type ErrorMessage = &'static [u8];

const MSG_MEMORY_ALLOCATION_FAILED: ErrorMessage = b"Memory allocation failed\0";
const MSG_UNSUPPORTED_PRESET_FLAG: ErrorMessage = b"Unsupported preset flag\0";
const MSG_UNSUPPORTED_PRESET: ErrorMessage = b"Unsupported preset\0";
const MSG_SUM_LC_LP: ErrorMessage = b"The sum of lc and lp must not exceed 4\0";
const MSG_UNKNOWN_OPTION_NAME: ErrorMessage = b"Unknown option name\0";
const MSG_OPTIONS_MUST_BE_NAME_VALUE: ErrorMessage =
    b"Options must be 'name=value' pairs separated with commas\0";
const MSG_OPTION_VALUE_EMPTY: ErrorMessage = b"Option value cannot be empty\0";
const MSG_INVALID_OPTION_VALUE: ErrorMessage = b"Invalid option value\0";
const MSG_NOT_NON_NEGATIVE_DECIMAL: ErrorMessage = b"Value is not a non-negative decimal integer\0";
const MSG_VALUE_OUT_OF_RANGE: ErrorMessage = b"Value out of range\0";
const MSG_NO_INTEGER_SUFFIXES: ErrorMessage =
    b"This option does not support any integer suffixes\0";
const MSG_INVALID_MULTIPLIER_SUFFIX: ErrorMessage =
    b"Invalid multiplier suffix (KiB, MiB, or GiB)\0";
const MSG_UNKNOWN_FILTER_NAME: ErrorMessage = b"Unknown filter name\0";
const MSG_FILTER_NOT_XZ: ErrorMessage = b"This filter cannot be used in the .xz format\0";
const MSG_EMPTY_STRING: ErrorMessage =
    b"Empty string is not allowed, try \"6\" if a default value is needed\0";
const MSG_MAX_FILTERS: ErrorMessage = b"The maximum number of filters is four\0";
const MSG_FILTER_NAME_MISSING: ErrorMessage = b"Filter name is missing\0";
const MSG_INVALID_FILTER_CHAIN: ErrorMessage =
    b"Invalid filter chain ('lzma2' missing at the end?)\0";
const MSG_STR_TO_FILTERS_NULL_ARGS: ErrorMessage =
    b"Unexpected NULL pointer argument(s) to lzma_str_to_filters()\0";
const MSG_STR_TO_FILTERS_BAD_FLAGS: ErrorMessage = b"Unsupported flags to lzma_str_to_filters()\0";

const LZMA12_PRESET_STR: &[u8] = b"0-9[e]";

#[derive(Copy, Clone, PartialEq, Eq)]
enum OptionType {
    Uint32,
    LzmaMode,
    LzmaMatchFinder,
    LzmaPreset,
}

#[derive(Copy, Clone)]
enum OptionField {
    None,
    StartOffset,
    Dist,
    DictSize,
    Lc,
    Lp,
    Pb,
    Mode,
    NiceLen,
    MatchFinder,
    Depth,
}

#[derive(Copy, Clone)]
struct NameValue {
    name: &'static [u8],
    value: u32,
}

#[derive(Copy, Clone)]
struct OptionSpec {
    name: &'static [u8],
    kind: OptionType,
    flags: u8,
    field: OptionField,
    range_min: u32,
    range_max: u32,
    map: &'static [NameValue],
}

#[derive(Copy, Clone)]
enum ParseKind {
    Bcj,
    Delta,
    Lzma12,
}

#[derive(Copy, Clone)]
struct FilterSpec {
    name: &'static [u8],
    opts_size: usize,
    id: lzma_vli,
    parse_kind: ParseKind,
    optmap: &'static [OptionSpec],
    strfy_encoder: usize,
    strfy_decoder: usize,
    allow_null: bool,
}

const LZMA12_MODE_MAP: [NameValue; 2] = [
    NameValue {
        name: b"fast",
        value: LZMA_MODE_FAST as u32,
    },
    NameValue {
        name: b"normal",
        value: LZMA_MODE_NORMAL as u32,
    },
];

const LZMA12_MF_MAP: [NameValue; 5] = [
    NameValue {
        name: b"hc3",
        value: LZMA_MF_HC3 as u32,
    },
    NameValue {
        name: b"hc4",
        value: LZMA_MF_HC4 as u32,
    },
    NameValue {
        name: b"bt2",
        value: LZMA_MF_BT2 as u32,
    },
    NameValue {
        name: b"bt3",
        value: LZMA_MF_BT3 as u32,
    },
    NameValue {
        name: b"bt4",
        value: LZMA_MF_BT4 as u32,
    },
];

const BCJ_OPTMAP: [OptionSpec; 1] = [OptionSpec {
    name: b"start",
    kind: OptionType::Uint32,
    flags: OPTMAP_NO_STRFY_ZERO | OPTMAP_USE_BYTE_SUFFIX,
    field: OptionField::StartOffset,
    range_min: 0,
    range_max: u32::MAX,
    map: &[],
}];

const DELTA_OPTMAP: [OptionSpec; 1] = [OptionSpec {
    name: b"dist",
    kind: OptionType::Uint32,
    flags: 0,
    field: OptionField::Dist,
    range_min: LZMA_DELTA_DIST_MIN,
    range_max: LZMA_DELTA_DIST_MAX,
    map: &[],
}];

const LZMA12_OPTMAP: [OptionSpec; 9] = [
    OptionSpec {
        name: b"preset",
        kind: OptionType::LzmaPreset,
        flags: 0,
        field: OptionField::None,
        range_min: 0,
        range_max: 0,
        map: &[],
    },
    OptionSpec {
        name: b"dict",
        kind: OptionType::Uint32,
        flags: OPTMAP_USE_BYTE_SUFFIX,
        field: OptionField::DictSize,
        range_min: LZMA_DICT_SIZE_MIN,
        range_max: (1u32 << 30) + (1u32 << 29),
        map: &[],
    },
    OptionSpec {
        name: b"lc",
        kind: OptionType::Uint32,
        flags: 0,
        field: OptionField::Lc,
        range_min: LZMA_LCLP_MIN,
        range_max: LZMA_LCLP_MAX,
        map: &[],
    },
    OptionSpec {
        name: b"lp",
        kind: OptionType::Uint32,
        flags: 0,
        field: OptionField::Lp,
        range_min: LZMA_LCLP_MIN,
        range_max: LZMA_LCLP_MAX,
        map: &[],
    },
    OptionSpec {
        name: b"pb",
        kind: OptionType::Uint32,
        flags: 0,
        field: OptionField::Pb,
        range_min: LZMA_PB_MIN,
        range_max: LZMA_PB_MAX,
        map: &[],
    },
    OptionSpec {
        name: b"mode",
        kind: OptionType::LzmaMode,
        flags: OPTMAP_USE_NAME_VALUE_MAP,
        field: OptionField::Mode,
        range_min: 0,
        range_max: 0,
        map: &LZMA12_MODE_MAP,
    },
    OptionSpec {
        name: b"nice",
        kind: OptionType::Uint32,
        flags: 0,
        field: OptionField::NiceLen,
        range_min: 2,
        range_max: 273,
        map: &[],
    },
    OptionSpec {
        name: b"mf",
        kind: OptionType::LzmaMatchFinder,
        flags: OPTMAP_USE_NAME_VALUE_MAP,
        field: OptionField::MatchFinder,
        range_min: 0,
        range_max: 0,
        map: &LZMA12_MF_MAP,
    },
    OptionSpec {
        name: b"depth",
        kind: OptionType::Uint32,
        flags: 0,
        field: OptionField::Depth,
        range_min: 0,
        range_max: u32::MAX,
        map: &[],
    },
];

const FILTER_NAME_MAP: [FilterSpec; 10] = [
    FilterSpec {
        name: b"lzma1",
        opts_size: size_of::<lzma_options_lzma>(),
        id: LZMA_FILTER_LZMA1,
        parse_kind: ParseKind::Lzma12,
        optmap: &LZMA12_OPTMAP,
        strfy_encoder: 9,
        strfy_decoder: 5,
        allow_null: false,
    },
    FilterSpec {
        name: b"lzma2",
        opts_size: size_of::<lzma_options_lzma>(),
        id: LZMA_FILTER_LZMA2,
        parse_kind: ParseKind::Lzma12,
        optmap: &LZMA12_OPTMAP,
        strfy_encoder: 9,
        strfy_decoder: 2,
        allow_null: false,
    },
    FilterSpec {
        name: b"x86",
        opts_size: size_of::<lzma_options_bcj>(),
        id: LZMA_FILTER_X86,
        parse_kind: ParseKind::Bcj,
        optmap: &BCJ_OPTMAP,
        strfy_encoder: 1,
        strfy_decoder: 1,
        allow_null: true,
    },
    FilterSpec {
        name: b"arm",
        opts_size: size_of::<lzma_options_bcj>(),
        id: LZMA_FILTER_ARM,
        parse_kind: ParseKind::Bcj,
        optmap: &BCJ_OPTMAP,
        strfy_encoder: 1,
        strfy_decoder: 1,
        allow_null: true,
    },
    FilterSpec {
        name: b"armthumb",
        opts_size: size_of::<lzma_options_bcj>(),
        id: LZMA_FILTER_ARMTHUMB,
        parse_kind: ParseKind::Bcj,
        optmap: &BCJ_OPTMAP,
        strfy_encoder: 1,
        strfy_decoder: 1,
        allow_null: true,
    },
    FilterSpec {
        name: b"arm64",
        opts_size: size_of::<lzma_options_bcj>(),
        id: LZMA_FILTER_ARM64,
        parse_kind: ParseKind::Bcj,
        optmap: &BCJ_OPTMAP,
        strfy_encoder: 1,
        strfy_decoder: 1,
        allow_null: true,
    },
    FilterSpec {
        name: b"powerpc",
        opts_size: size_of::<lzma_options_bcj>(),
        id: LZMA_FILTER_POWERPC,
        parse_kind: ParseKind::Bcj,
        optmap: &BCJ_OPTMAP,
        strfy_encoder: 1,
        strfy_decoder: 1,
        allow_null: true,
    },
    FilterSpec {
        name: b"ia64",
        opts_size: size_of::<lzma_options_bcj>(),
        id: LZMA_FILTER_IA64,
        parse_kind: ParseKind::Bcj,
        optmap: &BCJ_OPTMAP,
        strfy_encoder: 1,
        strfy_decoder: 1,
        allow_null: true,
    },
    FilterSpec {
        name: b"sparc",
        opts_size: size_of::<lzma_options_bcj>(),
        id: LZMA_FILTER_SPARC,
        parse_kind: ParseKind::Bcj,
        optmap: &BCJ_OPTMAP,
        strfy_encoder: 1,
        strfy_decoder: 1,
        allow_null: true,
    },
    FilterSpec {
        name: b"delta",
        opts_size: size_of::<lzma_options_delta>(),
        id: LZMA_FILTER_DELTA,
        parse_kind: ParseKind::Delta,
        optmap: &DELTA_OPTMAP,
        strfy_encoder: 1,
        strfy_decoder: 1,
        allow_null: false,
    },
];

struct LzmaString {
    buf: *mut u8,
    pos: usize,
}

impl LzmaString {
    unsafe fn init(allocator: *const lzma_allocator) -> Result<Self, lzma_ret> {
        let buf = lzma_alloc(STR_ALLOC_SIZE, allocator).cast::<u8>();
        if buf.is_null() {
            return Err(LZMA_MEM_ERROR);
        }

        Ok(Self { buf, pos: 0 })
    }

    unsafe fn free(&mut self, allocator: *const lzma_allocator) {
        lzma_free(self.buf.cast(), allocator);
        self.buf = ptr::null_mut();
    }

    fn is_full(&self) -> bool {
        self.pos == STR_ALLOC_SIZE - 1
    }

    unsafe fn finish(
        mut self,
        output: *mut *mut c_char,
        allocator: *const lzma_allocator,
    ) -> lzma_ret {
        if self.is_full() {
            lzma_free(self.buf.cast(), allocator);
            *output = ptr::null_mut();
            return LZMA_PROG_ERROR;
        }

        *self.buf.add(self.pos) = 0;
        *output = self.buf.cast();
        self.buf = ptr::null_mut();
        LZMA_OK
    }

    unsafe fn append_bytes(&mut self, bytes: &[u8]) {
        let limit = STR_ALLOC_SIZE - 1 - self.pos;
        let copy_size = bytes.len().min(limit);
        ptr::copy_nonoverlapping(bytes.as_ptr(), self.buf.add(self.pos), copy_size);
        self.pos += copy_size;
    }
}

#[inline]
fn error_ptr(message: ErrorMessage) -> *const c_char {
    message.as_ptr().cast()
}

#[inline]
fn peek(bytes: &[u8], index: usize) -> u8 {
    bytes.get(index).copied().unwrap_or(0)
}

#[inline]
fn is_digit(byte: u8) -> bool {
    byte.is_ascii_digit()
}

fn bytes_eq(bytes: &[u8], start: usize, end: usize, value: &[u8]) -> bool {
    end - start == value.len() && &bytes[start..end] == value
}

unsafe fn append_u32(dest: &mut LzmaString, mut value: u32, use_byte_suffix: bool) {
    if value == 0 {
        dest.append_bytes(b"0");
        return;
    }

    let suffixes: [&[u8]; 4] = [b"", b"KiB", b"MiB", b"GiB"];
    let mut suffix = 0usize;
    if use_byte_suffix {
        while (value & 1023) == 0 && suffix < suffixes.len() - 1 {
            value >>= 10;
            suffix += 1;
        }
    }

    let mut buf = [0u8; 16];
    let mut pos = buf.len();
    while value != 0 {
        pos -= 1;
        buf[pos] = b'0' + (value % 10) as u8;
        value /= 10;
    }

    dest.append_bytes(&buf[pos..]);
    dest.append_bytes(suffixes[suffix]);
}

unsafe fn set_option_value(filter_options: *mut c_void, spec: &OptionSpec, value: u32) {
    match spec.field {
        OptionField::StartOffset => {
            (*filter_options.cast::<lzma_options_bcj>()).start_offset = value;
        }
        OptionField::Dist => {
            (*filter_options.cast::<lzma_options_delta>()).dist = value;
        }
        OptionField::DictSize => {
            (*filter_options.cast::<lzma_options_lzma>()).dict_size = value;
        }
        OptionField::Lc => {
            (*filter_options.cast::<lzma_options_lzma>()).lc = value;
        }
        OptionField::Lp => {
            (*filter_options.cast::<lzma_options_lzma>()).lp = value;
        }
        OptionField::Pb => {
            (*filter_options.cast::<lzma_options_lzma>()).pb = value;
        }
        OptionField::Mode => {
            (*filter_options.cast::<lzma_options_lzma>()).mode = value as i32;
        }
        OptionField::NiceLen => {
            (*filter_options.cast::<lzma_options_lzma>()).nice_len = value;
        }
        OptionField::MatchFinder => {
            (*filter_options.cast::<lzma_options_lzma>()).mf = value as i32;
        }
        OptionField::Depth => {
            (*filter_options.cast::<lzma_options_lzma>()).depth = value;
        }
        OptionField::None => {}
    }
}

unsafe fn get_option_value(filter_options: *const c_void, spec: &OptionSpec) -> u32 {
    match spec.field {
        OptionField::StartOffset => (*filter_options.cast::<lzma_options_bcj>()).start_offset,
        OptionField::Dist => (*filter_options.cast::<lzma_options_delta>()).dist,
        OptionField::DictSize => (*filter_options.cast::<lzma_options_lzma>()).dict_size,
        OptionField::Lc => (*filter_options.cast::<lzma_options_lzma>()).lc,
        OptionField::Lp => (*filter_options.cast::<lzma_options_lzma>()).lp,
        OptionField::Pb => (*filter_options.cast::<lzma_options_lzma>()).pb,
        OptionField::Mode => (*filter_options.cast::<lzma_options_lzma>()).mode as u32,
        OptionField::NiceLen => (*filter_options.cast::<lzma_options_lzma>()).nice_len,
        OptionField::MatchFinder => (*filter_options.cast::<lzma_options_lzma>()).mf as u32,
        OptionField::Depth => (*filter_options.cast::<lzma_options_lzma>()).depth,
        OptionField::None => 0,
    }
}

fn find_filter_spec_by_id(id: lzma_vli) -> Option<&'static FilterSpec> {
    FILTER_NAME_MAP.iter().find(|spec| spec.id == id)
}

fn parse_lzma12_preset(
    bytes: &[u8],
    index: &mut usize,
    end: usize,
    preset: &mut u32,
) -> Option<ErrorMessage> {
    debug_assert!(*index < end);
    *preset = u32::from(bytes[*index] - b'0');

    loop {
        *index += 1;
        if *index >= end {
            return None;
        }

        match bytes[*index] {
            b'e' => *preset |= LZMA_PRESET_EXTREME,
            _ => return Some(MSG_UNSUPPORTED_PRESET_FLAG),
        }
    }
}

unsafe fn set_lzma12_preset(
    bytes: &[u8],
    index: &mut usize,
    end: usize,
    filter_options: *mut c_void,
) -> Option<ErrorMessage> {
    let mut preset = 0u32;
    let errmsg = parse_lzma12_preset(bytes, index, end, &mut preset);
    if errmsg.is_some() {
        return errmsg;
    }

    if lzma_lzma_preset_impl(filter_options.cast::<lzma_options_lzma>(), preset) != 0 {
        return Some(MSG_UNSUPPORTED_PRESET);
    }

    None
}

unsafe fn parse_options(
    bytes: &[u8],
    index: &mut usize,
    end: usize,
    filter_options: *mut c_void,
    optmap: &[OptionSpec],
) -> Option<ErrorMessage> {
    while *index < end && peek(bytes, *index) != 0 {
        if bytes[*index] == b',' {
            *index += 1;
            continue;
        }

        let mut name_eq_value_end = end;
        for pos in *index..end {
            if bytes[pos] == b',' {
                name_eq_value_end = pos;
                break;
            }
        }

        let mut equals_sign = None;
        for pos in *index..name_eq_value_end {
            if bytes[pos] == b'=' {
                equals_sign = Some(pos);
                break;
            }
        }

        let Some(equals_sign) = equals_sign else {
            return Some(MSG_OPTIONS_MUST_BE_NAME_VALUE);
        };

        if *index == equals_sign {
            return Some(MSG_OPTIONS_MUST_BE_NAME_VALUE);
        }

        let name_len = equals_sign - *index;
        if name_len > NAME_LEN_MAX {
            return Some(MSG_UNKNOWN_OPTION_NAME);
        }

        let mut spec_index = None;
        for (candidate_index, spec) in optmap.iter().enumerate() {
            if bytes_eq(bytes, *index, equals_sign, spec.name) {
                spec_index = Some(candidate_index);
                break;
            }
        }

        let Some(spec_index) = spec_index else {
            return Some(MSG_UNKNOWN_OPTION_NAME);
        };

        let spec = &optmap[spec_index];
        *index = equals_sign + 1;
        let value_len = name_eq_value_end - *index;
        if value_len == 0 {
            return Some(MSG_OPTION_VALUE_EMPTY);
        }

        if spec.kind == OptionType::LzmaPreset {
            let errmsg = set_lzma12_preset(bytes, index, name_eq_value_end, filter_options);
            if errmsg.is_some() {
                return errmsg;
            }
            continue;
        }

        let value = if (spec.flags & OPTMAP_USE_NAME_VALUE_MAP) != 0 {
            if value_len > NAME_LEN_MAX {
                return Some(MSG_INVALID_OPTION_VALUE);
            }

            let mut found = None;
            for map in spec.map {
                if bytes_eq(bytes, *index, name_eq_value_end, map.name) {
                    found = Some(map.value);
                    break;
                }
            }

            let Some(found) = found else {
                return Some(MSG_INVALID_OPTION_VALUE);
            };
            found
        } else if !is_digit(bytes[*index]) {
            return Some(MSG_NOT_NON_NEGATIVE_DECIMAL);
        } else {
            let mut p = *index;
            let mut value = 0u32;
            while p < name_eq_value_end && is_digit(bytes[p]) {
                if value > u32::MAX / 10 {
                    return Some(MSG_VALUE_OUT_OF_RANGE);
                }

                value *= 10;
                let add = u32::from(bytes[p] - b'0');
                if u32::MAX - add < value {
                    return Some(MSG_VALUE_OUT_OF_RANGE);
                }

                value += add;
                p += 1;
            }

            if p < name_eq_value_end {
                let multiplier_start = p;
                if (spec.flags & OPTMAP_USE_BYTE_SUFFIX) == 0 {
                    *index = multiplier_start;
                    return Some(MSG_NO_INTEGER_SUFFIXES);
                }

                let shift = match bytes[p] {
                    b'k' | b'K' => 10,
                    b'm' | b'M' => 20,
                    b'g' | b'G' => 30,
                    _ => {
                        *index = multiplier_start;
                        return Some(MSG_INVALID_MULTIPLIER_SUFFIX);
                    }
                };
                p += 1;

                if p < name_eq_value_end && bytes[p] == b'i' {
                    p += 1;
                }
                if p < name_eq_value_end && bytes[p] == b'B' {
                    p += 1;
                }
                if p < name_eq_value_end {
                    *index = multiplier_start;
                    return Some(MSG_INVALID_MULTIPLIER_SUFFIX);
                }
                if value > (u32::MAX >> shift) {
                    return Some(MSG_VALUE_OUT_OF_RANGE);
                }

                value <<= shift;
            }

            if value < spec.range_min || value > spec.range_max {
                return Some(MSG_VALUE_OUT_OF_RANGE);
            }

            value
        };

        set_option_value(filter_options, spec, value);
        *index = name_eq_value_end;
    }

    None
}

unsafe fn parse_bcj(
    bytes: &[u8],
    index: &mut usize,
    end: usize,
    filter_options: *mut c_void,
) -> Option<ErrorMessage> {
    parse_options(bytes, index, end, filter_options, &BCJ_OPTMAP)
}

unsafe fn parse_delta(
    bytes: &[u8],
    index: &mut usize,
    end: usize,
    filter_options: *mut c_void,
) -> Option<ErrorMessage> {
    let opts = &mut *filter_options.cast::<lzma_options_delta>();
    opts.r#type = LZMA_DELTA_TYPE_BYTE;
    opts.dist = LZMA_DELTA_DIST_MIN;
    parse_options(bytes, index, end, filter_options, &DELTA_OPTMAP)
}

unsafe fn parse_lzma12(
    bytes: &[u8],
    index: &mut usize,
    end: usize,
    filter_options: *mut c_void,
) -> Option<ErrorMessage> {
    let opts = filter_options.cast::<lzma_options_lzma>();
    let ret = lzma_lzma_preset_impl(opts, LZMA_PRESET_DEFAULT);
    debug_assert_eq!(ret, 0);

    let errmsg = parse_options(bytes, index, end, filter_options, &LZMA12_OPTMAP);
    if errmsg.is_some() {
        return errmsg;
    }

    if (*opts).lc + (*opts).lp > LZMA_LCLP_MAX {
        return Some(MSG_SUM_LC_LP);
    }

    None
}

unsafe fn parse_filter(
    bytes: &[u8],
    index: &mut usize,
    end: usize,
    filter: &mut lzma_filter,
    allocator: *const lzma_allocator,
    only_xz: bool,
) -> Option<ErrorMessage> {
    let mut name_end = end;
    let mut opts_start = end;
    for pos in *index..end {
        if bytes[pos] == b':' || bytes[pos] == b'=' {
            name_end = pos;
            opts_start = pos + 1;
            break;
        }
    }

    let name_len = name_end - *index;
    if name_len > NAME_LEN_MAX {
        return Some(MSG_UNKNOWN_FILTER_NAME);
    }

    for spec in &FILTER_NAME_MAP {
        if bytes_eq(bytes, *index, name_end, spec.name) {
            if only_xz && spec.id >= LZMA_FILTER_RESERVED_START {
                return Some(MSG_FILTER_NOT_XZ);
            }

            let options = lzma_alloc_zero(spec.opts_size, allocator);
            if options.is_null() {
                return Some(MSG_MEMORY_ALLOCATION_FAILED);
            }

            *index = opts_start;
            let errmsg = match spec.parse_kind {
                ParseKind::Bcj => parse_bcj(bytes, index, end, options),
                ParseKind::Delta => parse_delta(bytes, index, end, options),
                ParseKind::Lzma12 => parse_lzma12(bytes, index, end, options),
            };

            if let Some(errmsg) = errmsg {
                lzma_free(options, allocator);
                return Some(errmsg);
            }

            filter.id = spec.id;
            filter.options = options;
            return None;
        }
    }

    Some(MSG_UNKNOWN_FILTER_NAME)
}

unsafe fn str_to_filters_inner(
    bytes: &[u8],
    index: &mut usize,
    filters: *mut lzma_filter,
    flags: u32,
    allocator: *const lzma_allocator,
) -> Option<ErrorMessage> {
    while peek(bytes, *index) == b' ' {
        *index += 1;
    }

    if peek(bytes, *index) == 0 {
        return Some(MSG_EMPTY_STRING);
    }

    if is_digit(peek(bytes, *index))
        || (peek(bytes, *index) == b'-' && is_digit(peek(bytes, *index + 1)))
    {
        if peek(bytes, *index) == b'-' {
            *index += 1;
        }

        let mut end = bytes.len();
        for pos in *index..bytes.len() {
            if bytes[pos] == b' ' {
                end = pos;
                for tail in pos + 1..bytes.len() {
                    if bytes[tail] != b' ' {
                        return Some(MSG_UNSUPPORTED_PRESET);
                    }
                }
                break;
            }
        }

        let mut preset = 0u32;
        let errmsg = parse_lzma12_preset(bytes, index, end, &mut preset);
        if errmsg.is_some() {
            return errmsg;
        }

        let options =
            lzma_alloc(size_of::<lzma_options_lzma>(), allocator).cast::<lzma_options_lzma>();
        if options.is_null() {
            return Some(MSG_MEMORY_ALLOCATION_FAILED);
        }

        if lzma_lzma_preset_impl(options, preset) != 0 {
            lzma_free(options.cast(), allocator);
            return Some(MSG_UNSUPPORTED_PRESET);
        }

        (*filters.add(0)).id = LZMA_FILTER_LZMA2;
        (*filters.add(0)).options = options.cast();
        (*filters.add(1)).id = LZMA_VLI_UNKNOWN;
        (*filters.add(1)).options = ptr::null_mut();
        return None;
    }

    let only_xz = (flags & LZMA_STR_ALL_FILTERS) == 0;
    let mut temp_filters = [lzma_filter {
        id: LZMA_VLI_UNKNOWN,
        options: ptr::null_mut(),
    }; LZMA_FILTERS_MAX + 1];
    let mut count = 0usize;

    loop {
        if count == LZMA_FILTERS_MAX {
            filters_free_impl(temp_filters.as_mut_ptr(), allocator);
            return Some(MSG_MAX_FILTERS);
        }

        if peek(bytes, *index) == b'-' && peek(bytes, *index + 1) == b'-' {
            *index += 2;
        }

        let mut filter_end = *index;
        while peek(bytes, filter_end) != 0 {
            if (peek(bytes, filter_end) == b'-' && peek(bytes, filter_end + 1) == b'-')
                || peek(bytes, filter_end) == b' '
            {
                break;
            }
            filter_end += 1;
        }

        if filter_end == *index {
            filters_free_impl(temp_filters.as_mut_ptr(), allocator);
            return Some(MSG_FILTER_NAME_MISSING);
        }

        if let Some(errmsg) = parse_filter(
            bytes,
            index,
            filter_end,
            &mut temp_filters[count],
            allocator,
            only_xz,
        ) {
            for free_index in 0..count {
                lzma_free(temp_filters[free_index].options.cast(), allocator);
            }
            return Some(errmsg);
        }

        while peek(bytes, *index) == b' ' {
            *index += 1;
        }

        count += 1;
        if peek(bytes, *index) == 0 {
            break;
        }
    }

    temp_filters[count].id = LZMA_VLI_UNKNOWN;
    temp_filters[count].options = ptr::null_mut();

    if (flags & LZMA_STR_NO_VALIDATION) == 0 {
        let mut dummy = 0usize;
        if validate_chain_impl(temp_filters.as_ptr(), &mut dummy) != LZMA_OK {
            for free_index in 0..count {
                lzma_free(temp_filters[free_index].options.cast(), allocator);
            }
            return Some(MSG_INVALID_FILTER_CHAIN);
        }
    }

    ptr::copy_nonoverlapping(temp_filters.as_ptr(), filters, count + 1);
    None
}

unsafe fn strfy_filter(
    dest: &mut LzmaString,
    mut delimiter: &[u8],
    optmap: &[OptionSpec],
    filter_options: *const c_void,
) {
    for spec in optmap {
        if spec.kind == OptionType::LzmaPreset {
            continue;
        }

        let value = get_option_value(filter_options, spec);
        if value == 0 && (spec.flags & OPTMAP_NO_STRFY_ZERO) != 0 {
            continue;
        }

        dest.append_bytes(delimiter);
        delimiter = b",";
        dest.append_bytes(spec.name);
        dest.append_bytes(b"=");

        if (spec.flags & OPTMAP_USE_NAME_VALUE_MAP) != 0 {
            let mut found = false;
            for map in spec.map {
                if map.value == value {
                    dest.append_bytes(map.name);
                    found = true;
                    break;
                }
            }

            if !found {
                dest.append_bytes(b"UNKNOWN");
            }
        } else {
            append_u32(dest, value, (spec.flags & OPTMAP_USE_BYTE_SUFFIX) != 0);
        }
    }
}

pub(crate) unsafe fn str_to_filters_impl(
    str_ptr: *const c_char,
    error_pos: *mut c_int,
    filters: *mut lzma_filter,
    flags: u32,
    allocator: *const lzma_allocator,
) -> *const c_char {
    if str_ptr.is_null() || filters.is_null() {
        return error_ptr(MSG_STR_TO_FILTERS_NULL_ARGS);
    }

    let supported_flags = LZMA_STR_ALL_FILTERS | LZMA_STR_NO_VALIDATION;
    if (flags & !supported_flags) != 0 {
        return error_ptr(MSG_STR_TO_FILTERS_BAD_FLAGS);
    }

    let bytes = CStr::from_ptr(str_ptr).to_bytes();
    let mut used = 0usize;
    let errmsg = str_to_filters_inner(bytes, &mut used, filters, flags, allocator);

    if !error_pos.is_null() {
        let capped = used.min(c_int::MAX as usize);
        *error_pos = capped as c_int;
    }

    errmsg.map_or(ptr::null(), error_ptr)
}

pub(crate) unsafe fn str_from_filters_impl(
    output_str: *mut *mut c_char,
    filters: *const lzma_filter,
    flags: u32,
    allocator: *const lzma_allocator,
) -> lzma_ret {
    if output_str.is_null() {
        return LZMA_PROG_ERROR;
    }
    *output_str = ptr::null_mut();

    if filters.is_null() {
        return LZMA_PROG_ERROR;
    }

    let supported_flags =
        LZMA_STR_ENCODER | LZMA_STR_DECODER | LZMA_STR_GETOPT_LONG | LZMA_STR_NO_SPACES;
    if (flags & !supported_flags) != 0 {
        return LZMA_OPTIONS_ERROR;
    }

    if (*filters).id == LZMA_VLI_UNKNOWN {
        return LZMA_OPTIONS_ERROR;
    }

    let mut dest = match LzmaString::init(allocator) {
        Ok(dest) => dest,
        Err(ret) => return ret,
    };
    let show_opts = (flags & (LZMA_STR_ENCODER | LZMA_STR_DECODER)) != 0;
    let opt_delim: &[u8] = if (flags & LZMA_STR_GETOPT_LONG) != 0 {
        b"="
    } else {
        b":"
    };

    let mut i = 0usize;
    while (*filters.add(i)).id != LZMA_VLI_UNKNOWN {
        if i == LZMA_FILTERS_MAX {
            dest.free(allocator);
            return LZMA_OPTIONS_ERROR;
        }

        if i > 0 && (flags & LZMA_STR_NO_SPACES) == 0 {
            dest.append_bytes(b" ");
        }

        if (flags & LZMA_STR_GETOPT_LONG) != 0 || (i > 0 && (flags & LZMA_STR_NO_SPACES) != 0) {
            dest.append_bytes(b"--");
        }

        let Some(spec) = find_filter_spec_by_id((*filters.add(i)).id) else {
            dest.free(allocator);
            return LZMA_OPTIONS_ERROR;
        };

        dest.append_bytes(spec.name);
        if !show_opts {
            i += 1;
            continue;
        }

        if (*filters.add(i)).options.is_null() {
            if !spec.allow_null {
                dest.free(allocator);
                return LZMA_OPTIONS_ERROR;
            }

            i += 1;
            continue;
        }

        let optmap_count = if (flags & LZMA_STR_ENCODER) != 0 {
            spec.strfy_encoder
        } else {
            spec.strfy_decoder
        };

        strfy_filter(
            &mut dest,
            opt_delim,
            &spec.optmap[..optmap_count],
            (*filters.add(i)).options.cast_const(),
        );
        i += 1;
    }

    dest.finish(output_str, allocator)
}

pub(crate) unsafe fn str_list_filters_impl(
    output_str: *mut *mut c_char,
    filter_id: lzma_vli,
    flags: u32,
    allocator: *const lzma_allocator,
) -> lzma_ret {
    if output_str.is_null() {
        return LZMA_PROG_ERROR;
    }
    *output_str = ptr::null_mut();

    let supported_flags =
        LZMA_STR_ALL_FILTERS | LZMA_STR_ENCODER | LZMA_STR_DECODER | LZMA_STR_GETOPT_LONG;
    if (flags & !supported_flags) != 0 {
        return LZMA_OPTIONS_ERROR;
    }

    let mut dest = match LzmaString::init(allocator) {
        Ok(dest) => dest,
        Err(ret) => return ret,
    };
    let show_opts = (flags & (LZMA_STR_ENCODER | LZMA_STR_DECODER)) != 0;
    let filter_delim: &[u8] = if show_opts { b"\n" } else { b" " };
    let opt_delim: &[u8] = if (flags & LZMA_STR_GETOPT_LONG) != 0 {
        b"="
    } else {
        b":"
    };
    let mut first = true;

    for spec in &FILTER_NAME_MAP {
        if filter_id != LZMA_VLI_UNKNOWN && filter_id != spec.id {
            continue;
        }

        if spec.id >= LZMA_FILTER_RESERVED_START
            && (flags & LZMA_STR_ALL_FILTERS) == 0
            && filter_id == LZMA_VLI_UNKNOWN
        {
            continue;
        }

        if !first {
            dest.append_bytes(filter_delim);
        }
        first = false;

        if (flags & LZMA_STR_GETOPT_LONG) != 0 {
            dest.append_bytes(b"--");
        }

        dest.append_bytes(spec.name);
        if !show_opts {
            continue;
        }

        let end = if (flags & LZMA_STR_ENCODER) != 0 {
            spec.strfy_encoder
        } else {
            spec.strfy_decoder
        };
        let mut delimiter = opt_delim;
        for option in &spec.optmap[..end] {
            dest.append_bytes(delimiter);
            delimiter = b",";
            dest.append_bytes(option.name);
            dest.append_bytes(b"=<");

            if option.kind == OptionType::LzmaPreset {
                dest.append_bytes(LZMA12_PRESET_STR);
            } else if (option.flags & OPTMAP_USE_NAME_VALUE_MAP) != 0 {
                for (map_index, map) in option.map.iter().enumerate() {
                    if map_index > 0 {
                        dest.append_bytes(b"|");
                    }
                    dest.append_bytes(map.name);
                }
            } else {
                append_u32(
                    &mut dest,
                    option.range_min,
                    (option.flags & OPTMAP_USE_BYTE_SUFFIX) != 0,
                );
                dest.append_bytes(b"-");
                append_u32(
                    &mut dest,
                    option.range_max,
                    (option.flags & OPTMAP_USE_BYTE_SUFFIX) != 0,
                );
            }

            dest.append_bytes(b">");
        }
    }

    if first {
        dest.free(allocator);
        return LZMA_OPTIONS_ERROR;
    }

    dest.finish(output_str, allocator)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::{CStr, CString};

    #[test]
    fn stringifies_lzma2_encoder_options_byte_for_byte() {
        let input = CString::new("lzma2").unwrap();
        let mut filters = [lzma_filter {
            id: 0,
            options: ptr::null_mut(),
        }; LZMA_FILTERS_MAX + 1];
        let mut output = ptr::null_mut();

        unsafe {
            assert!(str_to_filters_impl(
                input.as_ptr(),
                ptr::null_mut(),
                filters.as_mut_ptr(),
                0,
                ptr::null()
            )
            .is_null());
            assert_eq!(
                str_from_filters_impl(&mut output, filters.as_ptr(), LZMA_STR_ENCODER, ptr::null()),
                LZMA_OK
            );
            assert_eq!(
                CStr::from_ptr(output).to_str().unwrap(),
                "lzma2:dict=8MiB,lc=3,lp=0,pb=2,mode=normal,nice=64,mf=bt4,depth=0"
            );
            libc::free(output.cast());
            filters_free_impl(filters.as_mut_ptr(), ptr::null());
        }
    }

    #[test]
    fn reports_expected_error_position_for_unknown_option() {
        let input = CString::new("lzma2:foo=1").unwrap();
        let mut filters = [lzma_filter {
            id: 0,
            options: ptr::null_mut(),
        }; LZMA_FILTERS_MAX + 1];
        let mut error_pos = -1;

        unsafe {
            let result = str_to_filters_impl(
                input.as_ptr(),
                &mut error_pos,
                filters.as_mut_ptr(),
                0,
                ptr::null(),
            );
            assert!(!result.is_null());
        }

        assert_eq!(error_pos, 6);
    }

    #[test]
    fn lists_single_filter_without_all_filters_flag() {
        let mut output = ptr::null_mut();

        unsafe {
            assert_eq!(
                str_list_filters_impl(&mut output, LZMA_FILTER_LZMA1, 0, ptr::null()),
                LZMA_OK
            );
            assert_eq!(CStr::from_ptr(output).to_str().unwrap(), "lzma1");
            libc::free(output.cast());
        }
    }
}
