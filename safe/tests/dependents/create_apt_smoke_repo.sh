#!/usr/bin/env bash
set -euo pipefail

dir="${1:?usage: create_apt_smoke_repo.sh <workdir>}"

mkdir -p \
  "$dir/pkg/DEBIAN" \
  "$dir/pkg/usr/share/liblzma-apt-smoke" \
  "$dir/repo/pool/main/l/liblzma-smoke" \
  "$dir/repo/dists/stable/main/binary-amd64" \
  "$dir/root/state/lists/partial" \
  "$dir/root/cache/archives/partial" \
  "$dir/root/etc/apt/sources.list.d"
: >"$dir/root/state/status"

cat >"$dir/pkg/DEBIAN/control" <<'EOF'
Package: liblzma-apt-smoke
Version: 1.0
Architecture: all
Maintainer: Smoke Test <smoke@example.com>
Description: liblzma apt smoke test
EOF
printf 'apt metadata via Packages.xz\n' >"$dir/pkg/usr/share/liblzma-apt-smoke/message.txt"
dpkg-deb --build -Zxz "$dir/pkg" "$dir/repo/pool/main/l/liblzma-smoke/liblzma-apt-smoke_1.0_all.deb" >/tmp/apt-build-pkg.log 2>&1

dpkg-scanpackages "$dir/repo/pool" /dev/null >"$dir/repo/dists/stable/main/binary-amd64/Packages" 2>"$dir/scanpackages.log"
xz -9 -c "$dir/repo/dists/stable/main/binary-amd64/Packages" >"$dir/repo/dists/stable/main/binary-amd64/Packages.xz"
apt-ftparchive release "$dir/repo/dists/stable" >"$dir/repo/dists/stable/Release"

cat >"$dir/root/etc/apt/sources.list" <<'EOF'
deb [trusted=yes] http://127.0.0.1:18080 stable main
EOF
