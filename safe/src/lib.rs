#![allow(non_camel_case_types)]

pub mod ffi;
mod internal {
    #![allow(dead_code)]

    pub(crate) mod block;
    pub(crate) mod check;
    pub(crate) mod common;
    pub(crate) mod container;
    pub(crate) mod filter;
    pub(crate) mod hardware;
    pub(crate) mod index;
    pub(crate) mod lz;
    pub(crate) mod lzma;
    pub(crate) mod preset;
    pub(crate) mod rangecoder;
    pub(crate) mod delta;
    pub(crate) mod simple;
    pub(crate) mod stream_flags;
    pub(crate) mod stream_state;
    pub(crate) mod upstream;
    pub(crate) mod vli;
}

pub use ffi::types::*;

#[cfg(all(liblzma_linux, target_family = "unix"))]
core::arch::global_asm!(include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/abi/linux_symver_aliases.S"
)));
