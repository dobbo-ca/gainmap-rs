#!/bin/sh
# The x86_64 and the aarch64 builds of examples/oracle.rs give byte-identical output. The two
# builds run on two machines, so the x86_64 side writes a reference and the aarch64 side checks it:
# tests/parity/arm64.sh ref|arm <corpus> <oracle> <ref-dir> [every-n]
#
#   ref   <oracle> is the x86_64 binary; writes the checksums of its outputs to <ref-dir>
#   arm   <oracle> is the aarch64 binary; compares the checksums of its outputs with <ref-dir>
#
# Every n-th input (default 4) goes through `oracle uhdr` (text, coded bytes and pixels), `oracle
# planes` (the decoded HEIF and AVIF planes, and its limit, thread and cancel checks), and
# `oracle avif` and `oracle xmp` for the text outputs. The checksum of an output dir is the sha256
# of every file in it, by name, so equal checksums are what `diff -r` would call identical; each
# output dir is deleted at once, so the disk use stays at one file's output. `arm` exits 1 on any
# difference.
set -eu
usage='usage: arm64.sh ref|arm <corpus> <oracle> <ref-dir> [every-n]'
side=${1:?$usage}
corpus=${2:?$usage}
o=${3:?$usage}
ref=${4:?$usage}
every=${5:-4}
here=$(cd "$(dirname "$0")" && pwd)
work=/work
rm -rf "$work" /out
mkdir -p "$work" /out
[ "$side" = ref ] && mkdir -p "$ref"
n=$(sh "$here/inputs.sh" "$corpus" "$work/in" "$every")
echo "$n inputs (every $every-th)"
[ "$n" -gt 0 ]
bad=0
# The checksum of an output dir (find ... | sort, so the order does not depend on the machine).
sum() { (cd "$1" && find . -type f -exec sha256sum {} + | LC_ALL=C sort -k2); }
# check <name> <command...>: ref stores the command's output under <ref-dir>/<input>/<name>; arm compares it.
check() {
  name=$1; shift
  if [ "$side" = ref ]; then mkdir -p "$ref/$f"; "$@" > "$ref/$f/$name"
  else "$@" > /out.sum
    cmp -s /out.sum "$ref/$f/$name" || { echo "DIFFERENT: $name $f" >&2; bad=$((bad + 1)); }; fi
}
for f in $(ls "$work/in"); do
  rm -rf "$work/one" /out/*
  mkdir "$work/one"
  ln -s "$(readlink "$work/in/$f")" "$work/one/$f"
  for mode in uhdr avif planes; do
    $o $mode "$work/one" /out/$mode
    check $mode sum /out/$mode
  done
  $o xmp "$work/one" /out/xmp.jsonl
  check xmp sha256sum < /out/xmp.jsonl
  echo "$f: $(ls /out/uhdr | wc -l | tr -d ' ') uhdr files"
done
rm -rf /out /out.sum "$work"
echo "$n inputs, $bad differences"
[ "$bad" -eq 0 ]
