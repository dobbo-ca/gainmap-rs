#!/bin/sh
# The parity run (docs/parity.md): tests/parity/run.sh <corpus> <oracle-binary> <report-dir>
#
# <corpus> is what tests/parity/fetch.sh made; <oracle-binary> is the built examples/oracle.rs.
# The C tools come from the Dockerfile stage oracle-tools: UHDR_ORACLE (default
# /opt/oracle/bin/uhdr_oracle, libultrahdr + libjpeg-turbo), AVIF_ORACLE (default
# /opt/oracle/bin/avif_oracle, libavif), EXIFTOOL (default /opt/exiftool/exiftool).
#
# Per input file, to bound the disk use: the C++ oracle and the Rust oracle write into temp dirs,
# the .txt files are compared with diff (the first line is only "ok or not": the error classes of
# the two sides differ by design) and the raw files with cmp, then the dirs are deleted. JPEG
# inputs also run through `oracle gen` mutations and `oracle uhdr`/`oracle xmp` (no crash, no
# hang; an error is fine). After the loop ExifTool is compared with `oracle xmp`.
# Works in /work (deleted at the end). Writes <report-dir>/summary.md, the lists of differences,
# and <report-dir>/status (0 pass, 1 fail). Exits 0, or 2 when a tool is missing.
# PARITY_EVERY_N=<n> keeps every n-th input only.
set -u
corpus=${1:?usage: run.sh <corpus> <oracle-binary> <report-dir>}
oracle=${2:?usage: run.sh <corpus> <oracle-binary> <report-dir>}
report=${3:?usage: run.sh <corpus> <oracle-binary> <report-dir>}
here=$(cd "$(dirname "$0")" && pwd)
uhdr=${UHDR_ORACLE:-/opt/oracle/bin/uhdr_oracle}
avif=${AVIF_ORACLE:-/opt/oracle/bin/avif_oracle}
exiftool=${EXIFTOOL:-/opt/exiftool/exiftool}

missing() { echo "run.sh: missing $1" >&2; exit 2; }
for c in perl python3 timeout awk cmp diff; do command -v "$c" >/dev/null 2>&1 || missing "$c"; done
[ -f "$exiftool" ] || missing "$exiftool"
[ -x "$oracle" ] || missing "$oracle"
[ -x "$uhdr" ] || missing "$uhdr"
[ -x "$avif" ] || missing "$avif"

work=/work
rm -rf "$work"
mkdir -p "$work/gm" "$report/diffs" "$report/crashers"
status=0
: > "$work/results.tsv"
: > "$report/rust-status.tsv"
: > "$report/failures.tsv"

every=${PARITY_EVERY_N:-1}
n=$(sh "$here/inputs.sh" "$corpus" "$work/in" "$every") || { echo "run.sh: inputs.sh failed" >&2; status=1; }
case $n in ''|*[!0-9]*) n=0 ;; esac
echo "inputs: $n"
# An empty or partial corpus must not pass: corpus.tsv lists PARITY_MIN_INPUTS files (divided
# by every-n).
min=$((${PARITY_MIN_INPUTS:-47} / every))
if [ "$n" -lt "$min" ]; then echo "run.sh: $n inputs, want at least $min" >&2; status=1; fi

fail() { printf '%s\t%s\t%s\n' "$1" "$2" "$3" >> "$report/failures.tsv"; status=1; }

# The text with its first line reduced to "status ok" or "status err".
norm_txt() { awk 'NR == 1 { print ($0 == "status ok") ? "status ok" : "status err"; next } { print }' "$1"; }

# compare_one <file> <channel> <suffix> <text|raw> <c-dir> <rust-dir>
compare_one() {
  f=$1; ch=$2; sfx=$3; kind=$4; c="$5/$f.$sfx"; r="$6/$f.$sfx"
  if [ -f "$c" ] && [ -f "$r" ]; then
    if [ "$kind" = text ]; then
      norm_txt "$c" > "$work/n1"; norm_txt "$r" > "$work/n2"
      if cmp -s "$work/n1" "$work/n2"; then res=equal; else
        res=differ; diff -u "$work/n1" "$work/n2" > "$report/diffs/$f.$ch.diff"
      fi
    elif cmp -s "$c" "$r"; then res=equal
    else
      res=differ; cmp "$c" "$r" > "$report/diffs/$f.$ch.cmp" 2>&1
    fi
  elif [ -f "$c" ]; then res=only-c
  elif [ -f "$r" ]; then res=only-rust
  else return 0
  fi
  printf '%s\t%s\t%s\n' "$f" "$ch" "$res" >> "$work/results.tsv"
}

sweep_total=0
for f in $(ls "$work/in"); do
  one="$work/one"; oc="$work/oc"; or="$work/or"
  rm -rf "$one" "$oc" "$or"; mkdir -p "$one"
  ln -s "$(readlink "$work/in/$f")" "$one/$f"

  # ---- libultrahdr + libjpeg-turbo against gainmap (every file: both must refuse a non-JPEG)
  timeout 900 "$uhdr" "$one" "$oc" || fail "$f" uhdr-cpp "exit $?"
  timeout 900 "$oracle" uhdr "$one" "$or" || fail "$f" uhdr-rust "exit $?"
  compare_one "$f" uhdr-txt txt text "$oc" "$or"
  compare_one "$f" uhdr-base-jpg base.jpg raw "$oc" "$or"
  compare_one "$f" uhdr-gm-jpg gm.jpg raw "$oc" "$or"
  compare_one "$f" uhdr-base-rgba base.rgba raw "$oc" "$or"
  compare_one "$f" uhdr-gm-raw gm.raw raw "$oc" "$or"
  printf '%s\t%s\n' "$f" "$(head -n 1 "$or/$f.txt" 2>/dev/null | cut -d' ' -f2)" >> "$report/rust-status.tsv"
  [ -f "$or/$f.gm.jpg" ] && cp "$or/$f.gm.jpg" "$work/gm/$f.gm.jpg"

  # ---- libavif against gainmap (AVIF and HEIC inputs)
  case $f in
    *.avif|*.avifs|*.heic|*.heif)
      rm -rf "$oc" "$or"
      timeout 900 "$avif" "$one" "$oc" || fail "$f" avif-c "exit $?"
      timeout 900 "$oracle" avif "$one" "$or" || fail "$f" avif-rust "exit $?"
      compare_one "$f" avif txt text "$oc" "$or"
      ;;
  esac

  # ---- no crash, no hang on mutated files (the Rust oracle only; errors are fine)
  case $f in
    *.jpg|*.jpeg)
      rm -rf "$work/gen" "$work/gen-out"
      "$oracle" gen "$one" "$work/gen" || fail "$f" gen "exit $?"
      ng=$(ls "$work/gen" 2>/dev/null | wc -l | tr -d ' ')
      sweep_total=$((sweep_total + ng))
      if [ "$ng" -gt 0 ]; then
        if ! timeout 1800 "$oracle" uhdr "$work/gen" "$work/gen-out" --no-raw \
           || ! timeout 900 "$oracle" xmp "$work/gen" /dev/null; then
          # Find the file: run them one by one and keep the first that fails.
          for g in "$work/gen"/*; do
            rm -rf "$work/g1" "$work/g1o"; mkdir "$work/g1"; ln -s "$g" "$work/g1/"
            if ! timeout 300 "$oracle" uhdr "$work/g1" "$work/g1o" --no-raw \
               || ! timeout 300 "$oracle" xmp "$work/g1" /dev/null; then
              cp "$g" "$report/crashers/"
              fail "$(basename "$g")" crash "mutation of $f"
              break
            fi
          done
          [ -z "$(ls "$report/crashers")" ] && fail "$f" crash "failed as a batch, no single file reproduces it"
        fi
      fi
      rm -rf "$work/gen" "$work/gen-out" "$work/g1" "$work/g1o"
      ;;
  esac
  rm -rf "$one" "$oc" "$or"
done

# ---- ExifTool 13.64 against `oracle xmp`: the primary images (the inputs) and the gain-map JPEGs
# that `oracle uhdr` wrote. -j JSON, -n no print conversion, -G1 family-1 group names, -a
# duplicates, -b binary values, -ext '*' every extension.
"$oracle" xmp "$work/in" "$work/xmp.jsonl" || fail all xmp-oracle "exit $?"
perl "$exiftool" -j -n -G1 -a -b -ext '*' -XMP-hdrgm:all -XMP-HDRGainMap:all "$work/in" "$work/gm" \
  > "$work/exiftool.json" 2> "$report/exiftool.stderr"
python3 -I "$here/compare.py" "$work/exiftool.json" "$work/xmp.jsonl" "$here/known-differences.tsv" "$report" || status=1

# ---- the table
awk -F'\t' -v known="$here/known-differences.tsv" -v diffs="$report/differences.tsv" -v ndiff="$work/ndiff" '
BEGIN {
  while ((getline line < known) > 0) {
    if (line ~ /^#/ || line == "") continue
    split(line, p, "\t"); k[p[1] SUBSEP p[2]] = 1
  }
  print "file\tchannel\tresult" > diffs
}
{
  f = $1; ch = $2; res = $3
  inputs[ch]++
  if (res != "only-rust") okc[ch]++
  if (res == "equal") { eq[ch]++; next }
  base = ch; sub(/-.*/, "", base)
  if ((f SUBSEP ch) in k) { kn[ch]++; used[f SUBSEP ch] = 1 }
  else if ((f SUBSEP base) in k) { kn[ch]++; used[f SUBSEP base] = 1 }
  else { df[ch]++; nd++; print f "\t" ch "\t" res > diffs }
}
END {
  print "| channel | inputs | ok | equal | differ | known |"
  print "|---|---:|---:|---:|---:|---:|"
  n = split("uhdr-txt uhdr-base-jpg uhdr-gm-jpg uhdr-base-rgba uhdr-gm-raw avif", chs, " ")
  for (i = 1; i <= n; i++) { c = chs[i]; printf "| %s | %d | %d | %d | %d | %d |\n", c, inputs[c], okc[c], eq[c], df[c], kn[c] }
  unused = 0
  for (key in k) if (!(key in used)) { split(key, kk, SUBSEP); if (kk[2] != "xmp") unused++ }
  print ""
  print "Known differences listed but unused (xmp excluded): " unused
  print nd + 0 > ndiff
}' "$work/results.tsv" > "$work/table.md"
ndiff=$(cat "$work/ndiff" 2>/dev/null || echo 1)
[ "$ndiff" -eq 0 ] || status=1

# Guards against a run that compared nothing: libultrahdr must have read some files, and the
# Rust side must have agreed on at least that many txt files.
uhdr_ok=$(awk -F'\t' '$2 == "uhdr-txt" && $3 == "equal"' "$work/results.tsv" | wc -l | tr -d ' ')
if [ "$uhdr_ok" -lt $((${PARITY_MIN_UHDR_OK:-15} / every)) ]; then
  echo "run.sh: only $uhdr_ok equal uhdr-txt files" >&2; status=1
fi
avif_eq=$(awk -F'\t' '$2 == "avif" && $3 == "equal"' "$work/results.tsv" | wc -l | tr -d ' ')
if [ "$avif_eq" -lt $((${PARITY_MIN_AVIF_OK:-10} / every)) ]; then
  echo "run.sh: only $avif_eq equal avif files" >&2; status=1
fi
[ -s "$report/failures.tsv" ] && status=1

{
  echo "# gainmap-rs parity run"
  echo
  if [ "$status" -eq 0 ]; then echo "Result: **pass**"; else echo "Result: **FAIL**"; fi
  echo
  cat /opt/oracle/versions.txt 2>/dev/null
  echo "ExifTool $(perl "$exiftool" -ver)"
  echo
  echo "Inputs: $n (corpus.tsv, docs/corpus.md)"
  echo
  echo "## libultrahdr 2.0.2 + libjpeg-turbo 3.2.0, and libavif 1.4.2"
  echo
  echo '"ok": the C tool produced the channel. "equal": same bytes (txt: same text, the error class of a refusal is ignored). "known": a difference listed in known-differences.tsv.'
  echo
  cat "$work/table.md"
  echo
  echo "Rust status of each input (docs/parity.md lists the classes):"
  echo
  cut -f2 "$report/rust-status.tsv" | sort | uniq -c | sort -rn | awk '{ printf "- %s: %s\n", $2, $1 }'
  echo
  cat "$report/xmp.md" 2>/dev/null
  echo
  echo "## No crash on mutations"
  echo
  echo "Mutated files run through \`oracle uhdr\` and \`oracle xmp\`: $sweep_total. Failures (panic, abort, hang, tool error): $(wc -l < "$report/failures.tsv" | tr -d ' ')."
  if [ -s "$report/failures.tsv" ]; then
    echo
    sed 's/^/- /' "$report/failures.tsv" | head -n 30
  fi
  if [ "$ndiff" -ne 0 ]; then
    echo
    echo "Differences (all in differences.tsv, details in diffs/):"
    echo
    sed 1d "$report/differences.tsv" | head -n 30 | sed 's/^/- /'
  fi
} > "$report/summary.md"
echo "$status" > "$report/status"
cat "$report/summary.md"

rm -rf "$work"
exit 0
