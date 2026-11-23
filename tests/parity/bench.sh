#!/bin/sh
# Wall time of libultrahdr + libjpeg-turbo and of examples/bench.rs over the same JPEG files, as
# /report/bench.md: tests/parity/bench.sh [in-dir] [uhdr-oracle] [bench-binary] [report-dir].
# Information only (no speed bar). The C++ oracle (tests/parity/uhdr_oracle.cpp --no-raw) probes
# each file with libultrahdr and decodes the base image and the gain map with libjpeg-turbo, one
# thread, output of the pixels switched off; one run, process start-up included. The Rust bench
# example reads and decodes the same files with one thread and with the default threads, the
# files already in memory, best of three rounds. Files that are not JPEG (HEIF, AVIF: no pixel
# decode yet) are left out on both sides.
set -eu
in=${1:-/work/in}
uhdr=${2:-/opt/oracle/bin/uhdr_oracle}
bench=${3:-target/release/examples/bench}
out=${4:-/report}
mkdir -p "$out" /work/jpg
for f in "$in"/*.jpg; do ln -sf "$(readlink "$f" || echo "$f")" "/work/jpg/$(basename "$f")"; done

"$bench" /work/jpg > "$out/rust.md"
cat "$out/rust.md"

t0=$(date +%s%N)
"$uhdr" /work/jpg /work/c-out --no-raw
t1=$(date +%s%N)
c_ms=$(( (t1 - t0) / 1000000 ))
rm -rf /work/c-out /work/jpg

# the last row of rust.md: | files | MB | decoded | read ms | decode ms (1 thread) | decode ms (N threads) | N |
row=$(sed -n '$p' "$out/rust.md")
col() { echo "$row" | awk -F'|' -v n="$1" '{ gsub(/ /, "", $n); print $n }'; }
files=$(col 2); mb=$(col 3); decoded=$(col 4); read_ms=$(col 5); one_ms=$(col 6); many_ms=$(col 7); threads=$(col 8)
rust_one=$(awk -v a="$read_ms" -v b="$one_ms" 'BEGIN { printf "%.1f", a + b }')
ratio=$(awk -v a="$c_ms" -v b="$rust_one" 'BEGIN { if (b > 0) printf "%.2f", b / a; else print "n/a" }')

{
  echo "## libultrahdr 2.0.2 + libjpeg-turbo 3.2.0 and gainmap-rs, same files (information only)"
  echo
  echo "$files JPEG files, $mb MB, $decoded decoded. gainmap-rs: files in memory, best of 3 rounds. C++ oracle: one run, one thread, process start-up included."
  echo
  echo "| | wall ms |"
  echo "|---|---|"
  echo "| libultrahdr probe + libjpeg-turbo decode of base and gain map (1 thread) | $c_ms |"
  echo "| gainmap-rs \`read\` + \`decode\`, 1 thread | $rust_one |"
  echo "| gainmap-rs \`decode\`, $threads threads | $many_ms (decode only; \`read\` $read_ms) |"
  echo
  echo "gainmap-rs 1 thread / C++: ${ratio}x."
} > "$out/bench.md"
cat "$out/bench.md"
