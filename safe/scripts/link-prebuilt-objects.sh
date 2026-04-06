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
safe_liblzma="$lib_dir/liblzma.so.5"
scratch="$safe_dir/target/prebuilt-link"
bin_dir="$scratch/bin"
stage_root="$safe_dir/tests/generated/upstream-build"
stage_tests="$stage_root/tests"
source_tests="$safe_dir/tests/upstream"

mkdir -p "$bin_dir"

"$script_dir/relink-release-shared.sh" >/dev/null
if [[ "$mode" == "run" ]]; then
  "$script_dir/build-upstream-tools.sh" "$stage_root" >/dev/null
fi

run_relinked_binary() {
  local exe="$1"
  local name="$2"

  case "$name" in
    create_compress_files)
      rm -f "$stage_tests"/compress_generated_text
      (
        cd "$stage_tests"
        "$exe" compress_generated_text
      )
      ;;
    test_*)
      (
        cd "$stage_tests"
        srcdir="$source_tests" "$exe"
      )
      ;;
    *)
      (
        cd "$stage_tests"
        "$exe"
      )
      ;;
  esac
}

for obj in "$repo_root"/build/tests/*.o; do
  name=$(basename "${obj%.o}")
  exe="$bin_dir/$name"
  cc "$obj" \
    -Wl,-rpath,"$lib_dir" \
    -Wl,-rpath-link,"$lib_dir" \
    -Wl,--push-state,--no-as-needed \
    "$safe_liblzma" \
    -Wl,--pop-state \
    -lpthread \
    -o "$exe"
  if [[ "$mode" == "run" ]]; then
    run_relinked_binary "$exe" "$name"
  fi
done
