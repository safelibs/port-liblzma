# Safety Audit

This final hardening and signoff pass audited the Rust `liblzma` port against
the retained non-memory-corruption scope in `relevant_cves.json`. That scope is
intentionally centered on CVE-2024-3094: release integrity, supply-chain
control, and proof that shipped artifacts come from auditable tracked sources.

The final audit kept two goals in scope:

1. Keep `unsafe` confined to the ABI and layout boundaries that the public `liblzma` contract makes unavoidable.
2. Prove the shipped `safe/` artifacts are built from auditable, tracked source inputs and do not functionally depend on `original/`, `build/`, or `cmake-build/` implementation code.

The allowed remaining `unsafe` categories are:

- C ABI entrypoints and callback trampolines that must accept raw pointers and lengths from external callers.
- Pointer translation between public `liblzma` C layouts and the internal Rust state that backs them.
- Symbol-version alias shims for Linux ABI compatibility.
- Narrow raw memory reads and writes where the C ABI requires byte-exact layout or caller-owned buffers.

The CRC helpers do not currently require `unsafe` intrinsics. CPU feature detection stays on the safe side of the standard library.

The shipped Rust dependency graph is intentionally small: `safe-liblzma`, vendored `libc`, and vendored `lzma-rust2`. The previous `sha2` -> `digest` -> `generic-array` chain was removed from the active build so no third-party crate with non-audited `unsafe` remains in the compiled dependency graph.

## Invariants

- Every exported `lzma_*` entrypoint treats all caller pointers as untrusted until null, size, and state checks have run.
- Internal callbacks only mutate caller-owned buffers through the `(ptr, pos, size)` triplets that the C ABI already exposes.
- Heap objects allocated through `lzma_allocator` are created and freed through the same allocator family.
- Multithreaded encoder and decoder workers only receive deep-copied filter arrays or opaque allocator handles; they do not share mutable Rust references across threads.
- The package build is expected to consume tracked files from `safe/` plus the non-code upstream documentation files `original/AUTHORS`, `original/NEWS`, and `original/THANKS`. `safe/scripts/release-verify.sh` traces this explicitly.
- The relink, symbol-check, package, release-verification, and benchmark scripts share mutable outputs under `safe/target/relink/`, `safe/target/release/`, and `safe/dist/`, so they are intentionally treated as serial-only maintenance steps.

## Remaining Unsafe Inventory

| Path | Unsafe shape | Why it remains |
| --- | --- | --- |
| `safe/src/lib.rs` | `global_asm!` symver include | Linux ABI compatibility requires symbol-version alias shims that cannot be expressed in safe Rust. |
| `safe/src/ffi/types.rs` | `unsafe extern "C" fn` pointer types in public structs | These are ABI declarations for allocator callbacks, not executable unsafe logic. |
| `safe/src/ffi/stubs.rs` | Exported `pub unsafe extern "C" fn lzma_*` wrappers | Direct public C ABI entrypoints must accept raw pointers and forward them to the validated Rust implementation. |
| `safe/fuzz/src/lib.rs` | Fuzzer ABI pointer translation and calls into the public `lzma_*` C entrypoints | The decode harness keeps the upstream OSS-Fuzz shape, so it must adapt `LLVMFuzzerTestOneInput(data, size)` to a Rust slice and drive the decoder through the same public ABI that external C callers use. |
| `safe/src/internal/block/buffer.rs` | Raw block/header/buffer pointer handling | The block buffer APIs operate on caller-owned `lzma_block` state and byte buffers. |
| `safe/src/internal/block/coder.rs` | Stream callback trampolines and coder state pointers | The stream-state ABI stores opaque coder pointers and invokes C-shaped callbacks. |
| `safe/src/internal/block/header.rs` | Byte-wise header reads/writes and `lzma_block*` access | Block headers are C layout data structures encoded directly into caller buffers. |
| `safe/src/internal/common.rs` | Allocator trampolines and raw zeroing/freeing | `lzma_allocator` is a C vtable and must be called through raw pointers. |
| `safe/src/internal/container/alone.rs` | `.lzma` coder callbacks and stream-pointer mutation | The public container API is callback-driven and pointer-based. |
| `safe/src/internal/container/auto.rs` | Auto-decoder callback forwarding | The wrapper dispatches through opaque inner coders supplied through the ABI. |
| `safe/src/internal/container/easy.rs` | Easy encode/decode buffer entrypoints | The easy APIs accept raw filter, stream, and output buffer pointers from C callers. |
| `safe/src/internal/container/lzip.rs` | Lzip decoder callback and output copies | The decoder fills caller-owned buffers and advances raw positions. |
| `safe/src/internal/container/microlzma.rs` | Microlzma coder callbacks and output copies | Same ABI constraint as the single-stream coders above. |
| `safe/src/internal/container/outqueue.rs` | Raw output-buffer copy helper | The queue drains into caller-provided `(out, out_pos, out_size)` buffers. |
| `safe/src/internal/container/stream.rs` | Stream entrypoints and output copies | These are thin ABI shims over the stream coder core. |
| `safe/src/internal/container/stream_buffer.rs` | Whole-buffer encode/decode entrypoints | The API shape is raw-pointer based by contract. |
| `safe/src/internal/container/stream_encoder_mt.rs` | Stream-mt callbacks and pointer-based block/stream state access | The threaded encoder still exposes the upstream MT ABI and writes directly into caller-owned stream buffers. |
| `safe/src/internal/container/stream_decoder_mt.rs` | Stream-mt callbacks and pointer-based block/stream state access | Same ABI constraint as the threaded encoder, on the decoder side. |
| `safe/src/internal/delta/common.rs` | Raw delta-option pointer reads | Delta options arrive as `lzma_options_delta*` from C callers. |
| `safe/src/internal/filter/common.rs` | Filter-array copy/free/validate helpers | Public filter chains are raw C arrays terminated by `LZMA_VLI_UNKNOWN`. |
| `safe/src/internal/filter/flags.rs` | Filter flag encode/decode over raw buffers | The filter flag format is a byte-level public ABI. |
| `safe/src/internal/filter/properties.rs` | Option-struct access and property bytes | Filter property blobs are defined by the C API and require direct layout control. |
| `safe/src/internal/filter/string_conv.rs` | Filter option parsing/stringification through raw option payloads | String conversion allocates and fills ABI-defined option records through raw pointers. |
| `safe/src/internal/index/core.rs` | Raw backing storage and ABI-compatible index layout translation | The index APIs expose opaque C pointers with upstream-compatible layout requirements. |
| `safe/src/internal/index/decode.rs` | Index decoder callbacks and raw index allocation | The decoder owns opaque index state behind ABI pointers. |
| `safe/src/internal/index/encode.rs` | Index encoder callbacks | The encoder is registered through the stream-state ABI callback table. |
| `safe/src/internal/index/file_info.rs` | File-info decoder callback state and seek buffers | This API is callback-driven and mutates caller-owned positions and temporary buffers. |
| `safe/src/internal/index/hash.rs` | Opaque hash pointer translation | The public hash object is passed around as an opaque C allocation. |
| `safe/src/internal/index/iter.rs` | Iterator union field access and zero-init | The public iterator stores internal state in ABI-defined raw slots. |
| `safe/src/internal/lzma/common.rs` | Filter-chain parsing from raw `lzma_filter*` | The parser has to reinterpret C-visible option payloads. |
| `safe/src/internal/preset.rs` | In-place initialization of `lzma_options_lzma` | Preset application writes directly into caller-provided option structs. |
| `safe/src/internal/simple/common.rs` | BCJ option pointer reads | BCJ options are optional C payload pointers. |
| `safe/src/internal/stream_flags.rs` | Stream header/footer byte encoding | The on-wire stream flag format is defined at the byte level. |
| `safe/src/internal/stream_state.rs` | Opaque `lzma_stream.internal` allocation and callback dispatch | The stream core stores Rust state behind the public C `internal` pointer. |
| `safe/src/internal/upstream.rs` | Pointer-to-slice translation, raw buffer copies, callback trampolines | This bridge layer adapts the public ABI to the pure-Rust codec core. |
| `safe/src/internal/vli.rs` | Direct VLI byte-buffer reads and writes | The VLI encode/decode API is explicitly buffer-and-position based. |

## What This Audit Removed

- Cargo-driven release and package builds now run `--offline --locked`.
- The XZ SHA-256 checksum path in vendored `lzma-rust2` now uses a local safe implementation, removing the `sha2` and `generic-array` dependency chain from shipped builds.
- The multithreaded workers now receive owned filter handles and opaque allocator addresses, removing the previous marker-trait exemption.
- Stale Rust re-exports that were generating warning noise were removed so the hardening gate is easier to interpret.

## Performance Triage

`safe/scripts/benchmark.sh` was rerun on 2026-04-06 against
`build/src/liblzma/.libs/liblzma.so.5.4.5`.

- `encode-text`: `0.245x` reference throughput
- `encode-random`: `1.072x` reference throughput
- `decode-text`: `0.153x` reference throughput
- `decode-random`: `0.096x` reference throughput

The final benchmark gate is not an upstream-parity requirement. It uses
workload-specific signoff floors derived from the accepted port baseline:
`encode-text >= 0.20x`, `encode-random >= 0.95x`, `decode-text >= 0.12x`, and
`decode-random >= 0.08x` reference throughput. That keeps materially worse
future regressions visible without failing the final gate on the already-audited
codec hot-path gap.

## Verification Hooks

- `safe/scripts/release-verify.sh` is the authoritative final-release proof. It traces the package build, rejects forbidden implementation inputs, checks tracked/textual source provenance for the files the build actually consumes, compares installed headers and symbol maps to the authoritative upstream originals, and confirms the packaged library matches the freshly built Rust artifact.
- `safe/scripts/run-rust-unit-tests.sh` and `safe/scripts/release-verify.sh` both exercise `safe/fuzz/` with locked offline Cargo resolution so the decode-focused harness stays in the final gate instead of drifting as a standalone workspace.
- `safe/fuzz/` contains a decode-focused harness with the same 300 MiB memory limit posture as the upstream OSS-Fuzz target.
- `safe/scripts/benchmark.sh` compares the Rust library against `build/src/liblzma/.libs/liblzma.so.5.4.5` on representative encode and decode workloads and enforces the accepted signoff floors for each workload unless a caller overrides them explicitly.
- `safe/scripts/relink-release-shared.sh` owns the shared relink path, and the related release/package/benchmark scripts document that those steps are serial-only so future maintainers do not overlap them in one worktree.
