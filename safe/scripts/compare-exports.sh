#!/usr/bin/env bash
set -euo pipefail

script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
repo_root=$(cd "$script_dir/../.." && pwd)
safe_lib="${1:-$repo_root/safe/target/release/liblzma.so}"
ref_lib="$repo_root/build/src/liblzma/.libs/liblzma.so.5.4.5"
map_file="$repo_root/safe/abi/liblzma_linux.map"

"$script_dir/relink-release-shared.sh" >/dev/null

tmpdir=$(mktemp -d)
trap 'rm -rf "$tmpdir"' EXIT

extract_map() {
  awk '
    /^[[:space:]]*lzma_[A-Za-z0-9_]+;[[:space:]]*$/ {
      gsub(/[;[:space:]]/, "", $0)
      print $0
    }
  ' "$1" | sort -u
}

extract_nm() {
  nm -D --defined-only "$1" \
    | awk '$3 ~ /^lzma_/ { sub(/@.*/, "", $3); print $3 }' \
    | sort -u
}

extract_dyn() {
  readelf --dyn-syms --wide "$1" \
    | awk '$8 ~ /^lzma_/ { name = $8; sub(/@.*/, "", name); print name }' \
    | sort -u
}

extract_map "$map_file" > "$tmpdir/map.txt"
extract_nm "$ref_lib" > "$tmpdir/ref-nm.txt"
extract_nm "$safe_lib" > "$tmpdir/safe-nm.txt"
extract_dyn "$ref_lib" > "$tmpdir/ref-dyn.txt"
extract_dyn "$safe_lib" > "$tmpdir/safe-dyn.txt"

diff -u "$tmpdir/map.txt" "$tmpdir/safe-nm.txt"
diff -u "$tmpdir/ref-nm.txt" "$tmpdir/safe-nm.txt"
diff -u "$tmpdir/ref-dyn.txt" "$tmpdir/safe-dyn.txt"
diff -u "$tmpdir/safe-nm.txt" "$tmpdir/safe-dyn.txt"
