#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"

fail() {
  printf 'check-layout: %s\n' "$*" >&2
  exit 1
}

require_file() {
  local path="$1"

  [[ -f "$repo_root/$path" ]] || fail "missing required file: $path"
}

require_dir() {
  local path="$1"

  [[ -d "$repo_root/$path" ]] || fail "missing required directory: $path"
}

require_executable() {
  local path="$1"

  [[ -x "$repo_root/$path" ]] || fail "required file is not executable: $path"
}

validate_json() {
  local path="$1"

  require_file "$path"
  python3 -m json.tool "$repo_root/$path" >/dev/null || fail "invalid JSON: $path"
}

validate_shell_syntax() {
  local path="$1"

  require_file "$path"
  bash -n "$repo_root/$path" || fail "invalid shell syntax: $path"
}

validate_package_metadata() {
  validate_shell_syntax "packaging/package.env"
  validate_shell_syntax "scripts/build-debs.sh"

  local library
  library="$(
    set -euo pipefail
    cd "$repo_root"
    unset SAFELIBS_LIBRARY
    # shellcheck source=/dev/null
    . "$repo_root/packaging/package.env"
    printf '%s' "${SAFELIBS_LIBRARY:-}"
  )" || fail "failed to source packaging/package.env"

  [[ -n "$library" ]] || fail "packaging/package.env must set SAFELIBS_LIBRARY"
  [[ "$library" =~ ^[a-z0-9][a-z0-9_-]*$ ]] || fail "invalid SAFELIBS_LIBRARY: $library"
}

require_gitattributes_entry() {
  local entry="$1"

  grep -Fxq -- "$entry" "$repo_root/.gitattributes" || fail "missing .gitattributes entry: $entry"
}

required_dirs=(
  original
  safe
  packaging
  docs
  tests/upstream
  tests/port
  scripts
)

required_files=(
  .github/workflows/ci-release.yml
  .gitattributes
  README.md
  AGENTS.md
  CLAUDE.md
  all_cves.json
  dependents.json
  docs/PORTING.md
  docs/PUBLISHING.md
  relevant_cves.json
  packaging/package.env
)

json_files=(
  all_cves.json
  dependents.json
  relevant_cves.json
)

executable_files=(
  scripts/build-debs.sh
  scripts/check-layout.sh
  scripts/install-build-deps.sh
  scripts/run-port-tests.sh
  scripts/run-tests.sh
  scripts/run-upstream-tests.sh
  scripts/run-validation-tests.sh
)

gitattributes_entries=(
  "*.sh text eol=lf"
  "*.json text eol=lf"
  "*.md text eol=lf"
  "*.env text eol=lf"
  ".github/workflows/*.yml text eol=lf"
)

for path in "${required_dirs[@]}"; do
  require_dir "$path"
done

for path in "${required_files[@]}"; do
  require_file "$path"
done

for path in "${json_files[@]}"; do
  validate_json "$path"
done

for path in "${executable_files[@]}"; do
  require_executable "$path"
done

for entry in "${gitattributes_entries[@]}"; do
  require_gitattributes_entry "$entry"
done

validate_package_metadata

printf 'Layout check passed.\n'
