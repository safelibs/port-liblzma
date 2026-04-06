#!/usr/bin/env bash
set -euo pipefail

script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
safe_dir=$(cd "$script_dir/.." && pwd)
repo_root=$(cd "$safe_dir/.." && pwd)
dist_dir="$safe_dir/dist"
version=$(dpkg-parsechangelog -l "$safe_dir/debian/changelog" -SVersion)
arch=$(dpkg-architecture -qDEB_HOST_ARCH)

mkdir -p "$dist_dir"
rm -f \
  "$dist_dir"/liblzma5_*.deb \
  "$dist_dir"/liblzma-dev_*.deb \
  "$dist_dir"/liblzma-safe_*.buildinfo \
  "$dist_dir"/liblzma-safe_*.changes

(
  cd "$safe_dir"
  dpkg-buildpackage -b -uc -us
)

artifacts=(
  "$repo_root/liblzma5_${version}_${arch}.deb"
  "$repo_root/liblzma-dev_${version}_${arch}.deb"
  "$repo_root/liblzma-safe_${version}_${arch}.buildinfo"
  "$repo_root/liblzma-safe_${version}_${arch}.changes"
)

for artifact in "${artifacts[@]}"; do
  if [[ ! -f "$artifact" ]]; then
    printf 'missing expected build artifact: %s\n' "$artifact" >&2
    exit 1
  fi

  mv -f "$artifact" "$dist_dir/"
done

printf '%s\n' \
  "$dist_dir/liblzma5_${version}_${arch}.deb" \
  "$dist_dir/liblzma-dev_${version}_${arch}.deb" \
  "$dist_dir/liblzma-safe_${version}_${arch}.buildinfo" \
  "$dist_dir/liblzma-safe_${version}_${arch}.changes"
