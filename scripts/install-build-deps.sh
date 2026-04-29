#!/usr/bin/env bash
# Install apt packages and a stable rust toolchain needed to
# dpkg-buildpackage the safe port.
set -euo pipefail

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

# Install rustup into $HOME so we don't pick up the runner's preinstalled
# (older) system rust.
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
  | sh -s -- -y --profile minimal --default-toolchain stable --no-modify-path

# shellcheck source=/dev/null
. "$HOME/.cargo/env"
rustup default stable
rustc --version
cargo --version

# Persist for subsequent CI steps (build-debs.sh runs in a fresh shell).
if [[ -n "${GITHUB_PATH:-}" ]]; then
  printf '%s\n' "$HOME/.cargo/bin" >> "$GITHUB_PATH"
fi
