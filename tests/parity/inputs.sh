#!/bin/sh
# Makes the flat parity input dir: tests/parity/inputs.sh <corpus> <out-dir> [every-n]
#
# <corpus> is what tests/parity/fetch.sh made. <out-dir> gets one symlink per corpus file (the
# oracles follow symlinks), with the corpus name. The list is sorted; with every-n only every
# n-th file is kept (the 1st, n+1-th, ...). Prints the number of inputs.
set -eu
corpus=$(cd "${1:?usage: inputs.sh <corpus> <out-dir> [every-n]}" && pwd)
out=${2:?usage: inputs.sh <corpus> <out-dir> [every-n]}
every=${3:-1}
mkdir -p "$out"
(cd "$corpus/files" && ls | grep -v -e '\.ok$' -e '\.part$' | LC_ALL=C sort) | awk -v n="$every" '(NR - 1) % n == 0' | {
  count=0
  while IFS= read -r name; do
    ln -s "$corpus/files/$name" "$out/$name"
    count=$((count + 1))
  done
  echo "$count"
}
