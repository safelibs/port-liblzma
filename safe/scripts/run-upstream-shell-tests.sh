#!/usr/bin/env bash
set -euo pipefail

tests=(
  test_files.sh
  test_compress_prepared_bcj_sparc
  test_compress_prepared_bcj_x86
  test_compress_generated_abc
  test_compress_generated_random
  test_compress_generated_text
  test_scripts.sh
)

while [[ $# -gt 0 ]]; do
  case "$1" in
    --force)
      shift
      ;;
    --all)
      shift
      ;;
    --tests)
      shift
      tests=()
      while [[ $# -gt 0 && "$1" != --* ]]; do
        tests+=("$1")
        shift
      done
      if [[ ${#tests[@]} -eq 0 ]]; then
        printf 'missing test names after --tests\n' >&2
        exit 1
      fi
      ;;
    *)
      printf 'unknown option: %s\n' "$1" >&2
      exit 1
      ;;
  esac
done

script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
repo_root=$(cd "$script_dir/../.." && pwd)
source_tests="$repo_root/safe/tests/upstream"
stage_root="$repo_root/safe/tests/generated/upstream-build"
stage_tests="$stage_root/tests"

"$script_dir/build-upstream-tools.sh" "$stage_root" >/dev/null

rm -f \
  "$stage_tests"/compress_generated_* \
  "$stage_tests"/tmp_comp_* \
  "$stage_tests"/tmp_uncomp_* \
  "$stage_tests"/xzgrep_test_*.xz \
  "$stage_tests"/xzgrep_test_output

for test_name in "${tests[@]}"; do
  test_path="$source_tests/$test_name"
  if [[ ! -f "$test_path" ]]; then
    printf 'unknown upstream shell test: %s\n' "$test_name" >&2
    exit 1
  fi

  (
    cd "$stage_tests"
    srcdir="$source_tests" "$test_path"
  )
done
