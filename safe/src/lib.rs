#![allow(non_camel_case_types)]

pub mod ffi;

pub use ffi::types::*;

#[cfg(all(liblzma_linux, target_family = "unix"))]
core::arch::global_asm!(include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/abi/linux_symver_aliases.S"
)));
