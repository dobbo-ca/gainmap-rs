#!/bin/sh
# The parity run (docs/parity.md): tests/parity/run.sh <corpus> <oracle-binary> <report-dir>
#
# <corpus> is what tests/parity/fetch.sh made; <oracle-binary> is the built examples/oracle.rs.
# The C tools come from the Dockerfile stage oracle-tools: UHDR_ORACLE (default
# /opt/oracle/bin/uhdr_oracle, libultrahdr + libjpeg-turbo), AVIF_ORACLE (default
# /opt/oracle/bin/avif_oracle, libavif + dav1d), HEIF_ORACLE (default /opt/oracle/bin/heif_oracle,
# libheif + libde265), EXIFTOOL (default /opt/exiftool/exiftool).
#
# Per input file, to bound the disk use: the C++ oracle and the Rust oracle write into temp dirs,
# the .txt files are compared with diff (uhdr: the first line is only "ok or not", the error
# classes of the two sides differ by design; avif: the outcomes must be the same, see norm_avif)
# and the raw files with cmp, then the dirs are deleted. JPEG
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
heif=${HEIF_ORACLE:-/opt/oracle/bin/heif_oracle}
exiftool=${EXIFTOOL:-/opt/exiftool/exiftool}

missing() { echo "run.sh: missing $1" >&2; exit 2; }
for c in perl python3 timeout awk cmp diff; do command -v "$c" >/dev/null 2>&1 || missing "$c"; done
[ -f "$exiftool" ] || missing "$exiftool"
[ -x "$oracle" ] || missing "$oracle"
[ -x "$uhdr" ] || missing "$uhdr"
[ -x "$avif" ] || missing "$avif"
[ -x "$heif" ] || missing "$heif"

work=/work
rm -rf "$work"
mkdir -p "$work/gm" "$report/diffs" "$report/crashers"
status=0
: > "$work/results.tsv"
: > "$report/rust-status.tsv"
: > "$report/failures.tsv"
: > "$report/exiftool.stderr"

every=${PARITY_EVERY_N:-1}
n=$(INPUTS_GEN=1 sh "$here/inputs.sh" "$corpus" "$work/in" "$every") || { echo "run.sh: inputs.sh failed" >&2; status=1; }
case $n in ''|*[!0-9]*) n=0 ;; esac
echo "inputs: $n"
# An empty or partial corpus must not pass: corpus.tsv and gen.sh list PARITY_MIN_INPUTS files (divided
# by every-n).
min=$((${PARITY_MIN_INPUTS:-77} / every))
if [ "$n" -lt "$min" ]; then echo "run.sh: $n inputs, want at least $min" >&2; status=1; fi

fail() { printf '%s\t%s\t%s\n' "$1" "$2" "$3" >> "$report/failures.tsv"; status=1; }

# The text for the uhdr-txt channel: the first line reduced to "status ok" or "status err" (the
# error classes of the two sides differ by design), the size and channel lines (lines 2 to 6)
# left out because the uhdr-dims channel compares them, then the metadata lines.
norm_uhdr() { awk 'NR == 1 { print ($0 == "status ok") ? "status ok" : "status err"; next } NR > 6 { print }' "$1"; }

# The size and channel lines of a uhdr text (lines 2 to 6).
norm_dims() { awk 'NR >= 2 && NR <= 6' "$1"; }

# The text of the avif channel: the first line says what happened, and only equal outcomes are
# equal. libavif: ok, no-gainmap (parsed, gain map ignored), error. Rust: ok, no-metadata (a gain
# map kept without usable metadata: the same outcome as libavif ignoring it), no-gain-map (the
# gain map was dropped) and any error class (the read failed). A dropped gain map or a failed
# read never equals a libavif outcome. <file> <c|rust>
norm_avif() {
  awk -v side="$2" 'NR == 1 {
    s = $2
    if (side == "rust") {
      if (s == "no-metadata") s = "no-gainmap"
      else if (s == "no-gain-map") s = "dropped"
      else if (s != "ok") s = "rust-" s
    }
    print "status " s; next
  } { print }' "$1"
}

# compare_one <file> <channel> <suffix> <txt|dims|avif|raw> <c-dir> <rust-dir>
# Appends "file<TAB>channel<TAB>result<TAB>cok<TAB>rust": cok is 1 when the C tool produced data
# for the channel (a text with "status ok", or a raw file). The guards below count only those
# rows, so files that both sides refused do not count as compared. dims is compared only when the
# C tool read the file. rust is the normalised Rust outcome of an avif row (empty for the others):
# a known difference of the avif channel covers only the outcome in known-differences.tsv.
compare_one() {
  f=$1; ch=$2; sfx=$3; kind=$4; c="$5/$f.$sfx"; r="$6/$f.$sfx"
  cok=0; rs=""
  if [ -f "$c" ]; then
    if [ "$kind" = raw ]; then cok=1; elif [ "$(head -n 1 "$c")" = "status ok" ]; then cok=1; fi
  fi
  if [ -f "$c" ] && [ -f "$r" ]; then
    case $kind in
      txt) norm_uhdr "$c" > "$work/n1"; norm_uhdr "$r" > "$work/n2" ;;
      dims) [ "$cok" = 1 ] || return 0; norm_dims "$c" > "$work/n1"; norm_dims "$r" > "$work/n2" ;;
      avif) norm_avif "$c" c > "$work/n1"; norm_avif "$r" rust > "$work/n2"; rs=$(sed -n '1s/^status //p' "$work/n2") ;;
    esac
    if [ "$kind" != raw ]; then
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
  printf '%s\t%s\t%s\t%s\t%s\n' "$f" "$ch" "$res" "$cok" "$rs" >> "$work/results.tsv"
}

sweep_total=0
for f in $(ls "$work/in"); do
  one="$work/one"; oc="$work/oc"; or="$work/or"; op="$work/op"
  rm -rf "$one" "$oc" "$or" "$op"; mkdir -p "$one"
  ln -s "$(readlink "$work/in/$f")" "$one/$f"

  # ---- libultrahdr + libjpeg-turbo against gainmap (every file: both must refuse a non-JPEG)
  timeout 900 "$uhdr" "$one" "$oc" || fail "$f" uhdr-cpp "exit $?"
  timeout 900 "$oracle" uhdr "$one" "$or" || fail "$f" uhdr-rust "exit $?"
  compare_one "$f" uhdr-txt txt txt "$oc" "$or"
  compare_one "$f" uhdr-dims txt dims "$oc" "$or"
  compare_one "$f" uhdr-base-jpg base.jpg raw "$oc" "$or"
  compare_one "$f" uhdr-gm-jpg gm.jpg raw "$oc" "$or"
  compare_one "$f" uhdr-base-rgba base.rgba raw "$oc" "$or"
  compare_one "$f" uhdr-gm-raw gm.raw raw "$oc" "$or"
  printf '%s\t%s\n' "$f" "$(head -n 1 "$or/$f.txt" 2>/dev/null | cut -d' ' -f2)" >> "$report/rust-status.tsv"
  [ -f "$or/$f.gm.jpg" ] && cp "$or/$f.gm.jpg" "$work/gm/$f.gm.jpg"

  # ---- fallback: where libultrahdr could not read the file, decode the JPEGs gainmap extracted
  # with libjpeg-turbo alone and compare with the Rust pixels.
  # Every JPEG of the corpus carries a gain map, so when libultrahdr refuses one, a missing Rust
  # file is a difference (never-drop rule), not a skip.
  if [ "$(head -n 1 "$oc/$f.txt" 2>/dev/null)" != "status ok" ]; then
    mkdir -p "$work/tj"
    for spec in base.jpg:base.rgba:turbo-base-rgba gm.jpg:gm.raw:turbo-gm-raw; do
      src=${spec%%:*}; rest=${spec#*:}; dst=${rest%%:*}; ch=${rest#*:}
      if [ ! -f "$or/$f.$src" ]; then
        case $f in
          *.jpg|*.jpeg) printf '%s\t%s\t%s\t%s\n' "$f" "$ch" missing-rust 0 >> "$work/results.tsv" ;;
        esac
        continue
      fi
      rm -f "$work/tj/$f.$dst"
      "$uhdr" decode "$or/$f.$src" "$work/tj/$f.$dst" 2>/dev/null
      compare_one "$f" "$ch" "$dst" raw "$work/tj" "$or"
    done
    rm -rf "$work/tj"
    # Where the base image starts and what size it has, without libultrahdr: the Rust base.jpg
    # must be the first bytes of the file, and the Rust base size must be the ImageWidth and
    # ImageHeight ExifTool reports for the file. Where the base image ends is not checked
    # independently (the pixels of turbo-base-rgba stop at the first EOI anyway).
    if [ -f "$or/$f.base.jpg" ]; then
      nb=$(wc -c < "$or/$f.base.jpg")
      if cmp -s -n "$nb" "$one/$f" "$or/$f.base.jpg"; then res=equal; else res=differ; fi
      printf '%s\t%s\t%s\t%s\n' "$f" base-prefix "$res" 1 >> "$work/results.tsv"
      w=$(perl "$exiftool" -s3 -n -ImageWidth "$one/$f" 2>> "$report/exiftool.stderr" | head -n 1)
      h=$(perl "$exiftool" -s3 -n -ImageHeight "$one/$f" 2>> "$report/exiftool.stderr" | head -n 1)
      mine=$(awk '$1 == "base.width" { w = $2 } $1 == "base.height" { h = $2 } END { print w "x" h }' "$or/$f.txt")
      if [ -n "$w" ] && [ "$mine" = "${w}x${h}" ]; then res=equal; else res=differ; fi
      printf '%s\t%s\t%s\t%s\n' "$f" base-size "$res" 1 >> "$work/results.tsv"
    fi
  fi

  # ---- ExifTool locates the second MPF image: its bytes must be the gain-map JPEG. No
  # MPImage2 after a successful ExifTool run (a file with no MPF) is n/a, not a difference. An
  # ExifTool that fails is a failure of the run and a difference, never n/a.
  if [ -f "$or/$f.gm.jpg" ]; then
    perl "$exiftool" -b -MPImage2 "$one/$f" > "$work/mp2" 2>> "$report/exiftool.stderr"
    et_exit=$?
    if [ "$et_exit" -ne 0 ]; then res=error; fail "$f" exiftool-mpimage2 "exit $et_exit"
    elif [ ! -s "$work/mp2" ]; then res=n/a
    elif cmp -s "$work/mp2" "$or/$f.gm.jpg"; then res=equal
    else res=differ; cmp "$work/mp2" "$or/$f.gm.jpg" > "$report/diffs/$f.exiftool-mpimage2.cmp" 2>&1
    fi
    printf '%s\t%s\t%s\t%s\n' "$f" exiftool-mpimage2 "$res" 1 >> "$work/results.tsv"
  fi

  # ---- libavif against gainmap (AVIF and HEIC inputs)
  case $f in
    *.avif|*.avifs|*.heic|*.heif)
      rm -rf "$oc" "$or" "$op"
      timeout 900 "$oracle" avif "$one" "$or" || fail "$f" avif-rust "exit $?"
      # The decoded planes. `oracle planes` also runs its own limit, thread and cancel checks on
      # every image it decodes: a failed check is a non-zero exit.
      timeout 900 "$oracle" planes "$one" "$op" || fail "$f" planes-rust "exit $?"
      # An image that `read` reported and `decode` failed on leaves <f>.<which>.err. Most have no
      # C oracle (libavif decodes no Adobe auxiliary gain map, libheif no tmap one), so nothing else
      # would notice: a gain map that gainmap keeps must decode.
      for e in "$op"/*.err; do
        [ -e "$e" ] || continue
        fail "$f" planes-rust-err "$(basename "$e"): $(head -n 1 "$e")"
      done
      # libavif refuses a HEIC (no 'avif' brand), so a HEIC has no libavif metadata channel. It is
      # gated by compare.py instead: Rust must report a gain map, and its XMP values and
      # sizes are compared with ExifTool. Its planes are compared with libheif, which decodes
      # the item ids that gainmap reports (`oracle planes` writes them to <f>.ids); for the Apple file
      # the ids are also chosen by libheif itself and compared (channel heifid).
      case $f in
        *.heic|*.heif)
          timeout 900 "$heif" "$one" "$oc" "$op" || fail "$f" heif-c "exit $?"
          for w in base gm; do
            compare_one "$f" "heifpix-$w-info" "$w.info" raw "$oc" "$op"
            compare_one "$f" "heifpix-$w-yuv" "$w.yuv" raw "$oc" "$op"
          done
          # The item choice, not taken from gainmap: libheif's primary item and Apple auxiliary
          # hdrgainmap image (<f>.heifids, written only when it finds the gain map that way) must
          # be the ids that gainmap reports.
          if [ -f "$oc/$f.heifids" ]; then
            res=equal
            while IFS= read -r l; do
              grep -qxF "$l" "$op/$f.ids" 2>/dev/null || res=differ
            done < "$oc/$f.heifids"
            printf '%s\t%s\t%s\t%s\t%s\n' "$f" heifid "$res" 1 "" >> "$work/results.tsv"
          fi
          ;;
        *)
          timeout 900 "$avif" "$one" "$oc" || fail "$f" avif-c "exit $?"
          compare_one "$f" avif txt avif "$oc" "$or"
          for w in base gm; do
            compare_one "$f" "avifpix-$w-info" "$w.info" raw "$oc" "$op"
            compare_one "$f" "avifpix-$w-yuv" "$w.yuv" raw "$oc" "$op"
          done
          ;;
      esac
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
        crashed=0
        if ! timeout 1800 "$oracle" uhdr "$work/gen" "$work/gen-out" --no-raw \
           || ! timeout 900 "$oracle" xmp "$work/gen" /dev/null; then
          # Find the file: run them one by one and keep the first that fails.
          for g in "$work/gen"/*; do
            rm -rf "$work/g1" "$work/g1o"; mkdir "$work/g1"; ln -s "$g" "$work/g1/"
            if ! timeout 300 "$oracle" uhdr "$work/g1" "$work/g1o" --no-raw \
               || ! timeout 300 "$oracle" xmp "$work/g1" /dev/null; then
              cp "$g" "$report/crashers/"
              fail "$(basename "$g")" crash "mutation of $f"
              crashed=1
              break
            fi
          done
          [ "$crashed" -eq 1 ] || fail "$f" crash "failed as a batch, no single file reproduces it"
        fi
      fi
      rm -rf "$work/gen" "$work/gen-out" "$work/g1" "$work/g1o"
      ;;
  esac
  rm -rf "$one" "$oc" "$or" "$op"
done

# ---- ExifTool 13.64 against `oracle xmp`: the primary images (the inputs) and the gain-map JPEGs
# that `oracle uhdr` wrote. -j JSON, -n no print conversion, -G1 family-1 group names, -a
# duplicates, -b binary values, -ext '*' every extension.
"$oracle" xmp "$work/in" "$work/xmp.jsonl" || fail all xmp-oracle "exit $?"
# The same run gives compare.py what it needs for the gain-map gates: the MPF image count, the
# GContainer directory (XMP-GContainer) and the image sizes (the HEIC check). A failing
# ExifTool fails the run: its checks must not silently disappear.
perl "$exiftool" -j -n -G1 -a -b -ext '*' -XMP-hdrgm:all -XMP-HDRGainMap:all -XMP-GContainer:all \
  -MPF:NumberOfImages -ImageWidth -ImageHeight "$work/in" "$work/gm" \
  > "$work/exiftool.json" 2>> "$report/exiftool.stderr" || fail all exiftool-json "exit $?"
python3 -I "$here/compare.py" "$work/exiftool.json" "$work/xmp.jsonl" "$here/known-differences.tsv" "$report" || status=1

# ---- the table
awk -F'\t' -v known="$here/known-differences.tsv" -v diffs="$report/differences.tsv" -v ndiff="$work/ndiff" '
BEGIN {
  while ((getline line < known) > 0) {
    if (line ~ /^#/ || line == "") continue
    split(line, p, "\t"); k[p[1] SUBSEP p[2]] = 1; want[p[1] SUBSEP p[2]] = p[4]
  }
  print "file\tchannel\tresult" > diffs
}
{
  f = $1; ch = $2; res = $3
  inputs[ch]++
  if (res == "n/a") { na[ch]++; next }
  if ($4 == 1) okc[ch]++
  if (res == "equal") { eq[ch]++; next }
  base = ch; sub(/-.*/, "", base)
  part = ch; sub(/-[^-]*$/, "", part)
  if ((f SUBSEP ch) in k && (ch != "avif" || want[f SUBSEP ch] == $5)) { kn[ch]++; used[f SUBSEP ch] = 1 }
  else if (index(ch, "-") && (f SUBSEP part) in k && (part !~ /^avifpix-/ || res == "only-rust")) { kn[ch]++; used[f SUBSEP part] = 1 }
  else if (index(ch, "-") && (f SUBSEP base) in k) { kn[ch]++; used[f SUBSEP base] = 1 }
  else { df[ch]++; nd++; print f "\t" ch "\t" res > diffs }
}
END {
  print "| channel | inputs | ok | equal | differ | known | n/a |"
  print "|---|---:|---:|---:|---:|---:|---:|"
  n = split("uhdr-txt uhdr-dims uhdr-base-jpg uhdr-gm-jpg uhdr-base-rgba uhdr-gm-raw avif turbo-base-rgba turbo-gm-raw base-prefix base-size exiftool-mpimage2 heifid heifpix-base-info heifpix-base-yuv heifpix-gm-info heifpix-gm-yuv avifpix-base-info avifpix-base-yuv avifpix-gm-info avifpix-gm-yuv", chs, " ")
  for (i = 1; i <= n; i++) { c = chs[i]; printf "| %s | %d | %d | %d | %d | %d | %d |\n", c, inputs[c], okc[c], eq[c], df[c], kn[c], na[c] }
  unused = 0
  for (key in k) if (!(key in used)) { split(key, kk, SUBSEP); if (kk[2] != "xmp") unused++ }
  print ""
  print "Known differences listed but unused (xmp excluded): " unused
  print nd + 0 > ndiff
}' "$work/results.tsv" > "$work/table.md"
ndiff=$(cat "$work/ndiff" 2>/dev/null || echo 1)
[ "$ndiff" -eq 0 ] || status=1

# Guards against a run that compared nothing: only files the C tool read (status ok) and the
# Rust side agreed on count, so files that both sides refused do not count.
uhdr_ok=$(awk -F'\t' '$2 == "uhdr-txt" && $3 == "equal" && $4 == 1' "$work/results.tsv" | wc -l | tr -d ' ')
if [ "$uhdr_ok" -lt $((${PARITY_MIN_UHDR_OK:-15} / every)) ]; then
  echo "run.sh: only $uhdr_ok equal uhdr-txt files" >&2; status=1
fi
avif_eq=$(awk -F'\t' '$2 == "avif" && $3 == "equal" && $4 == 1' "$work/results.tsv" | wc -l | tr -d ' ')
if [ "$avif_eq" -lt $((${PARITY_MIN_AVIF_OK:-26} / every)) ]; then
  echo "run.sh: only $avif_eq equal avif files" >&2; status=1
fi
# The same for the decoded planes: only files both tools decoded and the Rust planes equal count.
for spec in heifpix-base-yuv:PARITY_MIN_HEIFPIX_BASE:6 heifpix-gm-yuv:PARITY_MIN_HEIFPIX_GM:6 \
            avifpix-base-yuv:PARITY_MIN_AVIFPIX_BASE:38 avifpix-gm-yuv:PARITY_MIN_AVIFPIX_GM:26; do
  ch=${spec%%:*}; rest=${spec#*:}; var=${rest%%:*}; dflt=${rest#*:}
  eq=$(awk -F'\t' -v ch="$ch" '$2 == ch && $3 == "equal" && $4 == 1' "$work/results.tsv" | wc -l | tr -d ' ')
  eval "want=\${$var:-$dflt}"
  if [ "$eq" -lt $((want / every)) ]; then echo "run.sh: only $eq equal $ch files" >&2; status=1; fi
done
mp_eq=$(awk -F'\t' '$2 == "exiftool-mpimage2" && $3 == "equal"' "$work/results.tsv" | wc -l | tr -d ' ')
if [ "$mp_eq" -lt $((${PARITY_MIN_MPIMAGE2_OK:-10} / every)) ]; then
  echo "run.sh: only $mp_eq equal exiftool-mpimage2 files" >&2; status=1
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
  echo "Inputs: $n (corpus.tsv and the generated files, docs/corpus.md)"
  if [ -s "$corpus/gen.tsv" ]; then
    echo
    echo "Generated files (gen.sh; sha256, size, same bytes on two runs of the generator):"
    echo
    sed "s|^|- |; s|\t| |g" "$corpus/gen.tsv"
  fi
  echo
  echo "## libultrahdr 2.0.2 + libjpeg-turbo 3.2.0, and libavif 1.4.2"
  echo
  echo '"ok": the C tool produced the channel. "equal": same bytes (uhdr-txt: same text, the error class of a refusal is ignored; avif: the same outcome and text). "known": a difference listed in known-differences.tsv.'
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
