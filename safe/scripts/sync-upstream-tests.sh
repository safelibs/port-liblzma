#!/usr/bin/env bash
set -euo pipefail

script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
repo_root=$(cd "$script_dir/../.." && pwd)
src_dir="$repo_root/original/tests"
dest_dir="$repo_root/safe/tests/upstream"

rm -rf "$dest_dir"
mkdir -p "$dest_dir"
command cp -a "$src_dir/." "$dest_dir/"
