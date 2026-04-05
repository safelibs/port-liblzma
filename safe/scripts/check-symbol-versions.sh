#!/usr/bin/env bash
set -euo pipefail

script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
repo_root=$(cd "$script_dir/../.." && pwd)
safe_lib="${1:-$repo_root/safe/target/release/liblzma.so}"
ref_lib="$repo_root/build/src/liblzma/.libs/liblzma.so.5.4.5"

"$script_dir/relink-release-shared.sh" >/dev/null

tmpdir=$(mktemp -d)
trap 'rm -rf "$tmpdir"' EXIT

extract_versioned_dyn() {
  readelf --dyn-syms --wide "$1" \
    | awk '$8 ~ /^(lzma_|XZ_)/ { print $8 }' \
    | sort
}

extract_verdef_graph() {
  readelf --version-info --wide "$1" \
    | awk '
      /Version definition section/ { in_defs = 1; next }
      /Version needs section/ { in_defs = 0 }
      in_defs && /Name:/ {
        for (i = 1; i <= NF; ++i) {
          if ($i == "Name:") {
            current = $(i + 1)
            if (current ~ /^(liblzma\\.so\\.5|XZ_)/) {
              print "node\t" current
            }
          }
        }
      }
      in_defs && /Parent 1:/ {
        print "parent\t" current "\t" $3
      }
    ' | sort
}

extract_soname() {
  readelf -d "$1" | sed -n 's/.*Library soname: \\[\\(.*\\)\\].*/\\1/p'
}

extract_versioned_dyn "$ref_lib" > "$tmpdir/ref-dyn.txt"
extract_versioned_dyn "$safe_lib" > "$tmpdir/safe-dyn.txt"
extract_verdef_graph "$ref_lib" > "$tmpdir/ref-verdef.txt"
extract_verdef_graph "$safe_lib" > "$tmpdir/safe-verdef.txt"
extract_soname "$ref_lib" > "$tmpdir/ref-soname.txt"
extract_soname "$safe_lib" > "$tmpdir/safe-soname.txt"

diff -u "$tmpdir/ref-dyn.txt" "$tmpdir/safe-dyn.txt"
diff -u "$tmpdir/ref-verdef.txt" "$tmpdir/safe-verdef.txt"
diff -u "$tmpdir/ref-soname.txt" "$tmpdir/safe-soname.txt"
