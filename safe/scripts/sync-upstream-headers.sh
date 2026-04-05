#!/usr/bin/env bash
set -euo pipefail

script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
repo_root=$(cd "$script_dir/../.." && pwd)
src_dir="$repo_root/original/src/liblzma/api"
dest_dir="$repo_root/safe/include"

mkdir -p "$dest_dir/lzma"
rm -f "$dest_dir/lzma.h"
rm -rf "$dest_dir/lzma"
mkdir -p "$dest_dir/lzma"

command cp -a "$src_dir/lzma.h" "$dest_dir/lzma.h"
command cp -a "$src_dir/lzma/." "$dest_dir/lzma/"
