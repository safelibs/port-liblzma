#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
safe_dir="$(cd -- "$script_dir/.." && pwd)"
repo_root="$(cd -- "$safe_dir/.." && pwd)"
tracked_dockerfile_relative="safe/docker/dependent-test.Dockerfile"
dockerfile="$repo_root/$tracked_dockerfile_relative"
default_safe_package_dir="$safe_dir/dist"
implementation="original"
safe_package_dir="$default_safe_package_dir"
original_image_tag="liblzma-dependent-test:ubuntu24.04-original"
safe_image_tag="liblzma-dependent-test:ubuntu24.04-safe"
image_tag=""

usage() {
  cat <<'EOF'
usage: build-dependent-test-image.sh [--implementation <original|safe>] [--safe-package-dir <dir>] [--image-tag <tag>]

Builds the Ubuntu 24.04 dependent smoke-test image for the selected liblzma
implementation. Safe mode installs the tracked liblzma5/liblzma-dev .deb files
during docker build so the image is ready to run without runtime package mounts.
EOF
}

while (($#)); do
  case "$1" in
    --implementation)
      implementation="${2:?missing value for --implementation}"
      shift 2
      ;;
    --safe-package-dir)
      safe_package_dir="${2:?missing value for --safe-package-dir}"
      shift 2
      ;;
    --image-tag)
      image_tag="${2:?missing value for --image-tag}"
      shift 2
      ;;
    --help|-h)
      usage
      exit 0
      ;;
    *)
      printf 'unknown option: %s\n' "$1" >&2
      usage >&2
      exit 1
      ;;
  esac
done

case "$implementation" in
  original|safe)
    ;;
  *)
    printf 'unknown implementation: %s\n' "$implementation" >&2
    usage >&2
    exit 1
    ;;
esac

if [[ -z "$image_tag" ]]; then
  case "$implementation" in
    original)
      image_tag="$original_image_tag"
      ;;
    safe)
      image_tag="$safe_image_tag"
      ;;
  esac
fi

if [[ "$safe_package_dir" != /* ]]; then
  safe_package_dir="$repo_root/$safe_package_dir"
fi

command -v docker >/dev/null 2>&1 || {
  printf 'missing required host tool: docker\n' >&2
  exit 1
}

[[ -f "$dockerfile" ]] || {
  printf 'missing tracked Dockerfile: %s\n' "$tracked_dockerfile_relative" >&2
  exit 1
}

have_safe_artifacts() {
  local dir="$1"

  compgen -G "$dir/liblzma5_*.deb" >/dev/null \
    && compgen -G "$dir/liblzma-dev_*.deb" >/dev/null \
    && compgen -G "$dir/liblzma-safe_*.buildinfo" >/dev/null \
    && compgen -G "$dir/liblzma-safe_*.changes" >/dev/null
}

resolve_single_artifact() {
  local dir="$1"
  local pattern="$2"
  local description="$3"
  local -a matches=()

  shopt -s nullglob
  matches=("$dir"/$pattern)
  shopt -u nullglob

  if [[ "${#matches[@]}" -ne 1 ]]; then
    printf 'expected exactly one %s in %s matching %s, found %s\n' \
      "$description" "$dir" "$pattern" "${#matches[@]}" >&2
    exit 1
  fi

  printf '%s\n' "${matches[0]}"
}

refresh_safe_artifacts() {
  local target_dir="$1"

  if have_safe_artifacts "$target_dir"; then
    return 0
  fi

  if [[ "$target_dir" == "$default_safe_package_dir" ]]; then
    "$script_dir/build-deb.sh" >/dev/null
    have_safe_artifacts "$target_dir" || {
      printf 'safe package build did not produce the expected artifacts in %s\n' "$target_dir" >&2
      exit 1
    }
    return 0
  fi

  if ! have_safe_artifacts "$default_safe_package_dir"; then
    "$script_dir/build-deb.sh" >/dev/null
  fi

  mkdir -p "$target_dir"
  rm -f \
    "$target_dir"/liblzma5_*.deb \
    "$target_dir"/liblzma-dev_*.deb \
    "$target_dir"/liblzma-safe_*.buildinfo \
    "$target_dir"/liblzma-safe_*.changes
  cp -f \
    "$default_safe_package_dir"/liblzma5_*.deb \
    "$default_safe_package_dir"/liblzma-dev_*.deb \
    "$default_safe_package_dir"/liblzma-safe_*.buildinfo \
    "$default_safe_package_dir"/liblzma-safe_*.changes \
    "$target_dir"/
}

build_context="$(mktemp -d)"
trap 'rm -rf "$build_context"' EXIT

cp "$dockerfile" "$build_context/Dockerfile"
mkdir -p "$build_context/packages"

if [[ "$implementation" == "safe" ]]; then
  runtime_pkg=""
  dev_pkg=""
  buildinfo=""
  changes=""

  refresh_safe_artifacts "$safe_package_dir"
  runtime_pkg="$(resolve_single_artifact "$safe_package_dir" 'liblzma5_*.deb' 'liblzma5 package')"
  dev_pkg="$(resolve_single_artifact "$safe_package_dir" 'liblzma-dev_*.deb' 'liblzma-dev package')"
  buildinfo="$(resolve_single_artifact "$safe_package_dir" 'liblzma-safe_*.buildinfo' 'buildinfo artifact')"
  changes="$(resolve_single_artifact "$safe_package_dir" 'liblzma-safe_*.changes' 'changes artifact')"

  cp -f "$runtime_pkg" "$build_context/packages/liblzma5.deb"
  cp -f "$dev_pkg" "$build_context/packages/liblzma-dev.deb"
  cp -f "$buildinfo" "$build_context/packages/liblzma-safe.buildinfo"
  cp -f "$changes" "$build_context/packages/liblzma-safe.changes"
fi

docker build \
  --build-arg "LIBLZMA_IMPLEMENTATION=$implementation" \
  -t "$image_tag" \
  "$build_context"

printf '%s\n' "$image_tag"
