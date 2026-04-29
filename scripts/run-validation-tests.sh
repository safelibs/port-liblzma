#!/usr/bin/env bash
# Run the safelibs/validator test matrix in port-04-test mode against the
# .deb files this repository just built into dist/.
#
# Inputs:
#   - dist/*.deb                produced by scripts/build-debs.sh
#   - packaging/package.env     supplies SAFELIBS_LIBRARY
#   - SAFELIBS_COMMIT_SHA       commit identity for the synthetic port lock
#                               (falls back to git HEAD, then GITHUB_SHA, then
#                               a deterministic placeholder)
#
# Optional environment overrides:
#   - SAFELIBS_VALIDATOR_DIR    path to an existing validator checkout; when
#                               unset, the script clones safelibs/validator
#                               into .work/validator
#   - SAFELIBS_VALIDATOR_REF    git ref to clone (default: main)
#   - SAFELIBS_VALIDATOR_REPO   git remote (default: https://github.com/safelibs/validator)
#   - SAFELIBS_RECORD_CASTS     non-empty -> pass --record-casts to test.sh
#
# A library that has no entry in the validator's repositories.yml is a soft
# success (typical for the template itself or in-progress ports). A library
# with a validator entry but no matching dist/*.deb is a hard failure.
set -euo pipefail

repo_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"

fail() {
  printf 'run-validation-tests: %s\n' "$*" >&2
  exit 1
}

note() {
  printf 'run-validation-tests: %s\n' "$*"
}

package_env="$repo_root/packaging/package.env"
[[ -f "$package_env" ]] || fail "missing packaging/package.env"

# shellcheck source=/dev/null
. "$package_env"

[[ -n "${SAFELIBS_LIBRARY:-}" ]] || fail "SAFELIBS_LIBRARY is not set in packaging/package.env"
[[ "$SAFELIBS_LIBRARY" =~ ^[a-z0-9][a-z0-9_-]*$ ]] || fail "invalid SAFELIBS_LIBRARY: $SAFELIBS_LIBRARY"

dist_dir="$repo_root/dist"
[[ -d "$dist_dir" ]] || fail "missing dist/ directory; run scripts/build-debs.sh first"

shopt -s nullglob
debs=("$dist_dir"/*.deb)
shopt -u nullglob
(( ${#debs[@]} > 0 )) || fail "no .deb artifacts in dist/; run scripts/build-debs.sh first"

commit_sha="${SAFELIBS_COMMIT_SHA:-}"
if [[ -z "$commit_sha" ]] && command -v git >/dev/null 2>&1 \
   && git -C "$repo_root" rev-parse HEAD >/dev/null 2>&1; then
  commit_sha="$(git -C "$repo_root" rev-parse HEAD)"
fi
if [[ -z "$commit_sha" ]]; then
  commit_sha="${GITHUB_SHA:-0000000000000000000000000000000000000000}"
fi

work_dir="$repo_root/.work/validation"
rm -rf -- "$work_dir"
mkdir -p -- "$work_dir"

validator_dir="${SAFELIBS_VALIDATOR_DIR:-}"
if [[ -z "$validator_dir" ]]; then
  validator_dir="$repo_root/.work/validator"
  validator_ref="${SAFELIBS_VALIDATOR_REF:-main}"
  validator_repo="${SAFELIBS_VALIDATOR_REPO:-https://github.com/safelibs/validator}"
  if [[ -d "$validator_dir/.git" ]]; then
    note "refreshing existing validator checkout at $validator_dir"
    git -C "$validator_dir" fetch --depth=1 origin "$validator_ref"
    git -C "$validator_dir" checkout --force FETCH_HEAD
  else
    note "cloning $validator_repo @ $validator_ref into $validator_dir"
    rm -rf -- "$validator_dir"
    mkdir -p -- "$(dirname -- "$validator_dir")"
    git clone --depth=1 --branch "$validator_ref" "$validator_repo" "$validator_dir"
  fi
fi

[[ -f "$validator_dir/test.sh" ]] || fail "validator checkout missing test.sh: $validator_dir"
[[ -f "$validator_dir/repositories.yml" ]] || fail "validator checkout missing repositories.yml: $validator_dir"

override_root="$work_dir/override-debs"
lock_path="$work_dir/port-deb-lock.json"
artifact_root="$work_dir/artifacts"
mkdir -p -- "$override_root" "$artifact_root"

note "synthesizing port-04-test lock for $SAFELIBS_LIBRARY at commit ${commit_sha:0:12}"
build_status=0
SAFELIBS_LIBRARY="$SAFELIBS_LIBRARY" \
SAFELIBS_COMMIT_SHA="$commit_sha" \
SAFELIBS_DIST_DIR="$dist_dir" \
SAFELIBS_VALIDATOR_DIR="$validator_dir" \
SAFELIBS_LOCK_PATH="$lock_path" \
SAFELIBS_OVERRIDE_ROOT="$override_root" \
python3 "$repo_root/scripts/lib/build_port_lock.py" || build_status=$?
if (( build_status == 2 )); then
  note "library $SAFELIBS_LIBRARY has no validator manifest entry; skipping validator tests"
  exit 0
fi
if (( build_status != 0 )); then
  exit "$build_status"
fi

cast_arg=()
if [[ -n "${SAFELIBS_RECORD_CASTS:-}" ]]; then
  cast_arg=(--record-casts)
fi

note "running validator matrix for $SAFELIBS_LIBRARY"
bash "$validator_dir/test.sh" \
  --library "$SAFELIBS_LIBRARY" \
  --mode port-04-test \
  --override-deb-root "$override_root" \
  --port-deb-lock "$lock_path" \
  --artifact-root "$artifact_root" \
  "${cast_arg[@]}"
