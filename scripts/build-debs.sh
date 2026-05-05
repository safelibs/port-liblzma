#!/usr/bin/env bash
# Build the safe port via dpkg-buildpackage rooted in safe/. Stamps the
# changelog with `+safelibs<commit-epoch>` so the produced .deb files
# carry a deterministic version that wins over Ubuntu's copy under the
# apt pin in safelibs/apt.
set -euo pipefail

repo_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=/dev/null
. "$repo_root/scripts/lib/build-deb-common.sh"

prepare_rust_env
prepare_dist_dir "$repo_root"

cd "$repo_root/safe"
stamp_safelibs_changelog "$repo_root"
build_with_dpkg_buildpackage "$repo_root"
