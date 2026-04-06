#!/usr/bin/env bash
set -euo pipefail

script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
repo_root=$(cd "$script_dir/../.." && pwd)
safe_dir="$repo_root/safe"
lib_dir="$safe_dir/target/release"
stage_root="${1:-$safe_dir/tests/generated/upstream-build}"
stage_tests="$stage_root/tests"
stage_xz="$stage_root/src/xz/xz"
stage_xzdec="$stage_root/src/xzdec/xzdec"
stage_create="$stage_tests/create_compress_files"
safe_liblzma="$lib_dir/liblzma.so.5"
makefile="$repo_root/build/src/scripts/Makefile"

read_make_var() {
  local key="$1"
  awk -F ' = ' -v key="$key" '
    $1 == key {
      sub(/^[^=]*= /, "", $0)
      print
      found = 1
      exit
    }
    END {
      if (!found)
        exit 1
    }
  ' "$makefile"
}

resolve_liblzma() {
  local exe="$1"
  ldd "$exe" | awk '/liblzma\.so\.5/ { print $3; exit }'
}

verify_safe_link() {
  local exe="$1"
  local resolved
  resolved=$(resolve_liblzma "$exe")
  if [[ -z "$resolved" ]]; then
    printf 'failed to resolve liblzma for %s\n' "$exe" >&2
    exit 1
  fi

  if [[ "$(readlink -f "$resolved")" != "$(readlink -f "$safe_liblzma")" ]]; then
    printf 'expected %s to resolve liblzma.so.5 to %s, got %s\n' \
      "$exe" "$safe_liblzma" "$resolved" >&2
    exit 1
  fi
}

verify_script_uses_staged_xz() {
  local script="$1"
  local xz_cmd="$2"
  local xz_prog="${xz_cmd%% *}"
  local resolved

  if [[ "$xz_prog" == */* ]]; then
    resolved="$xz_prog"
  else
    resolved=$(PATH="$stage_root/src/xz:$PATH" bash -lc "command -v '$xz_prog'")
  fi

  if [[ -z "$resolved" ]]; then
    printf 'failed to resolve xz for %s\n' "$script" >&2
    exit 1
  fi

  if [[ "$(readlink -f "$resolved")" != "$(readlink -f "$stage_xz")" ]]; then
    printf 'expected %s to resolve xz via PATH to %s, got %s\n' \
      "$script" "$stage_xz" "$resolved" >&2
    exit 1
  fi

  if ! grep -Fq "xz='${xz_cmd} --format=auto'" "$script"; then
    printf 'unexpected xz command substitution in %s\n' "$script" >&2
    exit 1
  fi
}

"$script_dir/relink-release-shared.sh" >/dev/null
"$script_dir/generate-test-config.sh" >/dev/null

rm -rf "$stage_root"
mkdir -p "$stage_root/src/xz" "$stage_root/src/xzdec" "$stage_tests"
cp "$safe_dir/tests/generated/config.h" "$stage_root/config.h"

xz_objects=("$repo_root"/build/src/xz/xz-*.o)
xzdec_objects=("$repo_root"/build/src/xzdec/xzdec-*.o)

cc "${xz_objects[@]}" \
  -Wl,-rpath,"$lib_dir" \
  -Wl,-rpath-link,"$lib_dir" \
  -Wl,--push-state,--no-as-needed \
  "$safe_liblzma" \
  -Wl,--pop-state \
  -lpthread \
  -lm \
  -o "$stage_xz"

cc "${xzdec_objects[@]}" \
  -Wl,-rpath,"$lib_dir" \
  -Wl,-rpath-link,"$lib_dir" \
  -Wl,--push-state,--no-as-needed \
  "$safe_liblzma" \
  -Wl,--pop-state \
  -lpthread \
  -lm \
  -o "$stage_xzdec"

cc "$repo_root/build/tests/create_compress_files.o" \
  -Wl,-rpath,"$lib_dir" \
  -Wl,-rpath-link,"$lib_dir" \
  -Wl,--push-state,--no-as-needed \
  "$safe_liblzma" \
  -Wl,--pop-state \
  -lpthread \
  -lm \
  -o "$stage_create"

"$script_dir/generate-upstream-scripts.sh" "$stage_root"

verify_safe_link "$stage_xz"
verify_safe_link "$stage_xzdec"
verify_safe_link "$stage_create"

xz_cmd=$(read_make_var xz)
for script_path in "$stage_root"/src/scripts/xzdiff \
                   "$stage_root"/src/scripts/xzgrep \
                   "$stage_root"/src/scripts/xzmore \
                   "$stage_root"/src/scripts/xzless; do
  verify_script_uses_staged_xz "$script_path" "$xz_cmd"
done
