#!/usr/bin/env bash
set -euo pipefail

# Serial-only script: this rewrites safe/target/relink/ and
# safe/target/release/liblzma.so{,.5}. Do not run it concurrently with
# release-verify.sh, benchmark.sh, compare-exports.sh, check-symbol-versions.sh,
# build-deb.sh, or any other command that repackages safe/dist/ from the same
# worktree.

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
compat_aliases="$compat_dir/linux_symver_compat.S"
compat_aliases_obj="$compat_dir/linux_symver_compat.o"
compat_alias_args=()
archive_rewrite_dir="$compat_dir/archive-rewrite"

split_flags() {
  local value="$1"
  local -n out_ref="$2"

  out_ref=()
  if [[ -n "$value" ]]; then
    # Debian passes compiler/linker flags through the environment.
    read -r -a out_ref <<<"$value"
  fi
}

cppflags=()
cflags=()
ldflags=()
split_flags "${CPPFLAGS:-}" cppflags
split_flags "${CFLAGS:-}" cflags
split_flags "${LDFLAGS:-}" ldflags

mkdir -p "$compat_dir"

if [[ "${LIBLZMA_SKIP_CARGO_BUILD:-0}" != "1" ]]; then
  cargo build --manifest-path "$safe_dir/Cargo.toml" --offline --locked --release >/dev/null
fi

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
if ! objcopy --redefine-syms="$redefine_syms" "$compat_archive" 2>/dev/null; then
  rm -rf "$archive_rewrite_dir"
  mkdir -p "$archive_rewrite_dir"

  mapfile -t archive_members < <(ar t "$compat_archive")
  (
    cd "$archive_rewrite_dir"
    ar x "$compat_archive"

    for member in "${archive_members[@]}"; do
      if nm -g --defined-only "$member" 2>/dev/null | grep -Eq \
        ' T lzma_(block_uncomp_encode|cputhreads|get_progress|stream_encoder_mt|stream_encoder_mt_memusage)$'; then
        objcopy --redefine-syms="$redefine_syms" "$member"
      fi
    done

    rm -f "$compat_archive"
    ar crs "$compat_archive" "${archive_members[@]}"
  )
fi

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

cc "${cppflags[@]}" "${cflags[@]}" -fPIC -c "$default_aliases" -o "$default_aliases_obj"

if ! (nm -g --defined-only "$static_lib" 2>/dev/null || true) \
  | grep -q '__symver_lzma_block_uncomp_encode_XZ_5_2_2'; then
  cat > "$compat_aliases" <<'EOF'
    .text

    .macro version_compat alias, exported, target
    .globl \alias
    .type \alias, @function
\alias:
    jmp \target
    .size \alias, .-\alias
    .symver \alias, \exported
    .endm

    version_compat __symver_lzma_stream_encoder_mt_XZ_5_1_2alpha, lzma_stream_encoder_mt@XZ_5.1.2alpha, __safe_impl_lzma_stream_encoder_mt
    version_compat __symver_lzma_stream_encoder_mt_memusage_XZ_5_1_2alpha, lzma_stream_encoder_mt_memusage@XZ_5.1.2alpha, __safe_impl_lzma_stream_encoder_mt_memusage

    version_compat __symver_lzma_block_uncomp_encode_XZ_5_2_2, lzma_block_uncomp_encode@XZ_5.2.2, __safe_impl_lzma_block_uncomp_encode
    version_compat __symver_lzma_cputhreads_XZ_5_2_2, lzma_cputhreads@XZ_5.2.2, __safe_impl_lzma_cputhreads
    version_compat __symver_lzma_get_progress_XZ_5_2_2, lzma_get_progress@XZ_5.2.2, __safe_impl_lzma_get_progress
    version_compat __symver_lzma_stream_encoder_mt_XZ_5_2_2, lzma_stream_encoder_mt@XZ_5.2.2, __safe_impl_lzma_stream_encoder_mt
    version_compat __symver_lzma_stream_encoder_mt_memusage_XZ_5_2_2, lzma_stream_encoder_mt_memusage@XZ_5.2.2, __safe_impl_lzma_stream_encoder_mt_memusage

    .section .note.GNU-stack,"",@progbits
EOF

  cc "${cppflags[@]}" "${cflags[@]}" -fPIC -c "$compat_aliases" -o "$compat_aliases_obj"
  compat_alias_args=("$compat_aliases_obj")
fi

cc -shared \
  "${cflags[@]}" \
  "${ldflags[@]}" \
  -o "$shared_lib" \
  "$default_aliases_obj" \
  "${compat_alias_args[@]}" \
  -Wl,--whole-archive "$compat_archive" -Wl,--no-whole-archive \
  -Wl,--version-script="$compat_map" \
  -Wl,-soname,liblzma.so.5 \
  -ldl \
  -lpthread \
  -lm \
  -lc \
  -lgcc_s

ln -sf "liblzma.so" "$target_dir/liblzma.so.5"
