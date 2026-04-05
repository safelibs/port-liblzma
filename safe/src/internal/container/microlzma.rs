use crate::ffi::types::{lzma_bool, lzma_options_lzma, lzma_ret, lzma_stream};
use crate::internal::upstream;

pub(crate) unsafe fn microlzma_encoder(
    strm: *mut lzma_stream,
    options: *const lzma_options_lzma,
) -> lzma_ret {
    upstream::microlzma_encoder(strm, options)
}

pub(crate) unsafe fn microlzma_decoder(
    strm: *mut lzma_stream,
    comp_size: u64,
    uncomp_size: u64,
    uncomp_size_is_exact: lzma_bool,
    dict_size: u32,
) -> lzma_ret {
    upstream::microlzma_decoder(
        strm,
        comp_size,
        uncomp_size,
        uncomp_size_is_exact,
        dict_size,
    )
}
