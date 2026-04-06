#!/usr/bin/env bash
set -euo pipefail

# Serial-only script: this cleans and rebuilds safe/target/, rewrites
# safe/target/relink/, and repackages safe/dist/. Run it on its own and do not
# overlap it with relink-release-shared.sh, benchmark.sh, compare-exports.sh,
# check-symbol-versions.sh, or other package/release jobs in the same worktree.

script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
safe_dir=$(cd "$script_dir/.." && pwd)
repo_root=$(cd "$safe_dir/.." && pwd)
dist_dir="$safe_dir/dist"
tmpdir=$(mktemp -d)
trap 'rm -rf "$tmpdir"' EXIT

require_tool() {
  command -v "$1" >/dev/null 2>&1 || {
    printf 'missing required tool: %s\n' "$1" >&2
    exit 1
  }
}

log_step() {
  printf '\n==> %s\n' "$1"
}

clean_generated() {
  cargo clean --manifest-path "$safe_dir/Cargo.toml" >/dev/null 2>&1 || true
  cargo clean --manifest-path "$safe_dir/fuzz/Cargo.toml" >/dev/null 2>&1 || true

  rm -rf \
    "$safe_dir/debian/.debhelper" \
    "$safe_dir/debian/cargo-home" \
    "$safe_dir/debian/liblzma-dev" \
    "$safe_dir/debian/liblzma5" \
    "$safe_dir/debian/tmp"
  rm -f \
    "$safe_dir/debian/"*.debhelper.log \
    "$safe_dir/debian/"*.substvars \
    "$safe_dir/debian/debhelper-build-stamp" \
    "$safe_dir/debian/files"
  rm -f \
    "$dist_dir"/liblzma5_*.deb \
    "$dist_dir"/liblzma-dev_*.deb \
    "$dist_dir"/liblzma-safe_*.buildinfo \
    "$dist_dir"/liblzma-safe_*.changes \
    "$repo_root"/liblzma5_*.deb \
    "$repo_root"/liblzma-dev_*.deb \
    "$repo_root"/liblzma-safe_*.buildinfo \
    "$repo_root"/liblzma-safe_*.changes
}

require_tool cargo
require_tool ar
require_tool cmp
require_tool diff
require_tool dpkg-deb
require_tool dpkg-architecture
require_tool git
require_tool nm
require_tool python3
require_tool readelf
require_tool strace

compare_dynsym() {
  local expected=$1
  local actual=$2
  local label=$3

  readelf --dyn-syms --wide "$expected" \
    | awk '$5 ~ /GLOBAL|WEAK/ && $7 != "UND" { print $8 }' \
    | LC_ALL=C sort -u >"$tmpdir/$label.expected"
  readelf --dyn-syms --wide "$actual" \
    | awk '$5 ~ /GLOBAL|WEAK/ && $7 != "UND" { print $8 }' \
    | LC_ALL=C sort -u >"$tmpdir/$label.actual"
  diff -u "$tmpdir/$label.expected" "$tmpdir/$label.actual"
}

compare_archive_members() {
  local expected=$1
  local actual=$2
  local label=$3

  ar t "$expected" >"$tmpdir/$label.expected"
  ar t "$actual" >"$tmpdir/$label.actual"
  diff -u "$tmpdir/$label.expected" "$tmpdir/$label.actual"
}

compare_archive_symbols() {
  local expected=$1
  local actual=$2
  local label=$3

  nm -g --defined-only "$expected" 2>/dev/null \
    | awk 'NF >= 3 { print $NF }' \
    | LC_ALL=C sort -u >"$tmpdir/$label.expected"
  nm -g --defined-only "$actual" 2>/dev/null \
    | awk 'NF >= 3 { print $NF }' \
    | LC_ALL=C sort -u >"$tmpdir/$label.actual"
  diff -u "$tmpdir/$label.expected" "$tmpdir/$label.actual"
}

log_step "Checking authoritative headers and symbol maps"
cmp -s "$safe_dir/include/lzma.h" "$repo_root/original/src/liblzma/api/lzma.h"
for header in "$repo_root"/original/src/liblzma/api/lzma/*.h; do
  cmp -s "$safe_dir/include/lzma/$(basename "$header")" "$header"
done
cmp -s "$safe_dir/abi/liblzma_linux.map" "$repo_root/original/src/liblzma/liblzma_linux.map"
cmp -s "$safe_dir/abi/liblzma_generic.map" "$repo_root/original/src/liblzma/liblzma_generic.map"

log_step "Checking locked offline Cargo resolution"
cargo metadata --manifest-path "$safe_dir/Cargo.toml" --offline --locked --format-version=1 \
  >"$tmpdir/cargo-metadata.json"
cargo build --manifest-path "$safe_dir/Cargo.toml" --offline --locked --release >/dev/null
cargo metadata --manifest-path "$safe_dir/fuzz/Cargo.toml" --offline --locked --format-version=1 \
  >"$tmpdir/fuzz-cargo-metadata.json"
cargo test --manifest-path "$safe_dir/fuzz/Cargo.toml" --offline --locked >/dev/null
LIBLZMA_SKIP_CARGO_BUILD=1 "$script_dir/relink-release-shared.sh" >/dev/null

log_step "Tracing a clean Debian package build"
clean_generated
strace -ff -e trace=file -s 4096 -o "$tmpdir/trace" \
  "$script_dir/build-deb.sh" >/dev/null

python3 - "$repo_root" "$tmpdir" <<'PY'
import subprocess
import sys
from pathlib import Path
import re

repo_root = Path(sys.argv[1]).resolve()
trace_dir = Path(sys.argv[2]).resolve()

generated_prefixes = [
    repo_root / "safe/target",
    repo_root / "safe/debian/.debhelper",
    repo_root / "safe/debian/cargo-home",
    repo_root / "safe/debian/liblzma-dev",
    repo_root / "safe/debian/liblzma5",
    repo_root / "safe/debian/tmp",
    repo_root / "safe/dist",
    repo_root / "safe/tests/generated",
]
forbidden_prefixes = [
    repo_root / "original/src/liblzma",
    repo_root / "build",
    repo_root / "cmake-build",
]
allowed_original_files = {
    repo_root / "original/AUTHORS",
    repo_root / "original/NEWS",
    repo_root / "original/THANKS",
}
binary_signatures = (
    b"\x7fELF",
    b"!<arch>\n",
    b"\x1f\x8b",
    b"BZh",
    b"\xfd7zXZ\x00",
    b"PK\x03\x04",
)
quote_re = re.compile(r'"((?:\\.|[^"])*)"')


def normalize(raw: str) -> Path:
    return Path(raw.replace("\\\"", "\"").replace("\\\\", "\\"))


repo_inputs: set[Path] = set()
violations: list[str] = []

for trace in sorted(trace_dir.glob("trace*")):
    for lineno, line in enumerate(trace.read_text(errors="replace").splitlines(), start=1):
        for raw_path in quote_re.findall(line):
            path = normalize(raw_path)
            if not path.is_absolute():
                continue
            if not path.as_posix().startswith(repo_root.as_posix()):
                continue
            if not path.exists():
                continue
            repo_inputs.add(path)
            if any(path == prefix or path.is_relative_to(prefix) for prefix in forbidden_prefixes):
                violations.append(
                    f"{trace.name}:{lineno}: forbidden package-build input {path}"
                )

if violations:
    raise SystemExit("\n".join(violations))

for path in sorted(repo_inputs):
    if path == repo_root / "original" or path == repo_root / "safe":
        continue
    if path.as_posix().startswith((repo_root / "original").as_posix()):
        if path not in allowed_original_files and path not in {p.parent for p in allowed_original_files}:
            raise SystemExit(f"unexpected original/ access during package build: {path}")
        continue
    if not path.as_posix().startswith((repo_root / "safe").as_posix()):
        continue
    if any(path == prefix or path.is_relative_to(prefix) for prefix in generated_prefixes):
        continue
    if path.is_dir():
        continue

    rel = path.relative_to(repo_root).as_posix()
    subprocess.run(
        ["git", "-C", str(repo_root), "ls-files", "--error-unmatch", rel],
        check=True,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )
    if path.as_posix().startswith((repo_root / "safe/tests").as_posix()):
        continue
    data = path.read_bytes()
    if b"\0" in data:
        raise SystemExit(f"tracked build input contains NUL bytes: {path}")
    if any(data.startswith(sig) for sig in binary_signatures):
        raise SystemExit(f"tracked build input looks like a binary blob: {path}")
PY

log_step "Inspecting packaged contents against tracked outputs"
runtime_deb=$(find "$dist_dir" -maxdepth 1 -name 'liblzma5_*.deb' -print -quit)
dev_deb=$(find "$dist_dir" -maxdepth 1 -name 'liblzma-dev_*.deb' -print -quit)
[[ -n "$runtime_deb" && -n "$dev_deb" ]] || {
  printf 'missing built Debian packages under %s\n' "$dist_dir" >&2
  exit 1
}

multiarch=$(dpkg-architecture -qDEB_HOST_MULTIARCH)
runtime_root="$tmpdir/runtime"
dev_root="$tmpdir/dev"
dpkg-deb -x "$runtime_deb" "$runtime_root"
dpkg-deb -x "$dev_deb" "$dev_root"

cmp -s \
  "$safe_dir/target/release/liblzma.pc" \
  "$dev_root/usr/lib/$multiarch/pkgconfig/liblzma.pc"
compare_dynsym \
  "$safe_dir/target/release/liblzma.so" \
  "$runtime_root/usr/lib/$multiarch/liblzma.so.5.4.5" \
  runtime-dynsym
compare_archive_members \
  "$safe_dir/target/release/liblzma.a" \
  "$dev_root/usr/lib/$multiarch/liblzma.a" \
  static-members
compare_archive_symbols \
  "$safe_dir/target/release/liblzma.a" \
  "$dev_root/usr/lib/$multiarch/liblzma.a" \
  static-symbols
cmp -s \
  <(readelf -d "$safe_dir/target/release/liblzma.so" | grep 'SONAME') \
  <(readelf -d "$runtime_root/usr/lib/$multiarch/liblzma.so.5.4.5" | grep 'SONAME')
cmp -s "$safe_dir/include/lzma.h" "$dev_root/usr/include/lzma.h"
for header in "$safe_dir"/include/lzma/*.h; do
  cmp -s "$header" "$dev_root/usr/include/lzma/$(basename "$header")"
done

log_step "Checking packaged runtime linkage"
readelf -d "$runtime_root/usr/lib/$multiarch/liblzma.so.5.4.5" >"$tmpdir/runtime.dynamic"
if grep -Eq '/(original|build|cmake-build)/' "$tmpdir/runtime.dynamic"; then
  printf 'runtime library dynamic section refers to verification-only paths\n' >&2
  exit 1
fi

printf '\nrelease verification passed\n'
