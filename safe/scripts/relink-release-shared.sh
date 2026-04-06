#!/usr/bin/env bash
set -euo pipefail

script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
repo_root=$(cd "$script_dir/../.." && pwd)
safe_dir="$repo_root/safe"
target_dir="$safe_dir/target/release"
static_lib="$target_dir/liblzma.a"
shared_lib="$target_dir/liblzma.so"
compat_dir="$safe_dir/target/relink"
compat_archive="$compat_dir/liblzma.relink.a"
compat_map="$compat_dir/liblzma_linux.relink.map"
redefine_syms="$compat_dir/redefine-public-symbols.txt"
default_aliases="$compat_dir/linux_symver_defaults.S"
default_aliases_obj="$compat_dir/linux_symver_defaults.o"

mkdir -p "$compat_dir"

cargo build --manifest-path "$safe_dir/Cargo.toml" --release >/dev/null

REPO_ROOT="$repo_root" python3 - <<'PY'
import os
from pathlib import Path

root = Path(os.environ["REPO_ROOT"])
src = root / "safe/abi/liblzma_linux.map"
dst = root / "safe/target/relink/liblzma_linux.relink.map"
skip = {
    "lzma_block_uncomp_encode",
    "lzma_cputhreads",
    "lzma_get_progress",
    "lzma_stream_encoder_mt",
    "lzma_stream_encoder_mt_memusage",
}

out = []
current = None
block = []

for line in src.read_text().splitlines():
    stripped = line.strip()
    if current is None:
        if stripped.startswith("XZ_") and stripped.endswith("{"):
            current = stripped[:-1].strip()
            block = []
        else:
            out.append(line)
        continue

    if stripped.startswith("}"):
        parent = stripped[1:].strip().rstrip(";") or None
        kept = [entry for entry in block if entry.strip().rstrip(";") not in skip]

        out.append(f"{current} {{")
        if kept:
            out.append("global:")
            out.extend(kept)
            if current == "XZ_5.0":
                out.append("")
                out.append("local:")
                out.append("\t*;")
        elif current == "XZ_5.0":
            out.append("global:")
            out.append("")
            out.append("local:")
            out.append("\t*;")

        closing = "}"
        if parent:
            closing += f" {parent}"
        closing += ";"
        out.append(closing)
        out.append("")
        current = None
        block = []
        continue

    if stripped in {"global:", "local:", "*;"}:
        continue

    block.append(line)

dst.write_text("\n".join(out).rstrip() + "\n")
PY

cat > "$redefine_syms" <<'EOF'
lzma_block_uncomp_encode __safe_impl_lzma_block_uncomp_encode
lzma_cputhreads __safe_impl_lzma_cputhreads
lzma_get_progress __safe_impl_lzma_get_progress
lzma_stream_encoder_mt __safe_impl_lzma_stream_encoder_mt
lzma_stream_encoder_mt_memusage __safe_impl_lzma_stream_encoder_mt_memusage
EOF

cp "$static_lib" "$compat_archive"
objcopy --redefine-syms="$redefine_syms" "$compat_archive"

cat > "$default_aliases" <<'EOF'
    .text

    .macro version_default alias, exported, target
    .globl \alias
    .type \alias, @function
\alias:
    jmp \target
    .size \alias, .-\alias
    .symver \alias, \exported
    .endm

    version_default __symver_default_lzma_block_uncomp_encode_XZ_5_2, lzma_block_uncomp_encode@@XZ_5.2, __safe_impl_lzma_block_uncomp_encode
    version_default __symver_default_lzma_cputhreads_XZ_5_2, lzma_cputhreads@@XZ_5.2, __safe_impl_lzma_cputhreads
    version_default __symver_default_lzma_get_progress_XZ_5_2, lzma_get_progress@@XZ_5.2, __safe_impl_lzma_get_progress
    version_default __symver_default_lzma_stream_encoder_mt_XZ_5_2, lzma_stream_encoder_mt@@XZ_5.2, __safe_impl_lzma_stream_encoder_mt
    version_default __symver_default_lzma_stream_encoder_mt_memusage_XZ_5_2, lzma_stream_encoder_mt_memusage@@XZ_5.2, __safe_impl_lzma_stream_encoder_mt_memusage

    .section .note.GNU-stack,"",@progbits
EOF

cc -fPIC -c "$default_aliases" -o "$default_aliases_obj"

cc -shared \
  -o "$shared_lib" \
  "$default_aliases_obj" \
  -Wl,--whole-archive "$compat_archive" -Wl,--no-whole-archive \
  -Wl,--version-script="$compat_map" \
  -Wl,-soname,liblzma.so.5 \
  -ldl \
  -lpthread \
  -lm \
  -lc \
  -lgcc_s

ln -sf "liblzma.so" "$target_dir/liblzma.so.5"
