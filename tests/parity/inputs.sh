#!/bin/sh
# Makes the flat parity input dir: tests/parity/inputs.sh <corpus> <out-dir> [every-n]
#
# <corpus> is what tests/parity/fetch.sh made. <out-dir> gets one symlink per file listed in
# corpus.tsv and, when INPUTS_GEN is set, one per generated file listed in <corpus>/gen.names (tests/parity/gen.sh; the oracles follow
# symlinks), with the corpus name. The list is sorted; with every-n only every
# n-th file is kept (the 1st, n+1-th, ...). Prints the number of inputs.
set -eu
corpus=$(cd "${1:?usage: inputs.sh <corpus> <out-dir> [every-n]}" && pwd)
out=${2:?usage: inputs.sh <corpus> <out-dir> [every-n]}
every=${3:-1}
mkdir -p "$out"
here=$(cd "$(dirname "$0")" && pwd)
# Only the names listed in corpus.tsv, not whatever else the shared cache holds (other
# branches leave files there). A listed file that is not in the corpus is skipped; the minimum
# input count of run.sh catches that.
(
  grep -v '^#' "$here/corpus.tsv" | grep -v '^$' | cut -f1 | while IFS= read -r name; do
    [ -f "$corpus/files/$name" ] && [ -e "$corpus/files/$name.ok" ] && echo "$name"
  done
  # The generated files of the last gen.sh run, only with INPUTS_GEN set (run.sh ran gen.sh; the arm64 and bench stages did not).
  if [ -n "${INPUTS_GEN:-}" ] && [ -f "$corpus/gen.names" ]; then
    while IFS= read -r name; do
      [ -f "$corpus/files/$name" ] && [ -e "$corpus/files/$name.ok" ] && echo "$name"
    done < "$corpus/gen.names"
  fi
  true
) | LC_ALL=C sort | awk -v n="$every" '(NR - 1) % n == 0' | {
  count=0
  while IFS= read -r name; do
    ln -s "$corpus/files/$name" "$out/$name"
    count=$((count + 1))
  done
  echo "$count"
}
