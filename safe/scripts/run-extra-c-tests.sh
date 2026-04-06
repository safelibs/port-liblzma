#!/usr/bin/env bash
set -euo pipefail

mode="link-only"
mode_explicit=0
tests=()

while [[ $# -gt 0 ]]; do
  case "$1" in
    --run)
      mode="run"
      mode_explicit=1
      shift
      ;;
    --link-only)
      mode="link-only"
      mode_explicit=1
      shift
      ;;
    --tests)
      shift
      if [[ $# -eq 0 ]]; then
        printf 'missing test names after --tests\n' >&2
        exit 1
      fi
      while [[ $# -gt 0 && "$1" != --* ]]; do
        tests+=("$1")
        shift
      done
      if [[ ${#tests[@]} -eq 0 ]]; then
        printf 'missing test names after --tests\n' >&2
        exit 1
      fi
      if [[ $mode_explicit -eq 0 ]]; then
        mode="run"
      fi
      ;;
    --all)
      if [[ $mode_explicit -eq 0 ]]; then
        mode="run"
      fi
      shift
      ;;
    *)
      printf 'unknown mode: %s\n' "$1" >&2
      exit 1
      ;;
  esac
done

script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
repo_root=$(cd "$script_dir/../.." && pwd)
safe_dir="$repo_root/safe"
lib_dir="$safe_dir/target/release"
build_dir="$safe_dir/target/extra-c-tests"

"$script_dir/sync-upstream-headers.sh"
"$script_dir/sync-upstream-tests.sh"
"$script_dir/generate-test-config.sh"

mkdir -p "$build_dir"

cargo build --manifest-path "$safe_dir/Cargo.toml" --offline --locked --release >/dev/null
"$script_dir/relink-release-shared.sh" >/dev/null

if compgen -G "$repo_root/build/tests/*.o" >/dev/null; then
  "$script_dir/link-prebuilt-objects.sh" --link-only >/dev/null
fi

if [[ ${#tests[@]} -eq 0 ]]; then
  test_sources=("$safe_dir"/tests/extra/test_*.c)
else
  test_sources=()
  for test_name in "${tests[@]}"; do
    src="$safe_dir/tests/extra/${test_name}.c"
    if [[ ! -f "$src" ]]; then
      printf 'unknown test: %s\n' "$test_name" >&2
      exit 1
    fi
    test_sources+=("$src")
  done
fi

for src in "${test_sources[@]}"; do
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
    srcdir="$safe_dir/tests/upstream" "$exe"
  fi
done
