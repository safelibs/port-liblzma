#!/usr/bin/env bash
set -euo pipefail

script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
repo_root=$(cd "$script_dir/../.." && pwd)
src="$repo_root/build/config.h"
dest="$repo_root/safe/tests/generated/config.h"

mkdir -p "$(dirname "$dest")"

awk '
BEGIN {
  phase01["HAVE_DECODERS"] = 1
  phase01["HAVE_ENCODERS"] = 1
  phase01["HAVE_DECODER_ARM"] = 1
  phase01["HAVE_DECODER_ARM64"] = 1
  phase01["HAVE_DECODER_ARMTHUMB"] = 1
  phase01["HAVE_DECODER_DELTA"] = 1
  phase01["HAVE_DECODER_IA64"] = 1
  phase01["HAVE_DECODER_LZMA1"] = 1
  phase01["HAVE_DECODER_LZMA2"] = 1
  phase01["HAVE_DECODER_POWERPC"] = 1
  phase01["HAVE_DECODER_SPARC"] = 1
  phase01["HAVE_DECODER_X86"] = 1
  phase01["HAVE_ENCODER_ARM"] = 1
  phase01["HAVE_ENCODER_ARM64"] = 1
  phase01["HAVE_ENCODER_ARMTHUMB"] = 1
  phase01["HAVE_ENCODER_DELTA"] = 1
  phase01["HAVE_ENCODER_IA64"] = 1
  phase01["HAVE_ENCODER_LZMA1"] = 1
  phase01["HAVE_ENCODER_LZMA2"] = 1
  phase01["HAVE_ENCODER_POWERPC"] = 1
  phase01["HAVE_ENCODER_SPARC"] = 1
  phase01["HAVE_ENCODER_X86"] = 1
  phase01["HAVE_LZIP_DECODER"] = 1
  phase01["HAVE_MF_BT2"] = 1
  phase01["HAVE_MF_BT3"] = 1
  phase01["HAVE_MF_BT4"] = 1
  phase01["HAVE_MF_HC3"] = 1
  phase01["HAVE_MF_HC4"] = 1
}
/^#define / {
  macro = $2
  if (macro in phase01) {
    print "/* #undef " macro " */"
    next
  }
}
{ print }
END {
  print ""
  print "/* Phase 02 foundation: keep checksum and threading probes enabled. */"
  print "/* Encoder/decoder feature macros stay undefined until those paths are implemented. */"
}
' "$src" > "$dest"
