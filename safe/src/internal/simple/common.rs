use crate::ffi::types::{lzma_options_bcj, lzma_ret, lzma_vli, LZMA_OPTIONS_ERROR};
use crate::internal::filter::common::{
    LZMA_FILTER_ARM, LZMA_FILTER_ARM64, LZMA_FILTER_ARMTHUMB, LZMA_FILTER_IA64,
    LZMA_FILTER_POWERPC, LZMA_FILTER_SPARC, LZMA_FILTER_X86,
};

#[derive(Copy, Clone, Eq, PartialEq)]
pub(crate) enum SimpleFilterKind {
    X86,
    PowerPc,
    Ia64,
    Arm,
    ArmThumb,
    Arm64,
    Sparc,
}

impl SimpleFilterKind {
    pub(crate) const fn alignment(self) -> u32 {
        match self {
            Self::X86 => crate::internal::simple::x86::ALIGNMENT,
            Self::PowerPc => crate::internal::simple::powerpc::ALIGNMENT,
            Self::Ia64 => crate::internal::simple::ia64::ALIGNMENT,
            Self::Arm => crate::internal::simple::arm::ALIGNMENT,
            Self::ArmThumb => crate::internal::simple::armthumb::ALIGNMENT,
            Self::Arm64 => crate::internal::simple::arm64::ALIGNMENT,
            Self::Sparc => crate::internal::simple::sparc::ALIGNMENT,
        }
    }
}

pub(crate) const fn kind_from_filter_id(id: lzma_vli) -> Option<SimpleFilterKind> {
    match id {
        LZMA_FILTER_X86 => Some(SimpleFilterKind::X86),
        LZMA_FILTER_POWERPC => Some(SimpleFilterKind::PowerPc),
        LZMA_FILTER_IA64 => Some(SimpleFilterKind::Ia64),
        LZMA_FILTER_ARM => Some(SimpleFilterKind::Arm),
        LZMA_FILTER_ARMTHUMB => Some(SimpleFilterKind::ArmThumb),
        LZMA_FILTER_ARM64 => Some(SimpleFilterKind::Arm64),
        LZMA_FILTER_SPARC => Some(SimpleFilterKind::Sparc),
        _ => None,
    }
}

pub(crate) unsafe fn options_to_start_offset(
    kind: SimpleFilterKind,
    options: *const lzma_options_bcj,
) -> Result<u32, lzma_ret> {
    let offset = if options.is_null() {
        0
    } else {
        (*options).start_offset
    };
    validate_alignment(kind, offset)?;
    Ok(offset)
}

pub(crate) fn validate_alignment(
    kind: SimpleFilterKind,
    start_offset: u32,
) -> Result<(), lzma_ret> {
    if start_offset % kind.alignment() != 0 {
        return Err(LZMA_OPTIONS_ERROR);
    }

    Ok(())
}
