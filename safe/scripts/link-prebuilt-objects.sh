#!/usr/bin/env bash
set -euo pipefail

mode="link-only"
if [[ "${1:-}" == "--run" ]]; then
  mode="run"
elif [[ "${1:-}" == "--link-only" || -z "${1:-}" ]]; then
  mode="link-only"
else
  printf 'unknown mode: %s\n' "$1" >&2
  exit 1
fi

script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
repo_root=$(cd "$script_dir/../.." && pwd)
safe_dir="$repo_root/safe"
lib_dir="$safe_dir/target/release"
safe_lib="$lib_dir/liblzma.so"
scratch="$safe_dir/target/prebuilt-link"
bin_dir="$scratch/bin"

mkdir -p "$bin_dir"

"$script_dir/relink-release-shared.sh" >/dev/null

for obj in "$repo_root"/build/tests/*.o; do
  exe="$bin_dir/$(basename "${obj%.o}")"
  cc "$obj" \
    -L"$lib_dir" \
    -Wl,-rpath,"$lib_dir" \
    -Wl,-rpath-link,"$lib_dir" \
    -llzma \
    -lpthread \
    -o "$exe"
  if [[ "$mode" == "run" ]]; then
    "$exe"
  fi
done
