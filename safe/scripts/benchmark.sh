#!/usr/bin/env bash
set -euo pipefail

# Serial-only script: this rebuilds and relinks the release shared library in
# place before benchmarking it. Do not run it concurrently with
# relink-release-shared.sh, release-verify.sh, compare-exports.sh,
# check-symbol-versions.sh, build-deb.sh, or other jobs that rewrite
# safe/target/release/, safe/target/relink/, or safe/dist/.

script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
safe_dir=$(cd "$script_dir/.." && pwd)
repo_root=$(cd "$safe_dir/.." && pwd)
ref_lib="build/src/liblzma/.libs/liblzma.so.5.4.5"

iterations=${LIBLZMA_BENCH_ITERATIONS:-5}
size_mib=${LIBLZMA_BENCH_SIZE_MIB:-8}
warn_ratio=${LIBLZMA_BENCH_WARN_RATIO:-}
encode_text_warn_ratio=${LIBLZMA_BENCH_WARN_RATIO_ENCODE_TEXT:-0.20}
encode_random_warn_ratio=${LIBLZMA_BENCH_WARN_RATIO_ENCODE_RANDOM:-0.90}
decode_text_warn_ratio=${LIBLZMA_BENCH_WARN_RATIO_DECODE_TEXT:-0.12}
decode_random_warn_ratio=${LIBLZMA_BENCH_WARN_RATIO_DECODE_RANDOM:-0.08}
tmpdir=$(mktemp -d)
trap 'rm -rf "$tmpdir"' EXIT

while (($#)); do
  case "$1" in
    --iterations)
      iterations="${2:?missing value for --iterations}"
      shift 2
      ;;
    --size-mib)
      size_mib="${2:?missing value for --size-mib}"
      shift 2
      ;;
    --warn-ratio)
      warn_ratio="${2:?missing value for --warn-ratio}"
      shift 2
      ;;
    --reference)
      ref_lib="${2:?missing value for --reference}"
      shift 2
      ;;
    --help|-h)
      cat <<EOF
usage: $(basename "$0") [--iterations N] [--size-mib N] [--warn-ratio R] [--reference PATH]

Benchmarks the safe release library against the reference
$ref_lib on representative encode and decode workloads.

By default the comparison uses the final signoff floors for each workload.
Use --warn-ratio or LIBLZMA_BENCH_WARN_RATIO to force one global floor instead.
EOF
      exit 0
      ;;
    *)
      printf 'unknown option: %s\n' "$1" >&2
      exit 1
      ;;
  esac
done

if [[ "$ref_lib" != /* ]]; then
  ref_lib="$repo_root/$ref_lib"
fi

command -v cc >/dev/null 2>&1 || {
  printf 'missing required tool: cc\n' >&2
  exit 1
}
command -v python3 >/dev/null 2>&1 || {
  printf 'missing required tool: python3\n' >&2
  exit 1
}
[[ -f "$ref_lib" ]] || {
  printf 'missing reference library: %s\n' "$ref_lib" >&2
  exit 1
}

"$script_dir/relink-release-shared.sh" >/dev/null

safe_lib="$safe_dir/target/release/liblzma.so"
safe_runlib="$tmpdir/lib-safe"
ref_runlib="$tmpdir/lib-ref"
mkdir -p "$safe_runlib" "$ref_runlib"

ln -sf "$(readlink -f "$safe_lib")" "$safe_runlib/liblzma.so.5"
ln -sf "liblzma.so.5" "$safe_runlib/liblzma.so"
ln -sf "$(readlink -f "$ref_lib")" "$ref_runlib/liblzma.so.5"
ln -sf "liblzma.so.5" "$ref_runlib/liblzma.so"

cat >"$tmpdir/bench_helper.c" <<'EOF'
#include <errno.h>
#include <inttypes.h>
#include <lzma.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static volatile uint64_t bench_sink = 0;

static void die(const char *msg) {
  fprintf(stderr, "%s\n", msg);
  exit(1);
}

static void slurp(const char *path, uint8_t **buf, size_t *len) {
  FILE *fp = fopen(path, "rb");
  long end;
  long size;

  if (fp == NULL)
    die("fopen failed");
  if (fseek(fp, 0, SEEK_END) != 0)
    die("fseek failed");
  end = ftell(fp);
  if (end < 0)
    die("ftell failed");
  size = end;
  if (fseek(fp, 0, SEEK_SET) != 0)
    die("fseek failed");

  *len = (size_t)size;
  *buf = malloc(*len == 0 ? 1 : *len);
  if (*buf == NULL)
    die("malloc failed");

  if (*len != 0 && fread(*buf, 1, *len, fp) != *len)
    die("fread failed");
  fclose(fp);
}

static void emit_file(const char *path, const uint8_t *buf, size_t len) {
  FILE *fp = fopen(path, "wb");
  if (fp == NULL)
    die("fopen output failed");
  if (len != 0 && fwrite(buf, 1, len, fp) != len)
    die("fwrite failed");
  fclose(fp);
}

static void encode_to_memory(const uint8_t *input, size_t input_len, uint8_t **output, size_t *output_len) {
  size_t out_cap = lzma_stream_buffer_bound(input_len);
  lzma_ret ret;

  *output = malloc(out_cap == 0 ? 1 : out_cap);
  if (*output == NULL)
    die("malloc failed");

  *output_len = 0;
  ret = lzma_easy_buffer_encode(6, LZMA_CHECK_CRC64, NULL,
      input, input_len, *output, output_len, out_cap);
  if (ret != LZMA_OK)
    die("lzma_easy_buffer_encode failed");
  if (*output_len != 0)
    bench_sink ^= (*output)[0] ^ (*output)[*output_len - 1];
}

static void decode_to_memory(const uint8_t *input, size_t input_len) {
  uint8_t outbuf[65536];
  lzma_stream strm = LZMA_STREAM_INIT;
  lzma_ret ret = lzma_stream_decoder(&strm, 300ULL << 20, LZMA_CONCATENATED);
  if (ret != LZMA_OK)
    die("lzma_stream_decoder failed");

  strm.next_in = input;
  strm.avail_in = input_len;
  strm.next_out = outbuf;
  strm.avail_out = sizeof(outbuf);

  while ((ret = lzma_code(&strm, LZMA_FINISH)) == LZMA_OK) {
    if (strm.avail_out == 0) {
      bench_sink ^= outbuf[0];
      strm.next_out = outbuf;
      strm.avail_out = sizeof(outbuf);
    }
  }

  if (ret != LZMA_STREAM_END)
    die("lzma_code failed");
  bench_sink ^= (uint64_t)strm.total_out;
  lzma_end(&strm);
}

int main(int argc, char **argv) {
  uint8_t *input = NULL;
  uint8_t *output = NULL;
  size_t input_len = 0;
  size_t output_len = 0;

  if (argc < 3)
    die("usage: bench_helper <encode-memory|encode-file|decode-memory> <input> [output]");

  slurp(argv[2], &input, &input_len);

  if (strcmp(argv[1], "encode-memory") == 0) {
    encode_to_memory(input, input_len, &output, &output_len);
  } else if (strcmp(argv[1], "encode-file") == 0) {
    if (argc != 4)
      die("encode-file requires an output path");
    encode_to_memory(input, input_len, &output, &output_len);
    emit_file(argv[3], output, output_len);
  } else if (strcmp(argv[1], "decode-memory") == 0) {
    decode_to_memory(input, input_len);
  } else {
    die("unknown mode");
  }

  free(output);
  free(input);
  return 0;
}
EOF

cc -O2 \
  -I"$safe_dir/include" \
  "$tmpdir/bench_helper.c" \
  -L"$safe_runlib" \
  -Wl,-rpath,"$safe_runlib" \
  -llzma \
  -lpthread \
  -o "$tmpdir/bench_helper"

python3 - "$tmpdir" "$size_mib" <<'PY'
from pathlib import Path
import random
import sys

root = Path(sys.argv[1])
size_mib = int(sys.argv[2])
size = size_mib * 1024 * 1024

text_seed = (
    "liblzma benchmark corpus\n"
    "The quick brown fox jumps over the lazy dog.\n"
    "Rust hardening should preserve compatibility before chasing speed.\n"
)
text = (text_seed.encode("utf-8") * ((size // len(text_seed)) + 1))[:size]
root.joinpath("text.txt").write_bytes(text)

rng = random.Random(3094)
root.joinpath("random.bin").write_bytes(rng.randbytes(size))
PY

LD_LIBRARY_PATH="$ref_runlib" "$tmpdir/bench_helper" encode-file \
  "$tmpdir/text.txt" "$tmpdir/text.xz"
LD_LIBRARY_PATH="$ref_runlib" "$tmpdir/bench_helper" encode-file \
  "$tmpdir/random.bin" "$tmpdir/random.xz"

python3 - \
  "$tmpdir/bench_helper" \
  "$safe_runlib" \
  "$ref_runlib" \
  "$iterations" \
  "$warn_ratio" \
  "$encode_text_warn_ratio" \
  "$encode_random_warn_ratio" \
  "$decode_text_warn_ratio" \
  "$decode_random_warn_ratio" \
  "$tmpdir/text.txt" \
  "$tmpdir/random.bin" \
  "$tmpdir/text.xz" \
  "$tmpdir/random.xz" <<'PY'
from pathlib import Path
import os
import statistics
import subprocess
import sys
import time

bench = Path(sys.argv[1])
safe_runlib = sys.argv[2]
ref_runlib = sys.argv[3]
iterations = int(sys.argv[4])
uniform_warn_ratio = float(sys.argv[5]) if sys.argv[5] else None
warn_ratios = {
    "encode-text": float(sys.argv[6]),
    "encode-random": float(sys.argv[7]),
    "decode-text": float(sys.argv[8]),
    "decode-random": float(sys.argv[9]),
}

workloads = [
    ("encode-text", "encode-memory", Path(sys.argv[10]), Path(sys.argv[10]).stat().st_size),
    ("encode-random", "encode-memory", Path(sys.argv[11]), Path(sys.argv[11]).stat().st_size),
    ("decode-text", "decode-memory", Path(sys.argv[12]), Path(sys.argv[10]).stat().st_size),
    ("decode-random", "decode-memory", Path(sys.argv[13]), Path(sys.argv[11]).stat().st_size),
]

libraries = [
    ("reference", ref_runlib),
    ("safe", safe_runlib),
]

results = {}

for label, libdir in libraries:
    env = os.environ.copy()
    env["LD_LIBRARY_PATH"] = libdir
    for workload, mode, path, bytes_processed in workloads:
        samples = []
        subprocess.run(
            [str(bench), mode, str(path)],
            check=True,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            env=env,
        )
        for _ in range(iterations):
            started = time.perf_counter()
            subprocess.run(
                [str(bench), mode, str(path)],
                check=True,
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
                env=env,
            )
            samples.append(time.perf_counter() - started)
        median = statistics.median(samples)
        mib_per_s = (bytes_processed / (1024 * 1024)) / median if median > 0 else float("inf")
        results[(label, workload)] = (median, mib_per_s)

print("workload\tref_s\tsafe_s\tsafe/ref\tmin_safe/ref\tref_MiB_s\tsafe_MiB_s")
regressions = []
for workload, _, _, _ in workloads:
    ref_s, ref_mib = results[("reference", workload)]
    safe_s, safe_mib = results[("safe", workload)]
    ratio = ref_s / safe_s if safe_s > 0 else 0.0
    warn_ratio = uniform_warn_ratio if uniform_warn_ratio is not None else warn_ratios[workload]
    print(
        f"{workload}\t{ref_s:.6f}\t{safe_s:.6f}\t{ratio:.3f}\t{warn_ratio:.3f}\t{ref_mib:.2f}\t{safe_mib:.2f}"
    )
    if ratio < warn_ratio:
        regressions.append((workload, ratio, warn_ratio))

if regressions:
    print("\nsignoff benchmark floor crossed:")
    for workload, ratio, warn_ratio in regressions:
        print(
            f"  {workload}: safe throughput is {ratio:.3f}x reference"
            f" (minimum {warn_ratio:.3f}x)"
        )
    raise SystemExit(1)
PY
