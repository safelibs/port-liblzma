#!/usr/bin/env bash
set -euo pipefail

dir="${1:?usage: create_dpkg_smoke_package.sh <workdir>}"

mkdir -p "$dir/control" "$dir/payload/usr/share/liblzma-smoke"
cat >"$dir/control/control" <<'EOF'
Package: liblzma-smoke
Version: 1.0
Architecture: all
Maintainer: Smoke Test <smoke@example.com>
Description: liblzma dpkg smoke test
EOF
printf 'payload unpacked through data.tar.xz\n' >"$dir/payload/usr/share/liblzma-smoke/message.txt"

tar --owner=0 --group=0 --numeric-owner -C "$dir/control" -cf "$dir/control.tar" .
xz -9 -c "$dir/control.tar" >"$dir/control.tar.xz"
tar --owner=0 --group=0 --numeric-owner -C "$dir/payload" -cf "$dir/data.tar" .
xz -9 -c "$dir/data.tar" >"$dir/data.tar.xz"
printf '2.0\n' >"$dir/debian-binary"
ar rcs "$dir/liblzma-smoke_1.0_all.deb" \
  "$dir/debian-binary" \
  "$dir/control.tar.xz" \
  "$dir/data.tar.xz"

printf '%s\n' "$dir/liblzma-smoke_1.0_all.deb"
