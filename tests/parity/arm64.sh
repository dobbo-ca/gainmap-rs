#!/bin/sh
# The x86_64 and the aarch64 (qemu-user) builds of examples/oracle.rs give byte-identical output:
# tests/parity/arm64.sh <corpus> <x86-oracle> <aarch64-oracle-command> [every-n]
#
# <aarch64-oracle-command> is the whole command that starts the aarch64 binary (for example
# "qemu-aarch64 -L /usr/aarch64-linux-gnu target/aarch64-unknown-linux-gnu/release/examples/oracle").
# Every n-th input (default 4) goes through `oracle uhdr` (text, coded bytes and pixels), and
# `oracle avif` and `oracle xmp` for the text outputs; each output dir is compared with diff -r
# and deleted at once, so the disk use stays at one file's output. Exits 1 on any difference.
set -eu
corpus=${1:?usage: arm64.sh <corpus> <x86-oracle> <aarch64-oracle-command> [every-n]}
x=${2:?usage: arm64.sh <corpus> <x86-oracle> <aarch64-oracle-command> [every-n]}
a=${3:?usage: arm64.sh <corpus> <x86-oracle> <aarch64-oracle-command> [every-n]}
every=${4:-4}
here=$(cd "$(dirname "$0")" && pwd)
work=/work
rm -rf "$work" /out
mkdir -p "$work" /out
n=$(sh "$here/inputs.sh" "$corpus" "$work/in" "$every")
echo "$n inputs (every $every-th)"
[ "$n" -gt 0 ]
bad=0
for f in $(ls "$work/in"); do
  rm -rf "$work/one" /out/*
  mkdir "$work/one"
  ln -s "$(readlink "$work/in/$f")" "$work/one/$f"
  for mode in uhdr avif; do
    $x $mode "$work/one" /out/x-$mode
    $a $mode "$work/one" /out/a-$mode
    diff -r /out/x-$mode /out/a-$mode > /dev/null || { echo "DIFFERENT: $mode $f" >&2; bad=$((bad + 1)); }
  done
  $x xmp "$work/one" /out/x.jsonl
  $a xmp "$work/one" /out/a.jsonl
  cmp -s /out/x.jsonl /out/a.jsonl || { echo "DIFFERENT: xmp $f" >&2; bad=$((bad + 1)); }
  echo "$f: $(ls /out/x-uhdr | wc -l | tr -d ' ') uhdr files"
done
rm -rf /out "$work"
echo "$n inputs, $bad differences"
[ "$bad" -eq 0 ]
