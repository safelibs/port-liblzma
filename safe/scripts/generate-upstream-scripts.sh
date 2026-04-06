#!/usr/bin/env bash
set -euo pipefail

script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
repo_root=$(cd "$script_dir/../.." && pwd)
stage_root="${1:-$repo_root/safe/tests/generated/upstream-build}"
makefile="$repo_root/build/src/scripts/Makefile"
template_dir="$repo_root/original/src/scripts"
dest_dir="$stage_root/src/scripts"

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

POSIX_SHELL=$(read_make_var POSIX_SHELL)
ENABLE_PATH_FOR_SCRIPTS=$(read_make_var enable_path_for_scripts || printf '')
XZ_CMD=$(read_make_var xz)
PACKAGE_NAME=$(read_make_var PACKAGE_NAME)
VERSION=$(read_make_var VERSION)
PACKAGE_BUGREPORT=$(read_make_var PACKAGE_BUGREPORT)

mkdir -p "$dest_dir"

generate_script() {
  local name="$1"
  local src="$template_dir/${name}.in"
  local dest="$dest_dir/$name"

  env \
    POSIX_SHELL="$POSIX_SHELL" \
    ENABLE_PATH_FOR_SCRIPTS="$ENABLE_PATH_FOR_SCRIPTS" \
    XZ_CMD="$XZ_CMD" \
    PACKAGE_NAME="$PACKAGE_NAME" \
    VERSION="$VERSION" \
    PACKAGE_BUGREPORT="$PACKAGE_BUGREPORT" \
    perl -0pe '
      s/\@POSIX_SHELL\@/$ENV{POSIX_SHELL}/g;
      s/\@enable_path_for_scripts\@/$ENV{ENABLE_PATH_FOR_SCRIPTS}/g;
      s/\@xz\@/$ENV{XZ_CMD}/g;
      s/\@PACKAGE_NAME\@/$ENV{PACKAGE_NAME}/g;
      s/\@VERSION\@/$ENV{VERSION}/g;
      s/\@PACKAGE_BUGREPORT\@/$ENV{PACKAGE_BUGREPORT}/g;
    ' "$src" > "$dest"

  chmod +x "$dest"
}

for script_name in xzdiff xzgrep xzmore xzless; do
  generate_script "$script_name"
done
