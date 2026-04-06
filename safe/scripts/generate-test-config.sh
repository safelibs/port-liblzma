#!/usr/bin/env bash
set -euo pipefail

script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
repo_root=$(cd "$script_dir/../.." && pwd)
src="$repo_root/build/config.h"
dest="$repo_root/safe/tests/generated/config.h"
tmp="$dest.tmp"

mkdir -p "$(dirname "$dest")"

if [[ ! -f "$src" ]]; then
  printf 'missing upstream config header: %s\n' "$src" >&2
  exit 1
fi

awk '
{ print }
END {
  print ""
  print "/* Phase 06: single-threaded container APIs and the upstream helper harness are enabled. */"
}
' "$src" > "$tmp"

mv "$tmp" "$dest"
