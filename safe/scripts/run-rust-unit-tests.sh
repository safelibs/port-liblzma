#!/usr/bin/env bash
set -euo pipefail

script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
repo_root=$(cd "$script_dir/../.." && pwd)

cargo test --manifest-path "$repo_root/safe/Cargo.toml" --offline --locked
cargo test --manifest-path "$repo_root/safe/fuzz/Cargo.toml" --offline --locked
