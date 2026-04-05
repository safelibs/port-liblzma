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
build_dir="$safe_dir/target/extra-c-tests"

"$script_dir/sync-upstream-headers.sh"
"$script_dir/sync-upstream-tests.sh"
"$script_dir/generate-test-config.sh"

mkdir -p "$build_dir"

cargo build --manifest-path "$safe_dir/Cargo.toml" --release >/dev/null
"$script_dir/relink-release-shared.sh" >/dev/null

if compgen -G "$repo_root/build/tests/*.o" >/dev/null; then
  "$script_dir/link-prebuilt-objects.sh" --link-only >/dev/null
fi

for src in "$safe_dir"/tests/extra/test_*.c; do
  test_name=$(basename "${src%.c}")
  exe="$build_dir/$test_name"
  cc -std=c11 -D_GNU_SOURCE -DHAVE_CONFIG_H \
    -DSAFE_TEST_FILES_DIR="\"$safe_dir/tests/upstream/files\"" \
    -I"$safe_dir/tests/generated" \
    -I"$safe_dir/tests/upstream" \
    -I"$safe_dir/include" \
    "$src" \
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
