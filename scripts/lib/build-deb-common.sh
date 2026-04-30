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
#                          Run mk-build-deps + dpkg-buildpackage -us -uc in
#                          the current directory (full build: source + binary)
#                          and copy the resulting .deb / .dsc / .tar.* /
#                          .buildinfo / .changes files into $repo_root/dist.
#                          For 3.0 (quilt) ports, synthesizes a deterministic
#                          orig.tar.xz from `git archive HEAD:safe` (minus
#                          debian/) so dpkg-source has an upstream tarball
#                          to anchor the source build against.

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

_synthesize_orig_tarball_if_needed() {
  # SafeLibs ports treat the safe/ tree itself as upstream — there is no
  # separate upstream tarball, and there are no patches to round-trip.
  # For 3.0 (quilt) packages, synthesize a deterministic orig.tar.xz from
  # `git archive HEAD:safe` (minus debian/) so dpkg-source has the
  # upstream view it requires. Wipe debian/patches/ before the build so
  # dpkg-source doesn't try to apply (already-applied) patches.
  [[ -f debian/source/format ]] || return 0
  grep -Fq '3.0 (quilt)' debian/source/format || return 0

  local repo_root package upstream_version orig_tar
  repo_root="$(git rev-parse --show-toplevel)"
  package="$(dpkg-parsechangelog -S Source)"
  # Strip epoch ("N:" prefix) and Debian revision (everything after the
  # last "-") to match dpkg-source's orig.tar naming.
  upstream_version="$(dpkg-parsechangelog -S Version | sed -E 's/^[0-9]+://; s/-[^-]*$//')"
  orig_tar="../${package}_${upstream_version}.orig.tar.xz"

  if [[ ! -f "$orig_tar" ]]; then
    local stage prefix
    stage="$(mktemp -d)"
    prefix="${package}-${upstream_version}"

    # git archive HEAD:safe gives the exact tracked safe/ subtree at
    # this commit — no untracked files, no build artifacts. The prefix
    # places everything under <pkg>-<upstream>/.
    git -C "$repo_root" archive --format=tar --prefix="${prefix}/" HEAD:safe \
      | tar -xf - -C "$stage"

    # debian/ ships in the debian.tar overlay, not orig.tar.
    rm -rf "$stage/${prefix}/debian"

    tar --create --xz --file="$orig_tar" \
      --sort=name --owner=0 --group=0 --numeric-owner --mtime='@0' \
      -C "$stage" "$prefix"

    rm -rf "$stage"
  fi

  # SafeLibs ports' safe/ trees are already in post-patch state, so any
  # debian/patches/series content is stale (and would either fail to
  # apply or corrupt the tree). Drop the patches directory entirely so
  # dpkg-source treats the source as un-patched.
  rm -rf debian/patches
}

build_with_dpkg_buildpackage() {
  local repo_root="$1"
  sudo mk-build-deps -i -r -t "apt-get -y --no-install-recommends" debian/control
  _synthesize_orig_tarball_if_needed
  dpkg-buildpackage -us -uc
  shopt -s nullglob
  local artifacts=(
    ../*.deb
    ../*.ddeb
    ../*.dsc
    ../*.tar.gz ../*.tar.xz ../*.tar.bz2 ../*.tar.zst
    ../*.buildinfo
    ../*.changes
  )
  shopt -u nullglob
  if (( ${#artifacts[@]} == 0 )); then
    printf 'build_with_dpkg_buildpackage: dpkg-buildpackage produced no artifacts\n' >&2
    return 1
  fi
  cp -v "${artifacts[@]}" "$repo_root/dist"/
}
