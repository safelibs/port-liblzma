#!/usr/bin/env bash
# Install apt packages and a rust toolchain needed to build the safe
# port. Honors safe/rust-toolchain.toml when present (auto-detect the
# pinned channel); falls back to stable. Override SAFELIBS_RUST_TOOLCHAIN
# to force a specific toolchain regardless of the file.
#
# The reference template build only needs `dpkg-deb` (preinstalled on
# ubuntu-latest), so this script's apt + rustup install only matters for
# real ports overriding scripts/build-debs.sh. Such ports may safely
# replace this script with their own apt/rustup logic when their build
# needs more (clang+lld, autoconf, cmake, etc.).
set -euo pipefail

repo_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"

toolchain="${SAFELIBS_RUST_TOOLCHAIN:-}"
if [[ -z "$toolchain" && -f "$repo_root/safe/rust-toolchain.toml" ]]; then
  toolchain="$(grep -oP '^channel\s*=\s*"\K[^"]+' "$repo_root/safe/rust-toolchain.toml" || true)"
fi
toolchain="${toolchain:-stable}"

export DEBIAN_FRONTEND=noninteractive
sudo apt-get update
sudo apt-get install -y --no-install-recommends \
  build-essential \
  ca-certificates \
  curl \
  devscripts \
  dpkg-dev \
  equivs \
  fakeroot \
  file \
  git \
  jq \
  python3 \
  rsync \
  xz-utils

# Always install rustup into $HOME so subsequent CI steps see the pinned
# toolchain instead of the runner's preinstalled (older) system rust.
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
  | sh -s -- -y --profile minimal --default-toolchain "$toolchain" --no-modify-path

# shellcheck source=/dev/null
. "$HOME/.cargo/env"
rustup default "$toolchain"
rustc --version
cargo --version

# Persist for subsequent CI steps (build-debs.sh runs in a fresh shell).
if [[ -n "${GITHUB_PATH:-}" ]]; then
  printf '%s\n' "$HOME/.cargo/bin" >> "$GITHUB_PATH"
fi
