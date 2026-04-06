#!/usr/bin/env bash
set -Eeuo pipefail

export LANG=C.UTF-8
export LC_ALL=C.UTF-8
export DEBIAN_FRONTEND=noninteractive

READ_ONLY_ROOT=/work
IMPLEMENTATION="${LIBLZMA_IMPLEMENTATION:-original}"
SOURCE_ROOT=/tmp/liblzma-original
BUILD_ROOT=/tmp/liblzma-build
TEST_ROOT=/tmp/liblzma-dependent-tests
ONLY="${LIBLZMA_TEST_ONLY:-}"
CURRENT_STEP=""
MULTIARCH="$(gcc -print-multiarch)"
ACTIVE_LIBLZMA=""
APT_LIB="/usr/lib/${MULTIARCH}/libapt-pkg.so.6.0"
LIBXML2_SO="/usr/lib/${MULTIARCH}/libxml2.so.2"
LIBTIFF_SO="/usr/lib/${MULTIARCH}/libtiff.so.6"
LIBARCHIVE_SO="/usr/lib/${MULTIARCH}/libarchive.so.13"
BOOST_IOSTREAMS_SO="/usr/lib/${MULTIARCH}/libboost_iostreams.so.1.83.0"
DPKG_DEB_BIN="/usr/bin/dpkg-deb"
APT_GET_BIN="/usr/bin/apt-get"
APT_CACHE_BIN="/usr/bin/apt-cache"
PYTHON_BIN="/usr/bin/python3.12"
XMLLINT_BIN="/usr/bin/xmllint"
MKSQUASHFS_BIN="/usr/bin/mksquashfs"
UNSQUASHFS_BIN="/usr/bin/unsquashfs"
GDB_BIN="/usr/bin/gdb"
BSDTAR_BIN="/usr/bin/bsdtar"
BSDCAT_BIN="/usr/bin/bsdcat"
MODINFO_BIN="$(command -v modinfo)"
MARIADB_BIN="$(command -v mariadb)"
MARIADBD_BIN="$(command -v mariadbd)"
MARIADB_INSTALL_DB_BIN="$(command -v mariadb-install-db)"
MARIADB_PLUGIN_SO="/usr/lib/mysql/plugin/provider_lzma.so"

trap 'rc=$?; if [[ "$rc" -ne 0 && -n "$CURRENT_STEP" ]]; then printf "failed during: %s\n" "$CURRENT_STEP" >&2; fi; exit "$rc"' EXIT

log_step() {
  printf '\n==> %s\n' "$1"
}

die() {
  printf 'error: %s\n' "$*" >&2
  exit 1
}

require_nonempty_file() {
  local path="$1"

  [[ -s "$path" ]] || die "expected non-empty file: $path"
}

require_contains() {
  local path="$1"
  local needle="$2"

  if ! grep -F -- "$needle" "$path" >/dev/null 2>&1; then
    printf 'missing expected text in %s: %s\n' "$path" "$needle" >&2
    printf -- '--- %s ---\n' "$path" >&2
    cat "$path" >&2
    exit 1
  fi
}

should_run() {
  local package="$1"

  [[ -z "$ONLY" || "$ONLY" == "$package" ]]
}

reset_test_dir() {
  local name="$1"
  local dir="$TEST_ROOT/$name"

  rm -rf "$dir"
  mkdir -p "$dir"
  printf '%s\n' "$dir"
}

assert_exists() {
  local path="$1"

  [[ -e "$path" ]] || die "missing path: $path"
}

assert_links_to_active_liblzma() {
  local target="$1"
  local resolved=""

  assert_exists "$target"

  resolved="$(ldd "$target" | awk '$1 == "liblzma.so.5" { print $3; exit }')"
  [[ -n "$resolved" ]] || die "ldd did not report liblzma.so.5 for $target"
  resolved="$(readlink -f "$resolved")"
  [[ "$resolved" == "$ACTIVE_LIBLZMA" ]] || {
    printf 'expected %s to resolve liblzma.so.5 from %s, got %s\n' "$target" "$ACTIVE_LIBLZMA" "$resolved" >&2
    ldd "$target" >&2
    exit 1
  }
}

build_original_liblzma() {
  CURRENT_STEP="build original liblzma"
  log_step "Building and installing original liblzma"

  rm -rf "$SOURCE_ROOT" "$BUILD_ROOT" "$TEST_ROOT"
  mkdir -p "$BUILD_ROOT" "$TEST_ROOT"
  cp -a "$READ_ONLY_ROOT/original" "$SOURCE_ROOT"

  cd "$BUILD_ROOT"
  "$SOURCE_ROOT/configure" \
    --prefix=/usr/local \
    --disable-static \
    --disable-xz \
    --disable-xzdec \
    --disable-lzmadec \
    --disable-lzmainfo \
    --disable-scripts \
    --disable-doc \
    --disable-nls \
    --disable-dependency-tracking \
    >/tmp/liblzma-configure.log 2>&1
  make -j"$(nproc)" >/tmp/liblzma-make.log 2>&1
  make install >/tmp/liblzma-install.log 2>&1
  printf '/usr/local/lib\n' >/etc/ld.so.conf.d/000-local-liblzma.conf
  ldconfig

  export LD_LIBRARY_PATH="/usr/local/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
  export PKG_CONFIG_PATH="/usr/local/lib/pkgconfig${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"

  ACTIVE_LIBLZMA="$(readlink -f /usr/local/lib/liblzma.so.5)"
  [[ -n "$ACTIVE_LIBLZMA" && -f "$ACTIVE_LIBLZMA" ]] || die "failed to install local liblzma shared library"
  cd /
}

select_safe_liblzma() {
  CURRENT_STEP="select preinstalled safe liblzma packages"
  log_step "Using preinstalled safe liblzma packages"

  rm -rf "$TEST_ROOT"
  mkdir -p "$TEST_ROOT"

  unset LD_LIBRARY_PATH
  unset PKG_CONFIG_PATH
  ldconfig

  ACTIVE_LIBLZMA="$(readlink -f "/usr/lib/${MULTIARCH}/liblzma.so.5")"
  [[ -n "$ACTIVE_LIBLZMA" && -f "$ACTIVE_LIBLZMA" ]] || die "failed to locate packaged liblzma shared library"
  cd /
}

test_dpkg() {
  local dir

  CURRENT_STEP="dpkg"
  log_step "dpkg"
  assert_links_to_active_liblzma "$DPKG_DEB_BIN"
  dir="$(reset_test_dir dpkg)"

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

  "$DPKG_DEB_BIN" --info "$dir/liblzma-smoke_1.0_all.deb" >"$dir/info.log"
  require_contains "$dir/info.log" "Package: liblzma-smoke"
  "$DPKG_DEB_BIN" -x "$dir/liblzma-smoke_1.0_all.deb" "$dir/extract"
  require_contains "$dir/extract/usr/share/liblzma-smoke/message.txt" "payload unpacked through data.tar.xz"
}

test_apt() {
  local dir

  CURRENT_STEP="apt"
  log_step "apt"
  assert_links_to_active_liblzma "$APT_LIB"
  dir="$(reset_test_dir apt)"

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

  (
    set -euo pipefail
    cd "$dir/repo"
    python3 -m http.server 18080 >"$dir/http.log" 2>&1 &
    http_pid="$!"
    trap 'kill "$http_pid" >/dev/null 2>&1 || true; wait "$http_pid" >/dev/null 2>&1 || true' EXIT
    sleep 1

    timeout 60 "$APT_GET_BIN" \
      -o Debug::Acquire::http=true \
      -o Dir::State="$dir/root/state" \
      -o Dir::Cache="$dir/root/cache" \
      -o Dir::Etc::sourcelist="$dir/root/etc/apt/sources.list" \
      -o Dir::Etc::sourceparts="$dir/root/etc/apt/sources.list.d" \
      -o APT::Architecture=amd64 \
      update >"$dir/apt-update.log" 2>&1

    timeout 60 "$APT_CACHE_BIN" \
      -o Dir::State="$dir/root/state" \
      -o Dir::Cache="$dir/root/cache" \
      -o Dir::Etc::sourcelist="$dir/root/etc/apt/sources.list" \
      -o Dir::Etc::sourceparts="$dir/root/etc/apt/sources.list.d" \
      -o APT::Architecture=amd64 \
      show liblzma-apt-smoke >"$dir/apt-show.log" 2>&1
  )

  require_contains "$dir/apt-update.log" "Packages.xz"
  require_contains "$dir/apt-show.log" "Package: liblzma-apt-smoke"
}

test_python312() {
  local dir
  local module_path

  CURRENT_STEP="python3.12"
  log_step "python3.12"
  dir="$(reset_test_dir python312)"
  module_path="$("$PYTHON_BIN" - <<'PY'
import _lzma
print(_lzma.__file__)
PY
)"
  assert_links_to_active_liblzma "$module_path"

  "$PYTHON_BIN" - <<'PY' >"$dir/python.log"
import lzma
from pathlib import Path

work = Path("/tmp/liblzma-dependent-tests/python312")
payload = (b"python lzma smoke\n" * 64) + bytes(range(64))
compressed = lzma.compress(payload, format=lzma.FORMAT_XZ)
assert lzma.decompress(compressed) == payload

path = work / "payload.xz"
with lzma.open(path, "wb", preset=6) as handle:
    handle.write(payload)

with lzma.open(path, "rb") as handle:
    restored = handle.read()

assert restored == payload
print("python lzma ok")
PY

  require_contains "$dir/python.log" "python lzma ok"
}

test_libxml2() {
  local dir

  CURRENT_STEP="libxml2"
  log_step "libxml2"
  assert_links_to_active_liblzma "$LIBXML2_SO"
  dir="$(reset_test_dir libxml2)"

  cat >"$dir/document.xml" <<'EOF'
<root>
  <item>libxml2 through xz</item>
</root>
EOF
  xz -9 -c "$dir/document.xml" >"$dir/document.xml.xz"

  "$XMLLINT_BIN" --xpath 'string(/root/item)' "$dir/document.xml.xz" >"$dir/xmllint.out"
  require_contains "$dir/xmllint.out" "libxml2 through xz"
}

test_libtiff6() {
  local dir

  CURRENT_STEP="libtiff6"
  log_step "libtiff6"
  assert_links_to_active_liblzma "$LIBTIFF_SO"
  dir="$(reset_test_dir libtiff6)"

  cat >"$dir/libtiff_smoke.c" <<'EOF'
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <tiffio.h>

int main(int argc, char **argv) {
  const char *path = argv[1];
  const uint32_t width = 4;
  const uint32_t height = 4;
  uint16_t compression = 0;
  uint8_t rows[4][4] = {
    {0, 1, 2, 3},
    {4, 5, 6, 7},
    {8, 9, 10, 11},
    {12, 13, 14, 15},
  };

  TIFF *out = TIFFOpen(path, "w");
  if (out == NULL) {
    return 1;
  }

  TIFFSetField(out, TIFFTAG_IMAGEWIDTH, width);
  TIFFSetField(out, TIFFTAG_IMAGELENGTH, height);
  TIFFSetField(out, TIFFTAG_SAMPLESPERPIXEL, 1);
  TIFFSetField(out, TIFFTAG_BITSPERSAMPLE, 8);
  TIFFSetField(out, TIFFTAG_ORIENTATION, ORIENTATION_TOPLEFT);
  TIFFSetField(out, TIFFTAG_PLANARCONFIG, PLANARCONFIG_CONTIG);
  TIFFSetField(out, TIFFTAG_PHOTOMETRIC, PHOTOMETRIC_MINISBLACK);
  TIFFSetField(out, TIFFTAG_COMPRESSION, COMPRESSION_LZMA);
  TIFFSetField(out, TIFFTAG_ROWSPERSTRIP, height);

  for (uint32_t row = 0; row < height; ++row) {
    if (TIFFWriteScanline(out, rows[row], row, 0) != 1) {
      TIFFClose(out);
      return 2;
    }
  }

  TIFFClose(out);

  TIFF *in = TIFFOpen(path, "r");
  if (in == NULL) {
    return 3;
  }

  TIFFGetField(in, TIFFTAG_COMPRESSION, &compression);
  if (compression != COMPRESSION_LZMA) {
    TIFFClose(in);
    return 4;
  }

  for (uint32_t row = 0; row < height; ++row) {
    uint8_t scanline[4];
    if (TIFFReadScanline(in, scanline, row, 0) != 1) {
      TIFFClose(in);
      return 5;
    }
    if (memcmp(scanline, rows[row], sizeof(scanline)) != 0) {
      TIFFClose(in);
      return 6;
    }
  }

  TIFFClose(in);
  puts("libtiff lzma ok");
  return 0;
}
EOF

  cc \
    -o "$dir/libtiff-smoke" \
    "$dir/libtiff_smoke.c" \
    $(pkg-config --cflags --libs libtiff-4) \
    >/tmp/libtiff-build.log 2>&1
  "$dir/libtiff-smoke" "$dir/lzma.tiff" >"$dir/libtiff.log"
  require_contains "$dir/libtiff.log" "libtiff lzma ok"
}

test_squashfs_tools() {
  local dir

  CURRENT_STEP="squashfs-tools"
  log_step "squashfs-tools"
  assert_links_to_active_liblzma "$MKSQUASHFS_BIN"
  assert_links_to_active_liblzma "$UNSQUASHFS_BIN"
  dir="$(reset_test_dir squashfs)"

  mkdir -p "$dir/input/docs"
  printf 'squashfs xz payload\n' >"$dir/input/docs/message.txt"

  "$MKSQUASHFS_BIN" "$dir/input" "$dir/image.sqfs" -comp xz -noappend -all-root -quiet >"$dir/mksquashfs.log" 2>&1
  "$UNSQUASHFS_BIN" -dest "$dir/output" "$dir/image.sqfs" >"$dir/unsquashfs.log" 2>&1
  require_contains "$dir/output/docs/message.txt" "squashfs xz payload"
}

test_kmod() {
  local dir

  CURRENT_STEP="kmod"
  log_step "kmod"
  assert_links_to_active_liblzma "$MODINFO_BIN"
  dir="$(reset_test_dir kmod)"

  cat >"$dir/module.c" <<'EOF'
void liblzma_smoke(void) {}
EOF
  gcc -c -o "$dir/module.o" "$dir/module.c" >/tmp/kmod-build.log 2>&1
  printf 'description=liblzma kmod smoke\0license=GPL\0name=liblzma_smoke\0' >"$dir/modinfo.bin"
  objcopy \
    --add-section .modinfo="$dir/modinfo.bin" \
    --set-section-flags .modinfo=alloc,readonly \
    "$dir/module.o" \
    "$dir/liblzma_smoke.ko"
  xz -9 -c "$dir/liblzma_smoke.ko" >"$dir/liblzma_smoke.ko.xz"

  "$MODINFO_BIN" "$dir/liblzma_smoke.ko.xz" >"$dir/modinfo.log"
  require_contains "$dir/modinfo.log" "liblzma kmod smoke"
  require_contains "$dir/modinfo.log" "GPL"
}

test_gdb() {
  local dir

  CURRENT_STEP="gdb"
  log_step "gdb"
  assert_links_to_active_liblzma "$GDB_BIN"
  dir="$(reset_test_dir gdb)"

  cat >"$dir/gdb_smoke.c" <<'EOF'
#include <stdio.h>

__attribute__((noinline))
static int helper(int input) {
  int local = input + 7;
  puts("helper");
  return local * 3;
}

int main(void) {
  return helper(5) == 36 ? 0 : 1;
}
EOF

  gcc -g -O0 -fno-inline -o "$dir/gdb-smoke" "$dir/gdb_smoke.c" >/tmp/gdb-build.log 2>&1
  objcopy --only-keep-debug "$dir/gdb-smoke" "$dir/gdb-smoke.debug"
  strip --strip-debug "$dir/gdb-smoke"
  xz -9 -c "$dir/gdb-smoke.debug" >"$dir/gdb-smoke.debug.xz"
  objcopy \
    --add-section .gnu_debugdata="$dir/gdb-smoke.debug.xz" \
    --set-section-flags .gnu_debugdata=readonly \
    "$dir/gdb-smoke"
  rm -f "$dir/gdb-smoke.debug" "$dir/gdb-smoke.debug.xz"

  "$GDB_BIN" -q -nx -batch \
    -ex 'set debuginfod enabled off' \
    -ex 'break gdb_smoke.c:6' \
    -ex 'run' \
    -ex 'info locals' \
    "$dir/gdb-smoke" >"$dir/gdb.log" 2>&1

  require_contains "$dir/gdb.log" "local = 12"
}

test_libarchive13t64() {
  local dir

  CURRENT_STEP="libarchive13t64"
  log_step "libarchive13t64"
  assert_links_to_active_liblzma "$LIBARCHIVE_SO"
  dir="$(reset_test_dir libarchive)"

  mkdir -p "$dir/input"
  printf 'libarchive xz smoke\n' >"$dir/input/message.txt"

  "$BSDTAR_BIN" -acf "$dir/archive.tar.xz" -C "$dir/input" . >"$dir/create.log" 2>&1
  mkdir -p "$dir/output"
  "$BSDTAR_BIN" -xf "$dir/archive.tar.xz" -C "$dir/output" >"$dir/extract.log" 2>&1
  require_contains "$dir/output/message.txt" "libarchive xz smoke"
}

test_libarchive_tools() {
  local dir

  CURRENT_STEP="libarchive-tools"
  log_step "libarchive-tools"
  assert_links_to_active_liblzma "$BSDTAR_BIN"
  assert_links_to_active_liblzma "$BSDCAT_BIN"
  dir="$(reset_test_dir libarchive-tools)"

  mkdir -p "$dir/input/archive"
  printf 'libarchive tools tar.xz smoke\n' >"$dir/input/archive/message.txt"

  "$BSDTAR_BIN" -acf "$dir/archive.tar.xz" -C "$dir/input" . >"$dir/create.log" 2>&1
  "$BSDTAR_BIN" -tf "$dir/archive.tar.xz" >"$dir/list.log"
  require_contains "$dir/list.log" "message.txt"

  mkdir -p "$dir/output"
  "$BSDTAR_BIN" -xf "$dir/archive.tar.xz" -C "$dir/output" >"$dir/extract.log" 2>&1
  require_contains "$dir/output/archive/message.txt" "libarchive tools tar.xz smoke"

  printf 'libarchive tools bsdcat smoke\n' >"$dir/payload.txt"
  xz -9 -c "$dir/payload.txt" >"$dir/payload.txt.xz"
  "$BSDCAT_BIN" "$dir/payload.txt.xz" >"$dir/bsdcat.log"
  require_contains "$dir/bsdcat.log" "libarchive tools bsdcat smoke"
}

mariadb_query() {
  local socket="$1"
  local sql="$2"

  "$MARIADB_BIN" --protocol=socket --socket="$socket" -uroot -N -B -e "$sql"
}

wait_for_mariadb() {
  local socket="$1"
  local retries=60

  while ((retries > 0)); do
    if mariadb_query "$socket" "SELECT 1;" >/dev/null 2>&1; then
      return 0
    fi
    sleep 1
    retries=$((retries - 1))
  done

  return 1
}

test_mariadb_plugin_provider_lzma() {
  local dir
  local socket
  local plugin_status
  local have_lzma
  local mariadb_pid

  CURRENT_STEP="mariadb-plugin-provider-lzma"
  log_step "mariadb-plugin-provider-lzma"
  assert_links_to_active_liblzma "$MARIADB_PLUGIN_SO"
  dir="$(reset_test_dir mariadb)"
  socket="$dir/mariadb.sock"

  "$MARIADB_INSTALL_DB_BIN" \
    --no-defaults \
    --auth-root-authentication-method=normal \
    --user=root \
    --skip-test-db \
    --datadir="$dir/data" \
    >"$dir/install-db.log" 2>&1

  "$MARIADBD_BIN" \
    --no-defaults \
    --user=root \
    --datadir="$dir/data" \
    --socket="$socket" \
    --pid-file="$dir/mariadb.pid" \
    --skip-networking \
    --plugin-dir="$(dirname "$MARIADB_PLUGIN_SO")" \
    --log-error="$dir/mariadb.log" \
    >"$dir/mariadbd.stdout" 2>&1 &
  mariadb_pid="$!"
  trap 'kill "$mariadb_pid" >/dev/null 2>&1 || true; wait "$mariadb_pid" >/dev/null 2>&1 || true' RETURN

  wait_for_mariadb "$socket" || {
    cat "$dir/mariadb.log" >&2
    exit 1
  }

  plugin_status="$(mariadb_query "$socket" "SELECT PLUGIN_STATUS FROM INFORMATION_SCHEMA.PLUGINS WHERE PLUGIN_NAME = 'provider_lzma';" || true)"
  if [[ "$plugin_status" != "ACTIVE" ]]; then
    mariadb_query "$socket" "INSTALL SONAME 'provider_lzma';" >"$dir/install-plugin.log"
  fi

  plugin_status="$(mariadb_query "$socket" "SELECT PLUGIN_STATUS FROM INFORMATION_SCHEMA.PLUGINS WHERE PLUGIN_NAME = 'provider_lzma';")"
  [[ "$plugin_status" == "ACTIVE" ]] || die "provider_lzma plugin failed to activate"

  have_lzma="$(mariadb_query "$socket" "SHOW GLOBAL STATUS LIKE 'Innodb_have_lzma';" | awk '{print $2}')"
  [[ "$have_lzma" == "YES" || "$have_lzma" == "ON" ]] || die "expected Innodb_have_lzma to report support, got: ${have_lzma:-<empty>}"

  mariadb_query "$socket" "SET GLOBAL innodb_compression_algorithm = 'lzma';" >/dev/null
  mariadb_query "$socket" "SELECT @@GLOBAL.innodb_compression_algorithm;" >"$dir/algorithm.log"
  require_contains "$dir/algorithm.log" "lzma"

  kill "$mariadb_pid" >/dev/null 2>&1 || true
  wait "$mariadb_pid" >/dev/null 2>&1 || true
  trap - RETURN
}

test_libboost_iostreams1830() {
  local dir

  CURRENT_STEP="libboost-iostreams1.83.0"
  log_step "libboost-iostreams1.83.0"
  assert_links_to_active_liblzma "$BOOST_IOSTREAMS_SO"
  dir="$(reset_test_dir boost)"

  cat >"$dir/boost_smoke.cpp" <<'EOF'
#include <boost/iostreams/close.hpp>
#include <boost/iostreams/filter/lzma.hpp>
#include <boost/iostreams/filtering_stream.hpp>
#include <sstream>
#include <string>
#include <iostream>

namespace bio = boost::iostreams;

int main() {
  const std::string payload = std::string(4096, 'x') + " libboost-iostreams lzma";
  std::stringstream compressed;
  std::stringstream restored;

  {
    bio::filtering_ostream out;
    out.push(bio::lzma_compressor());
    out.push(compressed);
    out << payload;
    bio::close(out);
  }

  {
    std::stringstream input(compressed.str());
    bio::filtering_istream in;
    in.push(bio::lzma_decompressor());
    in.push(input);
    restored << in.rdbuf();
  }

  if (restored.str() != payload) {
    return 1;
  }

  std::cout << "boost lzma ok\n";
  return 0;
}
EOF

  g++ -std=c++17 -O2 -o "$dir/boost-smoke" "$dir/boost_smoke.cpp" -lboost_iostreams >/tmp/boost-build.log 2>&1
  "$dir/boost-smoke" >"$dir/boost.log"
  require_contains "$dir/boost.log" "boost lzma ok"
}

case "$IMPLEMENTATION" in
  original)
    build_original_liblzma
    ;;
  safe)
    select_safe_liblzma
    ;;
  *)
    die "unsupported implementation inside container: $IMPLEMENTATION"
    ;;
esac

should_run "dpkg" && test_dpkg
should_run "apt" && test_apt
should_run "python3.12" && test_python312
should_run "libxml2" && test_libxml2
should_run "libtiff6" && test_libtiff6
should_run "squashfs-tools" && test_squashfs_tools
should_run "kmod" && test_kmod
should_run "gdb" && test_gdb
should_run "libarchive13t64" && test_libarchive13t64
should_run "libarchive-tools" && test_libarchive_tools
should_run "mariadb-plugin-provider-lzma" && test_mariadb_plugin_provider_lzma
should_run "libboost-iostreams1.83.0" && test_libboost_iostreams1830

CURRENT_STEP=""
log_step "All requested liblzma dependent smoke tests passed"
