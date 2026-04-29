#!/usr/bin/env bash
# liblzma: drop the cdylib crate-type so the build only emits the
# staticlib + rlib outputs the debian rules expect, then run the
# standard safe-debian build via the shared helper.
set -euo pipefail

repo_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=/dev/null
. "$repo_root/scripts/lib/build-deb-common.sh"

prepare_rust_env
prepare_dist_dir "$repo_root"

python3 - <<'PY'
from pathlib import Path

cargo_toml = Path("safe/Cargo.toml")
old = 'crate-type = ["cdylib", "staticlib", "rlib"]\n'
new = 'crate-type = ["staticlib", "rlib"]\n'
text = cargo_toml.read_text()
if old in text and new not in text:
    cargo_toml.write_text(text.replace(old, new, 1))
PY

cd "$repo_root/safe"
stamp_safelibs_changelog "$repo_root"
build_with_dpkg_buildpackage "$repo_root"
