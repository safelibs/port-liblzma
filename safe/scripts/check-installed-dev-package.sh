#!/usr/bin/env bash
set -euo pipefail

script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
safe_dir=$(cd "$script_dir/.." && pwd)
repo_root=$(cd "$safe_dir/.." && pwd)
default_package_dir="$safe_dir/dist"
package_dir="$default_package_dir"
image_tag="${LIBLZMA_SAFE_DEV_CHECK_IMAGE:-liblzma-safe-dev-check:ubuntu24.04}"
public_version=$(
  awk '
    /#define LZMA_VERSION_MAJOR/ { major=$3 }
    /#define LZMA_VERSION_MINOR/ { minor=$3 }
    /#define LZMA_VERSION_PATCH/ { patch=$3 }
    END { printf "%s.%s.%s", major, minor, patch }
  ' "$safe_dir/include/lzma/version.h"
)

usage() {
  cat <<EOF
usage: $(basename "$0") [--package-dir <dir>]

Installs the built liblzma5/liblzma-dev packages into a clean Ubuntu 24.04
container, probes the pkg-config and header interface, compiles and runs a
trivial consumer, verifies the installed headers match safe/include/, and
checks the documentation links shipped by liblzma-dev.
EOF
}

while (($#)); do
  case "$1" in
    --package-dir)
      package_dir="${2:?missing value for --package-dir}"
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

if [[ "$package_dir" != /* ]]; then
  package_dir="$repo_root/$package_dir"
fi

command -v docker >/dev/null 2>&1 || {
  printf 'missing required host tool: docker\n' >&2
  exit 1
}

if ! compgen -G "$package_dir/liblzma5_*.deb" >/dev/null || ! compgen -G "$package_dir/liblzma-dev_*.deb" >/dev/null; then
  "$script_dir/build-deb.sh" >/dev/null
fi

if [[ "$package_dir" != "$default_package_dir" ]]; then
  mkdir -p "$package_dir"
  rm -f \
    "$package_dir"/liblzma5_*.deb \
    "$package_dir"/liblzma-dev_*.deb \
    "$package_dir"/liblzma-safe_*.buildinfo \
    "$package_dir"/liblzma-safe_*.changes
  cp -f \
    "$default_package_dir"/liblzma5_*.deb \
    "$default_package_dir"/liblzma-dev_*.deb \
    "$default_package_dir"/liblzma-safe_*.buildinfo \
    "$default_package_dir"/liblzma-safe_*.changes \
    "$package_dir"/
fi

docker build -t "$image_tag" - <<'DOCKERFILE'
FROM ubuntu:24.04

ARG DEBIAN_FRONTEND=noninteractive

RUN apt-get update \
 && apt-get install -y --no-install-recommends \
      apt \
      build-essential \
      ca-certificates \
      pkg-config \
      python3 \
      xz-utils \
 && rm -rf /var/lib/apt/lists/*
DOCKERFILE

docker run --rm -i \
  -e "LIBLZMA_PUBLIC_VERSION=$public_version" \
  -v "$repo_root:/work:ro" \
  -v "$package_dir:/dist:ro" \
  "$image_tag" \
  bash -s <<'CONTAINER'
set -Eeuo pipefail

die() {
  printf 'error: %s\n' "$*" >&2
  exit 1
}

require_path() {
  local path="$1"
  [[ -e "$path" ]] || die "missing path: $path"
}

apt-get update >/tmp/liblzma-safe-check-apt.log 2>&1
apt-get install -y --no-install-recommends binutils >/tmp/liblzma-safe-check-binutils.log 2>&1
for dpkg_cfg in /etc/dpkg/dpkg.cfg.d/docker /etc/dpkg/dpkg.cfg.d/excludes; do
  if [[ -f "$dpkg_cfg" ]]; then
    mv "$dpkg_cfg" "$dpkg_cfg.disabled"
  fi
done
dpkg -i /dist/liblzma5_*.deb /dist/liblzma-dev_*.deb >/tmp/liblzma-safe-check-dpkg.log 2>&1
apt-get install -f -y --no-install-recommends >/tmp/liblzma-safe-check-fixup.log 2>&1
ldconfig

multiarch="$(gcc -print-multiarch)"
runtime_link="/usr/lib/${multiarch}/liblzma.so.5"
runtime_real="$(readlink -f "$runtime_link")"
expected_runtime="$(dpkg -L liblzma5 | awk '/\/liblzma\.so\.5\.4\.5$/ { print; exit }')"

[[ -n "$expected_runtime" ]] || die "failed to locate installed liblzma.so.5.4.5"
[[ "$runtime_real" == "$expected_runtime" ]] || die "liblzma.so.5 resolves to $runtime_real, expected $expected_runtime"

require_path "/usr/include/lzma.h"
require_path "/usr/include/lzma"
require_path "/usr/lib/${multiarch}/liblzma.so"
require_path "/usr/lib/${multiarch}/pkgconfig/liblzma.pc"
[[ "$(pkg-config --variable=includedir liblzma)" == "/usr/include" ]] || die "pkg-config includedir did not resolve to /usr/include"
[[ "$(pkg-config --variable=libdir liblzma)" == "/usr/lib/${multiarch}" ]] || die "pkg-config libdir did not resolve to /usr/lib/${multiarch}"

pkg_config_flags="$(pkg-config --cflags --libs liblzma)"
pkg_config_version="$(pkg-config --modversion liblzma)"
[[ "$pkg_config_version" == "${LIBLZMA_PUBLIC_VERSION}" ]] || {
  printf 'unexpected pkg-config version: expected %s, found %s\n' "${LIBLZMA_PUBLIC_VERSION}" "$pkg_config_version" >&2
  exit 1
}

cat >/tmp/liblzma-dev-probe.c <<'EOF'
#include <lzma.h>
#include <stdio.h>
#include <string.h>

int main(void) {
  const char *runtime = lzma_version_string();

  if (strcmp(runtime, LZMA_VERSION_STRING) != 0) {
    return 1;
  }

  printf("%s\n", runtime);
  return 0;
}
EOF

cc -o /tmp/liblzma-dev-probe /tmp/liblzma-dev-probe.c $pkg_config_flags >/tmp/liblzma-safe-check-build.log 2>&1
/tmp/liblzma-dev-probe >/tmp/liblzma-safe-check-run.log

resolved_probe="$(ldd /tmp/liblzma-dev-probe | awk '$1 == "liblzma.so.5" { print $3; exit }')"
[[ -n "$resolved_probe" ]] || die "failed to resolve liblzma.so.5 for the probe binary"
[[ "$(readlink -f "$resolved_probe")" == "$expected_runtime" ]] || {
  printf 'probe resolved liblzma.so.5 to %s, expected %s\n' "$(readlink -f "$resolved_probe")" "$expected_runtime" >&2
  ldd /tmp/liblzma-dev-probe >&2
  exit 1
}

while IFS= read -r tracked_header; do
  rel_path="${tracked_header#/work/safe/include/}"
  installed_header="/usr/include/${rel_path}"

  require_path "$installed_header"
  cmp -s "$tracked_header" "$installed_header" || {
    printf 'installed header mismatch: %s\n' "$installed_header" >&2
    exit 1
  }
done < <(find /work/safe/include -type f | sort)

while read -r target link; do
  [[ -n "${target:-}" ]] || continue
  [[ "${target:0:1}" == "#" ]] && continue

  abs_target="/$target"
  abs_link="/$link"

  [[ -L "$abs_link" ]] || die "expected documentation symlink: $abs_link"
  require_path "$abs_target"
  [[ -e "$abs_link" ]] || die "broken documentation symlink: $abs_link"
  [[ "$(readlink -e "$abs_link")" == "$(readlink -e "$abs_target")" ]] || {
    printf 'documentation symlink mismatch: %s -> %s (expected %s)\n' \
      "$abs_link" "$(readlink -e "$abs_link")" "$(readlink -e "$abs_target")" >&2
    exit 1
  }
done </work/safe/debian/liblzma-dev.links

printf 'pkg-config flags: %s\n' "$pkg_config_flags"
printf 'installed runtime: %s\n' "$expected_runtime"
CONTAINER
