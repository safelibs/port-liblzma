#!/usr/bin/env bash
set -euo pipefail

dir="${1:?usage: libarchive_tools_smoke.sh <workdir>}"
bsdtar_bin="${BSDTAR_BIN:-/usr/bin/bsdtar}"
bsdcat_bin="${BSDCAT_BIN:-/usr/bin/bsdcat}"

mkdir -p "$dir/input/archive"
printf 'libarchive tools tar.xz smoke\n' >"$dir/input/archive/message.txt"

"$bsdtar_bin" -acf "$dir/archive.tar.xz" -C "$dir/input" . >"$dir/create.log" 2>&1
"$bsdtar_bin" -tf "$dir/archive.tar.xz" >"$dir/list.log"

mkdir -p "$dir/output"
"$bsdtar_bin" -xf "$dir/archive.tar.xz" -C "$dir/output" >"$dir/extract.log" 2>&1

printf 'libarchive tools bsdcat smoke\n' >"$dir/payload.txt"
xz -9 -c "$dir/payload.txt" >"$dir/payload.txt.xz"
"$bsdcat_bin" "$dir/payload.txt.xz" >"$dir/bsdcat.log"

printf 'libarchive tools ok\n'
