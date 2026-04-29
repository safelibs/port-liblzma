# shellcheck shell=bash
# Shared bash helpers used by per-port scripts/build-debs.sh.
# Source this file from a port's build-debs.sh after computing $repo_root.
#
# Functions:
#   prepare_rust_env       Source ~/.cargo/env, prepend ~/.cargo/bin to PATH.
#   prepare_dist_dir       Recreate $repo_root/dist as an empty directory.
#   stamp_safelibs_changelog
#                          Rewrite the leading entry of debian/changelog with
#                          version "<upstream>+safelibs<commit-epoch>". Must
#                          run in a directory containing debian/changelog.
#                          Honors $SAFELIBS_COMMIT_SHA when set.
#   build_with_dpkg_buildpackage
#                          Run mk-build-deps + dpkg-buildpackage -us -uc -b in
#                          the current directory and copy ../*.deb into
#                          $repo_root/dist.

prepare_rust_env() {
  if [[ -f "$HOME/.cargo/env" ]]; then
    # shellcheck source=/dev/null
    . "$HOME/.cargo/env"
  fi
  if [[ -d "$HOME/.cargo/bin" ]]; then
    case ":$PATH:" in
      *":$HOME/.cargo/bin:"*) ;;
      *) export PATH="$HOME/.cargo/bin:$PATH" ;;
    esac
  fi
}

prepare_dist_dir() {
  local repo_root="$1"
  rm -rf -- "$repo_root/dist"
  mkdir -p -- "$repo_root/dist"
}

_safelibs_commit_epoch() {
  local repo_root="$1"
  local epoch=""

  if [[ -n "${SAFELIBS_COMMIT_SHA:-}" ]] \
     && command -v git >/dev/null 2>&1 \
     && git -C "$repo_root" cat-file -e "$SAFELIBS_COMMIT_SHA^{commit}" 2>/dev/null; then
    epoch="$(git -C "$repo_root" log -1 --format=%ct "$SAFELIBS_COMMIT_SHA")"
  elif command -v git >/dev/null 2>&1 \
     && git -C "$repo_root" rev-parse HEAD >/dev/null 2>&1; then
    epoch="$(git -C "$repo_root" log -1 --format=%ct HEAD)"
  fi

  if [[ -z "$epoch" ]]; then
    epoch="$(date -u +%s)"
  fi

  printf '%s' "$epoch"
}

stamp_safelibs_changelog() {
  local repo_root="${1:-}"
  if [[ -z "$repo_root" ]]; then
    repo_root="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
  fi

  local upstream_version package_name distribution commit_epoch new_version release_date
  upstream_version="$(dpkg-parsechangelog -S Version | sed -E 's/\+safelibs[0-9]+$//')"
  package_name="$(dpkg-parsechangelog -S Source)"
  distribution="$(dpkg-parsechangelog -S Distribution)"
  commit_epoch="$(_safelibs_commit_epoch "$repo_root")"
  new_version="${upstream_version}+safelibs${commit_epoch}"
  release_date="$(date -u -R -d "@${commit_epoch}")"

  {
    printf '%s (%s) %s; urgency=medium\n\n  * Automated SafeLibs rebuild.\n\n -- SafeLibs CI <ci@safelibs.org>  %s\n\n' \
      "$package_name" "$new_version" "$distribution" "$release_date"
    cat debian/changelog
  } > debian/changelog.new
  mv debian/changelog.new debian/changelog
}

build_with_dpkg_buildpackage() {
  local repo_root="$1"
  sudo mk-build-deps -i -r -t "apt-get -y --no-install-recommends" debian/control
  dpkg-buildpackage -us -uc -b
  cp -v ../*.deb "$repo_root/dist"/
}
