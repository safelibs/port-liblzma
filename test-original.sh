#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
ONLY=""
IMPLEMENTATION="original"
DEFAULT_SAFE_PACKAGE_DIR="$ROOT/safe/dist"
SAFE_PACKAGE_DIR="$DEFAULT_SAFE_PACKAGE_DIR"
ORIGINAL_IMAGE_TAG="${LIBLZMA_ORIGINAL_TEST_IMAGE:-liblzma-dependent-test:ubuntu24.04-original}"
SAFE_IMAGE_TAG="${LIBLZMA_SAFE_TEST_IMAGE:-liblzma-dependent-test:ubuntu24.04-safe}"

usage() {
  cat <<'EOF'
usage: test-original.sh [--only <binary-package>] [--implementation <original|safe>] [--safe-package-dir <dir>]

Builds and tests either the vendored original liblzma or the packaged safe
replacement inside Docker, then smoke-tests the Ubuntu 24.04 dependent
packages recorded in dependents.json against the selected implementation.
Safe mode builds the image with the staged liblzma5/liblzma-dev packages and
then exercises the installed headers and runtime inside that image.

--only runs just one dependent by exact .dependents[].binary_package.
--implementation defaults to original.
--safe-package-dir defaults to safe/dist.
EOF
}

while (($#)); do
  case "$1" in
    --only)
      ONLY="${2:?missing value for --only}"
      shift 2
      ;;
    --implementation)
      IMPLEMENTATION="${2:?missing value for --implementation}"
      shift 2
      ;;
    --safe-package-dir)
      SAFE_PACKAGE_DIR="${2:?missing value for --safe-package-dir}"
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

case "$IMPLEMENTATION" in
  original|safe)
    ;;
  *)
    printf 'unknown implementation: %s\n' "$IMPLEMENTATION" >&2
    usage >&2
    exit 1
    ;;
esac

if [[ "$SAFE_PACKAGE_DIR" != /* ]]; then
  SAFE_PACKAGE_DIR="$ROOT/$SAFE_PACKAGE_DIR"
fi

for tool in docker python3; do
  command -v "$tool" >/dev/null 2>&1 || {
    printf 'missing required host tool: %s\n' "$tool" >&2
    exit 1
  }
done

[[ -d "$ROOT/original" ]] || {
  echo "missing original source tree" >&2
  exit 1
}

[[ -f "$ROOT/dependents.json" ]] || {
  echo "missing dependents.json" >&2
  exit 1
}

[[ -f "$ROOT/safe/docker/dependent-test.Dockerfile" ]] || {
  echo "missing tracked dependent-test Dockerfile" >&2
  exit 1
}

[[ -f "$ROOT/safe/scripts/build-dependent-test-image.sh" ]] || {
  echo "missing dependent image builder" >&2
  exit 1
}

[[ -f "$ROOT/safe/scripts/run-dependent-smokes.sh" ]] || {
  echo "missing dependent smoke runner" >&2
  exit 1
}

for tracked_asset in \
  "$ROOT/safe/tests/dependents/python_lzma_smoke.py" \
  "$ROOT/safe/tests/dependents/libtiff_smoke.c" \
  "$ROOT/safe/tests/dependents/gdb_smoke.c" \
  "$ROOT/safe/tests/dependents/boost_iostreams_smoke.cpp" \
  "$ROOT/safe/tests/dependents/libarchive_tools_smoke.sh" \
  "$ROOT/safe/tests/dependents/create_dpkg_smoke_package.sh" \
  "$ROOT/safe/tests/dependents/create_apt_smoke_repo.sh" \
  "$ROOT/safe/tests/dependents/libxml2_document.xml" \
  "$ROOT/safe/tests/dependents/kmod_smoke_module.c"; do
  [[ -f "$tracked_asset" ]] || {
    printf 'missing tracked dependent asset: %s\n' "${tracked_asset#"$ROOT/"}" >&2
    exit 1
  }
done

python3 - "$ROOT/dependents.json" "$ONLY" <<'PY'
import json
import sys
from pathlib import Path

expected = [
    "dpkg",
    "apt",
    "python3.12",
    "libxml2",
    "libtiff6",
    "squashfs-tools",
    "kmod",
    "gdb",
    "libarchive13t64",
    "libarchive-tools",
    "mariadb-plugin-provider-lzma",
    "libboost-iostreams1.83.0",
]

data = json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))
actual = [entry["binary_package"] for entry in data["dependents"]]

if actual != expected:
    raise SystemExit(
        f"unexpected dependents.json contents: expected {expected}, found {actual}"
    )

only = sys.argv[2]
if only and only not in set(actual):
    raise SystemExit(f"unknown --only binary package: {only}")
PY

case "$IMPLEMENTATION" in
  original)
    IMAGE_TAG="$ORIGINAL_IMAGE_TAG"
    ;;
  safe)
    IMAGE_TAG="$SAFE_IMAGE_TAG"
    ;;
esac

build_args=(
  --implementation "$IMPLEMENTATION"
  --image-tag "$IMAGE_TAG"
)

if [[ "$IMPLEMENTATION" == "safe" ]]; then
  build_args+=(
    --safe-package-dir "$SAFE_PACKAGE_DIR"
  )
fi

"$ROOT/safe/scripts/build-dependent-test-image.sh" "${build_args[@]}"

docker run \
  --rm \
  -i \
  --cap-add SYS_PTRACE \
  --security-opt seccomp=unconfined \
  -e "LIBLZMA_TEST_ONLY=$ONLY" \
  -e "LIBLZMA_IMPLEMENTATION=$IMPLEMENTATION" \
  -e "LIBLZMA_READ_ONLY_ROOT=/work" \
  -v "$ROOT:/work:ro" \
  "$IMAGE_TAG" \
  bash /work/safe/scripts/run-dependent-smokes.sh
