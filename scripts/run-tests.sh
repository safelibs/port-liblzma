#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"

usage() {
  printf 'Usage: %s <upstream|port>\n' "${0##*/}" >&2
}

run_test_dir() {
  local target="$1"
  local test_dir="$repo_root/tests/$target"
  local relative_test_dir="tests/$target"
  local nullglob_was_set=0
  local -a candidate_files=()
  local -a test_files=()
  local test_file

  if [[ ! -d "$test_dir" ]]; then
    printf 'No %s tests found in %s; nothing to run.\n' "$target" "$relative_test_dir"
    return 0
  fi

  if shopt -q nullglob; then
    nullglob_was_set=1
  fi

  shopt -s nullglob
  candidate_files=("$test_dir"/*.sh)

  if (( nullglob_was_set == 0 )); then
    shopt -u nullglob
  fi

  for test_file in "${candidate_files[@]}"; do
    if [[ -f "$test_file" ]]; then
      test_files+=("$test_file")
    fi
  done

  if (( ${#test_files[@]} == 0 )); then
    printf 'No %s tests found in %s; nothing to run.\n' "$target" "$relative_test_dir"
    return 0
  fi

  for test_file in "${test_files[@]}"; do
    printf 'Running %s\n' "${test_file#"$repo_root"/}"
    (
      cd "$repo_root"
      bash "$test_file"
    )
  done
}

if (( $# != 1 )); then
  usage
  exit 2
fi

case "$1" in
  upstream | port)
    run_test_dir "$1"
    ;;
  *)
    usage
    exit 2
    ;;
esac
