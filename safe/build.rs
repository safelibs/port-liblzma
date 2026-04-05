use std::env;
use std::path::PathBuf;

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let abi_dir = manifest_dir.join("abi");
    let linux_map = abi_dir.join("liblzma_linux.map");
    let generic_map = abi_dir.join("liblzma_generic.map");
    let alias_asm = abi_dir.join("linux_symver_aliases.S");

    println!("cargo:rustc-check-cfg=cfg(liblzma_linux)");
    println!("cargo:rerun-if-changed={}", linux_map.display());
    println!("cargo:rerun-if-changed={}", generic_map.display());
    println!("cargo:rerun-if-changed={}", alias_asm.display());
    println!("cargo:rerun-if-changed=src/lib.rs");
    println!("cargo:rerun-if-changed=src/ffi/mod.rs");
    println!("cargo:rerun-if-changed=src/ffi/types.rs");
    println!("cargo:rerun-if-changed=src/ffi/stubs.rs");

    match env::var("CARGO_CFG_TARGET_OS").as_deref() {
        Ok("linux") => {
            println!("cargo:rustc-cfg=liblzma_linux");
            println!(
                "cargo:rustc-cdylib-link-arg=-Wl,--version-script={}",
                linux_map.display()
            );
            println!("cargo:rustc-cdylib-link-arg=-Wl,-soname,liblzma.so.5");
        }
        _ => {
            println!(
                "cargo:rustc-cdylib-link-arg=-Wl,--version-script={}",
                generic_map.display()
            );
        }
    }
}
