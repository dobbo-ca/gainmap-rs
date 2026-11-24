#!/bin/sh
# Makes the generated parity files (docs/corpus.md "Generated files"): tests/parity/gen.sh <corpus>
#
# Hand-built gain-map files for shapes the public corpus lacks, written to <corpus>/files/gen_*.avif and
# gen_*.heic with the stamp <name>.ok, and listed in <corpus>/gen.names (which tests/parity/inputs.sh
# reads, so they go through every parity channel like a fetched file). They are made in CI, never
# committed: the tools are the CI-only generators of .github/ci/build-gen-avif.sh and build-gen-heic.sh,
# the inputs are corpus files (the licences are in docs/corpus.md), and the settings are fixed
# (aom speed 6, one thread). Every file is made twice and the two outputs compared; <corpus>/gen.tsv
# lists "name, sha256, size, deterministic (yes or no)". A file pinned in tests/parity/gen-pins.tsv
# ("name<TAB>sha256") must have that sha256. Exits 1 when a generator fails or a pin differs.
set -eu
corpus=${1:?usage: gen.sh <corpus>}
here=$(cd "$(dirname "$0")" && pwd)
gen=/opt/gen
files="$corpus/files"
tmp=$(mktemp -d)
mkdir -p "$files" "$tmp/1" "$tmp/2"
rm -f "$files"/gen_*
: > "$corpus/gen.names"
: > "$corpus/gen.tsv"
failed=0
export LD_LIBRARY_PATH="$gen/lib"

jpg="$files/seine_sdr_gainmap_srgb.jpg"   # 400x300, 8-bit, with an ISO gain map (BSD-2-Clause, libavif v1.4.2)
hdr="$files/seine_hdr_srgb.avif"          # 400x300, 10-bit HDR (BSD-2-Clause, libavif v1.4.2)
p010="$gen/data/raw_p010_image.p010"      # 1280x720 P010 (libultrahdr tests/data)
for f in "$jpg" "$hdr"; do [ -f "$f" ] || { echo "gen.sh: missing input $f" >&2; exit 1; }; done

# The generators write to $OUT. enc: avifenc on the JPEG (it converts the JPEG's gain map to a tmap
# item); comb: avifgainmaputil combine <base> <alternate>; uhdr: the libultrahdr sample app, HEIC out.
enc() { "$gen/bin/avifenc" -s 6 -j 1 "$@" "$jpg" "$OUT"; }
# --ignore-profile: libavif cannot compute a gain map for an image with an ICC profile.
comb() { "$gen/bin/avifgainmaputil" combine -s 6 -j 1 --ignore-profile "$@" "$OUT"; }
# encd: avifenc on an Ultra HDR JPEG made by libultrahdr with a gain map of half the size ($tmp/ds.jpg). libavif writes a clap
# on a tmap image only when the gain map is not the size of the base (write.c, AVIF_RESULT_NOT_IMPLEMENTED otherwise).
encd() { "$gen/bin/avifenc" -s 6 -j 1 "$@" "$tmp/ds.jpg" "$OUT"; }
uhdr() { "$gen/bin/ultrahdr_app" -m 0 -p "$p010" -w 1280 -h 720 -a 0 "$@" -z "$OUT"; }

# make <name> <generator> <args...>: runs the generator twice, keeps the first output.
make() {
  name=$1; shift
  for i in 1 2; do
    OUT="$tmp/$i/$name"
    rm -f "$OUT"
    if ! "$@" > "$tmp/log" 2>&1 || [ ! -s "$OUT" ]; then
      tail -n 60 "$tmp/log" >&2
      echo "gen.sh: $name failed" >&2
      failed=$((failed + 1))
      return 0
    fi
  done
  # An AVIF whose encoder log has no gain-map line was written without one: a generator mistake, not a corpus file.
  case $name in *.avif) grep -q "^ \* Gain map " "$tmp/log" || { cat "$tmp/log" >&2; echo "gen.sh: $name has no gain map" >&2; failed=$((failed + 1)); return 0; } ;; esac
  if cmp -s "$tmp/1/$name" "$tmp/2/$name"; then det=yes; else det=no; fi
  sha=$(sha256sum "$tmp/1/$name" | cut -d' ' -f1)
  size=$(wc -c < "$tmp/1/$name" | tr -d ' ')
  pin=$(awk -F'\t' -v n="$name" '$1 == n { print $2 }' "$here/gen-pins.tsv")
  if [ -n "$pin" ] && [ "$pin" != "$sha" ]; then
    echo "gen.sh: $name has sha256 $sha, gen-pins.tsv says $pin" >&2
    failed=$((failed + 1))
    return 0
  fi
  mv "$tmp/1/$name" "$files/$name"
  touch "$files/$name.ok"
  echo "$name" >> "$corpus/gen.names"
  printf '%s\t%s\t%s\t%s\n' "$name" "$sha" "$size" "$det" >> "$corpus/gen.tsv"
  echo "generated $name sha256=$sha size=$size deterministic=$det"
}

# AVIF tmap with irot and imir on the base image and the gain map (same size as the base).
make gen_tmap_irot1.avif enc --irot 1
make gen_tmap_irot3_imir1.avif enc --irot 3 --imir 1
make gen_tmap_imir0.avif enc --imir 0
# Transform properties with a gain map of half the size: the JPEG is made by libultrahdr (the HEIC generator), then avifenc converts it.
if [ -x "$gen/bin/ultrahdr_app" ]; then
  OUT=$tmp/ds.jpg; uhdr -s 2 > "$tmp/log" 2>&1 || { cat "$tmp/log" >&2; echo "gen.sh: the half-size gain-map JPEG failed" >&2; exit 1; }
  make gen_tmap_ds.avif encd
  make gen_tmap_ds_clap.avif encd --crop 8,8,1264,704
  make gen_tmap_ds_irot2_clap.avif encd --irot 2 --crop 8,8,1264,704
  make gen_tmap_ds_irot1_imir0_clap.avif encd --irot 1 --imir 0 --crop 8,8,1264,704
fi
# Gain-map shape: pixel format, bit depth, size, grid (base 8-bit 4:4:4 or 4:2:0 from the JPEG, alternate the HDR AVIF).
make gen_gm_yuv400.avif comb --yuv-gain-map 400 "$jpg" "$hdr"
make gen_gm_depth10.avif comb --depth-gain-map 10 "$jpg" "$hdr"
make gen_gm_depth12_yuv400.avif comb --depth-gain-map 12 --yuv-gain-map 400 "$jpg" "$hdr"
make gen_gm_down2.avif comb --downscaling 2 "$jpg" "$hdr"
make gen_gm_down4_yuv420.avif comb --downscaling 4 --yuv-gain-map 420 "$jpg" "$hdr"
make gen_grid2x2.avif comb --grid 2x2 "$jpg" "$hdr"
make gen_grid2x2_gm_yuv400_depth10.avif comb --grid 2x2 --yuv-gain-map 400 --depth-gain-map 10 "$jpg" "$hdr"
# Base image: 10 and 12 bit, 4:2:0 and 4:2:2 (from the JPEG), and an HDR base (the 10-bit AVIF is the base).
make gen_base10_yuv420.avif comb --depth 10 --yuv 420 "$jpg" "$hdr"
make gen_base12_yuv422.avif comb --depth 12 --yuv 422 "$jpg" "$hdr"
make gen_base_hdr10.avif comb "$hdr" "$jpg"
# HEIC tmap from libultrahdr (HEVC base and gain map): the default (3-channel gain map), one channel, a downscaled gain map, PQ and BT.2100.
if [ -x "$gen/bin/ultrahdr_app" ]; then
  make gen_uhdr_p010.heic uhdr
  make gen_uhdr_p010_mono.heic uhdr -M 0
  make gen_uhdr_p010_down2.heic uhdr -s 2
  make gen_uhdr_p010_pq_bt2100.heic uhdr -t 2 -C 2
else
  echo "gen.sh: no ultrahdr_app, the HEIC files are skipped" >&2
fi

rm -rf "$tmp"
echo "gen: $(wc -l < "$corpus/gen.names" | tr -d ' ') files, $failed failed"
[ "$failed" -eq 0 ]
