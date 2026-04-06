#!/usr/bin/env bash
set -Eeuo pipefail

export LANG=C.UTF-8
export LC_ALL=C.UTF-8
export DEBIAN_FRONTEND=noninteractive

READ_ONLY_ROOT="${LIBLZMA_READ_ONLY_ROOT:-/work}"
IMPLEMENTATION="${LIBLZMA_IMPLEMENTATION:-original}"
SOURCE_ROOT=/tmp/liblzma-original
BUILD_ROOT=/tmp/liblzma-build
TEST_ROOT=/tmp/liblzma-dependent-tests
ONLY="${LIBLZMA_TEST_ONLY:-}"
CURRENT_STEP=""
MULTIARCH="$(gcc -print-multiarch)"
MULTIARCH_LIBDIR="/usr/lib/${MULTIARCH}"
TRACKED_ROOT="${READ_ONLY_ROOT}/safe/tests/dependents"
ACTIVE_LIBLZMA=""
ACTIVE_INCLUDE_ROOT=""
APT_LIB="${MULTIARCH_LIBDIR}/libapt-pkg.so.6.0"
LIBXML2_SO="${MULTIARCH_LIBDIR}/libxml2.so.2"
LIBTIFF_SO="${MULTIARCH_LIBDIR}/libtiff.so.6"
LIBARCHIVE_SO="${MULTIARCH_LIBDIR}/libarchive.so.13"
BOOST_IOSTREAMS_SO="${MULTIARCH_LIBDIR}/libboost_iostreams.so.1.83.0"
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
PYTHON_LZMA_SMOKE="${TRACKED_ROOT}/python_lzma_smoke.py"
LIBTIFF_SMOKE_SRC="${TRACKED_ROOT}/libtiff_smoke.c"
GDB_SMOKE_SRC="${TRACKED_ROOT}/gdb_smoke.c"
BOOST_IOSTREAMS_SMOKE_SRC="${TRACKED_ROOT}/boost_iostreams_smoke.cpp"
LIBARCHIVE_TOOLS_SMOKE="${TRACKED_ROOT}/libarchive_tools_smoke.sh"
DPKG_SMOKE_HELPER="${TRACKED_ROOT}/create_dpkg_smoke_package.sh"
APT_SMOKE_HELPER="${TRACKED_ROOT}/create_apt_smoke_repo.sh"
LIBXML2_SMOKE_DOC="${TRACKED_ROOT}/libxml2_document.xml"
KMOD_SMOKE_SRC="${TRACKED_ROOT}/kmod_smoke_module.c"

trap 'rc=$?; if [[ "$rc" -ne 0 && -n "$CURRENT_STEP" ]]; then printf "failed during: %s\n" "$CURRENT_STEP" >&2; fi; exit "$rc"' EXIT

log_step() {
  printf '\n==> %s\n' "$1"
}

die() {
  printf 'error: %s\n' "$*" >&2
  exit 1
}

export_probe_environment() {
  export LIBLZMA_DEPENDENT_ACTIVE_LIBLZMA="$ACTIVE_LIBLZMA"
  export LIBLZMA_DEPENDENT_INCLUDE_DIR="$ACTIVE_INCLUDE_ROOT"
  export LIBLZMA_DEPENDENT_MULTIARCH="$MULTIARCH"
  export LIBLZMA_DEPENDENT_MULTIARCH_LIBDIR="$MULTIARCH_LIBDIR"
  export LIBLZMA_DEPENDENT_REPO_ROOT="$READ_ONLY_ROOT"
  export LIBLZMA_DEPENDENT_TEST_ROOT="$TEST_ROOT"
  export LIBLZMA_DEPENDENT_TRACKED_ROOT="$TRACKED_ROOT"
  export BSDCAT_BIN
  export BSDTAR_BIN
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

assert_tracked_file() {
  local path="$1"

  [[ -f "$path" ]] || die "missing tracked test asset: $path"
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
  ACTIVE_INCLUDE_ROOT="/usr/local/include"
  [[ -n "$ACTIVE_LIBLZMA" && -f "$ACTIVE_LIBLZMA" ]] || die "failed to install local liblzma shared library"
  assert_exists "$ACTIVE_INCLUDE_ROOT/lzma.h"
  export_probe_environment
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
  ACTIVE_INCLUDE_ROOT="/usr/include"
  [[ -n "$ACTIVE_LIBLZMA" && -f "$ACTIVE_LIBLZMA" ]] || die "failed to locate packaged liblzma shared library"
  assert_exists "$ACTIVE_INCLUDE_ROOT/lzma.h"
  export_probe_environment
  cd /
}

test_dpkg() {
  local dir
  local package_path

  CURRENT_STEP="dpkg"
  log_step "dpkg"
  assert_links_to_active_liblzma "$DPKG_DEB_BIN"
  dir="$(reset_test_dir dpkg)"

  package_path="$(bash "$DPKG_SMOKE_HELPER" "$dir")"

  "$DPKG_DEB_BIN" --info "$package_path" >"$dir/info.log"
  require_contains "$dir/info.log" "Package: liblzma-smoke"
  "$DPKG_DEB_BIN" -x "$package_path" "$dir/extract"
  require_contains "$dir/extract/usr/share/liblzma-smoke/message.txt" "payload unpacked through data.tar.xz"
}

test_apt() {
  local dir

  CURRENT_STEP="apt"
  log_step "apt"
  assert_links_to_active_liblzma "$APT_LIB"
  dir="$(reset_test_dir apt)"

  bash "$APT_SMOKE_HELPER" "$dir"

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

  "$PYTHON_BIN" "$PYTHON_LZMA_SMOKE" >"$dir/python.log"

  require_contains "$dir/python.log" "python lzma ok"
}

test_libxml2() {
  local dir

  CURRENT_STEP="libxml2"
  log_step "libxml2"
  assert_links_to_active_liblzma "$LIBXML2_SO"
  dir="$(reset_test_dir libxml2)"

  xz -9 -c "$LIBXML2_SMOKE_DOC" >"$dir/document.xml.xz"

  "$XMLLINT_BIN" --xpath 'string(/root/item)' "$dir/document.xml.xz" >"$dir/xmllint.out"
  require_contains "$dir/xmllint.out" "libxml2 through xz"
}

test_libtiff6() {
  local dir

  CURRENT_STEP="libtiff6"
  log_step "libtiff6"
  assert_links_to_active_liblzma "$LIBTIFF_SO"
  dir="$(reset_test_dir libtiff6)"

  cc \
    -I"$ACTIVE_INCLUDE_ROOT" \
    -o "$dir/libtiff-smoke" \
    "$LIBTIFF_SMOKE_SRC" \
    $(pkg-config --cflags --libs libtiff-4) \
    >/tmp/libtiff-build.log 2>&1
  assert_links_to_active_liblzma "$dir/libtiff-smoke"
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

  gcc -c -o "$dir/module.o" "$KMOD_SMOKE_SRC" >/tmp/kmod-build.log 2>&1
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

  gcc -g -O0 -fno-inline -I"$ACTIVE_INCLUDE_ROOT" -o "$dir/gdb-smoke" "$GDB_SMOKE_SRC" >/tmp/gdb-build.log 2>&1
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
    -ex 'break gdb_smoke.c:11' \
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

  bash "$LIBARCHIVE_TOOLS_SMOKE" "$dir" >"$dir/libarchive-tools.log"
  require_contains "$dir/libarchive-tools.log" "libarchive tools ok"
  require_contains "$dir/list.log" "message.txt"
  require_contains "$dir/output/archive/message.txt" "libarchive tools tar.xz smoke"
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

  g++ -std=c++17 -O2 -I"$ACTIVE_INCLUDE_ROOT" -o "$dir/boost-smoke" "$BOOST_IOSTREAMS_SMOKE_SRC" -lboost_iostreams >/tmp/boost-build.log 2>&1
  assert_links_to_active_liblzma "$dir/boost-smoke"
  "$dir/boost-smoke" >"$dir/boost.log"
  require_contains "$dir/boost.log" "boost lzma ok"
}

assert_tracked_file "$PYTHON_LZMA_SMOKE"
assert_tracked_file "$LIBTIFF_SMOKE_SRC"
assert_tracked_file "$GDB_SMOKE_SRC"
assert_tracked_file "$BOOST_IOSTREAMS_SMOKE_SRC"
assert_tracked_file "$LIBARCHIVE_TOOLS_SMOKE"
assert_tracked_file "$DPKG_SMOKE_HELPER"
assert_tracked_file "$APT_SMOKE_HELPER"
assert_tracked_file "$LIBXML2_SMOKE_DOC"
assert_tracked_file "$KMOD_SMOKE_SRC"

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
