#!/usr/bin/env bash
# Build the safe port via dpkg-buildpackage rooted in safe/.
# Stamps the changelog with `+safelibs<commit-epoch>` so the produced
# .deb files have a deterministic version that wins over Ubuntu's copy
# under the apt pin in safelibs/apt.
set -euo pipefail

repo_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
dist_dir="$repo_root/dist"

# shellcheck source=/dev/null
[ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"

if [[ -d "$HOME/.cargo/bin" ]]; then
  case ":$PATH:" in
    *":$HOME/.cargo/bin:"*) ;;
    *) export PATH="$HOME/.cargo/bin:$PATH" ;;
  esac
fi

rm -rf -- "$dist_dir"
mkdir -p -- "$dist_dir"

# liblzma-specific setup: drop the cdylib crate-type so the build only
# emits the staticlib + rlib outputs the debian rules expect.
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

upstream_version="$(dpkg-parsechangelog -S Version | sed -E 's/\+safelibs[0-9]+$//')"
package_name="$(dpkg-parsechangelog -S Source)"
distribution="$(dpkg-parsechangelog -S Distribution)"

if [[ -n "${SAFELIBS_COMMIT_SHA:-}" ]] \
   && command -v git >/dev/null 2>&1 \
   && git -C "$repo_root" cat-file -e "$SAFELIBS_COMMIT_SHA^{commit}" 2>/dev/null; then
  commit_epoch="$(git -C "$repo_root" log -1 --format=%ct "$SAFELIBS_COMMIT_SHA")"
elif command -v git >/dev/null 2>&1 && git -C "$repo_root" rev-parse HEAD >/dev/null 2>&1; then
  commit_epoch="$(git -C "$repo_root" log -1 --format=%ct HEAD)"
else
  commit_epoch="$(date -u +%s)"
fi

new_version="${upstream_version}+safelibs${commit_epoch}"
release_date="$(date -u -R -d "@${commit_epoch}")"

{
  printf '%s (%s) %s; urgency=medium\n\n  * Automated SafeLibs rebuild.\n\n -- SafeLibs CI <ci@safelibs.org>  %s\n\n' \
    "$package_name" "$new_version" "$distribution" "$release_date"
  cat debian/changelog
} > debian/changelog.new
mv debian/changelog.new debian/changelog

sudo mk-build-deps -i -r -t "apt-get -y --no-install-recommends" debian/control
dpkg-buildpackage -us -uc -b

cp -v ../*.deb "$dist_dir"/
