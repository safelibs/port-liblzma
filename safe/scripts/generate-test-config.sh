#!/usr/bin/env bash
set -euo pipefail

script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
repo_root=$(cd "$script_dir/../.." && pwd)
src="$repo_root/build/config.h"
dest="$repo_root/safe/tests/generated/config.h"

mkdir -p "$(dirname "$dest")"

awk '
BEGIN {
  phase03["HAVE_LZIP_DECODER"] = 1
}
/^#define / {
  macro = $2
  if (macro in phase03) {
    print "/* #undef " macro " */"
    next
  }
}
{ print }
END {
  print ""
  print "/* Phase 03: filter metadata, properties, string conversion, and stream flag helpers are implemented. */"
  print "/* Keep lzip disabled until the corresponding decoder surface exists in Rust. */"
}
' "$src" > "$dest"
