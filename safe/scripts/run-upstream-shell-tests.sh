#!/usr/bin/env bash
set -euo pipefail

if [[ "${1:-}" == "--force" ]]; then
  printf 'upstream shell tests are intentionally deferred in phase01 because the current safe liblzma is a link-only ABI shell.\n' >&2
  exit 1
fi

printf 'phase01 scaffold: upstream shell tests are not executed by default against the link-only ABI shell.\n'
