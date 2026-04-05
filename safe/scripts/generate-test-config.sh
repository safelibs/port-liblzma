#!/usr/bin/env bash
set -euo pipefail

script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
repo_root=$(cd "$script_dir/../.." && pwd)
src="$repo_root/build/config.h"
dest="$repo_root/safe/tests/generated/config.h"

mkdir -p "$(dirname "$dest")"

awk '
{ print }
END {
  print ""
  print "/* Phase 05: low-level block/raw helpers and special container codecs are enabled. */"
}
' "$src" > "$dest"
