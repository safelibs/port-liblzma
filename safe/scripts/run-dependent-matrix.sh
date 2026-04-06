#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
safe_dir="$(cd -- "$script_dir/.." && pwd)"
repo_root="$(cd -- "$safe_dir/.." && pwd)"
dependents_json="$repo_root/dependents.json"
runner="$repo_root/test-original.sh"
report_path="$repo_root/safe/tests/generated/dependent-matrix.json"
log_dir="$repo_root/safe/tests/generated/dependent-matrix"
implementation="safe"
allow_failures=0
default_safe_package_dir="$repo_root/safe/dist"
safe_package_dir="$default_safe_package_dir"

usage() {
  cat <<'EOF'
usage: safe/scripts/run-dependent-matrix.sh [--implementation <original|safe>] [--safe-package-dir <dir>] [--report <path>] [--allow-failures]

Runs ./test-original.sh --only <binary_package> once per dependent listed in
dependents.json, captures deterministic per-package logs, and refreshes
safe/tests/generated/dependent-matrix.json with the ordered results.

--implementation defaults to safe.
--safe-package-dir defaults to safe/dist.
--report defaults to safe/tests/generated/dependent-matrix.json.
--allow-failures exits zero after the full sweep even if some packages fail.
EOF
}

while (($#)); do
  case "$1" in
    --implementation)
      implementation="${2:?missing value for --implementation}"
      shift 2
      ;;
    --safe-package-dir)
      safe_package_dir="${2:?missing value for --safe-package-dir}"
      shift 2
      ;;
    --report)
      report_path="${2:?missing value for --report}"
      shift 2
      ;;
    --allow-failures)
      allow_failures=1
      shift
      ;;
    --help|-h)
      usage
      exit 0
      ;;
    *)
      printf 'unknown option: %s\n' "$1" >&2
      usage >&2
      exit 1
      ;;
  esac
done

case "$implementation" in
  original|safe)
    ;;
  *)
    printf 'unknown implementation: %s\n' "$implementation" >&2
    usage >&2
    exit 1
    ;;
esac

if [[ "$safe_package_dir" != /* ]]; then
  safe_package_dir="$repo_root/$safe_package_dir"
fi

if [[ "$report_path" != /* ]]; then
  report_path="$repo_root/$report_path"
fi

command -v python3 >/dev/null 2>&1 || {
  printf 'missing required host tool: python3\n' >&2
  exit 1
}

[[ -f "$dependents_json" ]] || {
  printf 'missing dependents.json: %s\n' "$dependents_json" >&2
  exit 1
}

[[ -x "$runner" ]] || {
  printf 'missing test runner: %s\n' "$runner" >&2
  exit 1
}

mkdir -p "$log_dir"
find "$log_dir" -maxdepth 1 -type f -name '*.log' -delete

results_tsv="$(mktemp)"
trap 'rm -f "$results_tsv"' EXIT

mapfile -t packages < <(
  python3 - "$dependents_json" <<'PY'
import json
import sys
from pathlib import Path

data = json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))
for entry in data["dependents"]:
    print(entry["binary_package"])
PY
)

have_failures=0

for package in "${packages[@]}"; do
  log_path_rel="safe/tests/generated/dependent-matrix/${package}.log"
  log_path_abs="$repo_root/$log_path_rel"
  cmd=("$runner" "--only" "$package" "--implementation" "$implementation")

  if [[ "$implementation" == "safe" ]]; then
    cmd+=("--safe-package-dir" "$safe_package_dir")
  fi

  {
    printf '$'
    printf ' %q' "${cmd[@]}"
    printf '\n'
  } >"$log_path_abs"

  set +e
  "${cmd[@]}" >>"$log_path_abs" 2>&1
  rc=$?
  set -e

  status="pass"
  if [[ "$rc" -ne 0 ]]; then
    status="fail"
    have_failures=1
  fi

  printf '%s\t%s\t%s\n' "$package" "$status" "$log_path_rel" >>"$results_tsv"
done

python3 - "$results_tsv" "$report_path" "$implementation" "$safe_package_dir" <<'PY'
import json
import sys
from pathlib import Path

results_tsv, report_path, implementation, safe_package_dir = sys.argv[1:5]
results = []

for line in Path(results_tsv).read_text(encoding="utf-8").splitlines():
    binary_package, status, log_path = line.split("\t")
    results.append(
        {
            "binary_package": binary_package,
            "status": status,
            "log_path": log_path,
        }
    )

payload = {"implementation": implementation}

if implementation == "safe":
    payload["safe_package_dir"] = safe_package_dir

payload["results"] = results

Path(report_path).parent.mkdir(parents=True, exist_ok=True)
Path(report_path).write_text(json.dumps(payload, indent=2) + "\n", encoding="utf-8")
PY

if [[ "$have_failures" -ne 0 && "$allow_failures" -eq 0 ]]; then
  exit 1
fi
