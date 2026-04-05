use crate::ffi::types::{lzma_ret, lzma_stream};
use crate::internal::upstream;

pub(crate) unsafe fn lzip_decoder(strm: *mut lzma_stream, memlimit: u64, flags: u32) -> lzma_ret {
    upstream::lzip_decoder(strm, memlimit, flags)
}
