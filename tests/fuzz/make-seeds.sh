#!/bin/sh
# Writes the fuzz seeds, byte by byte with printf (there are no binary fixtures in git):
# tests/fuzz/make-seeds.sh <out-dir> makes <out-dir>/<target>/ for every fuzz target. The
# Dockerfile fuzz stage runs it with /seeds; fuzz.yml hands those directories to libFuzzer.
#   read    Ultra HDR JPEGs (MPF, GContainer only, "image that follows", XMP Seq, ISO only,
#           truncated), a JPEG without a gain map, an AVIF with a tmap item (and one with an XMP
#           item too), a HEIC with an Apple auxiliary gain map and two XMP items
#   decode  [selector byte][file]: the same files, selector 0 (tiny max_alloc) and 3 (64 MiB)
#   iso     ISO 21496-1 payloads: one channel, three channels, common denominator, truncated
#   mpf     MPF payloads from the endian field: big and little endian, truncated
#   xmp     XMP packets: hdrgm attributes, hdrgm rdf:Seq, Apple HDRGainMap, garbage
#   scan    JPEG images: the primary, the gain map, truncated
# The image data of every JPEG is a 1x1 grey baseline image (one DC and one AC code).
set -eu
out=${1:?usage: make-seeds.sh <out-dir>}
t=$(mktemp -d)
trap 'rm -rf "$t"' EXIT
for d in read decode iso mpf xmp scan; do mkdir -p "$out/$d"; done

# bytes 255 216 writes the bytes with these decimal values.
bytes() { fmt=$(printf '\\%03o' "$@"); printf "$fmt"; }
be16() { bytes $(($1 / 256 % 256)) $(($1 % 256)); }
be32() { bytes $(($1 / 16777216 % 256)) $(($1 / 65536 % 256)) $(($1 / 256 % 256)) $(($1 % 256)); }
le16() { bytes $(($1 % 256)) $(($1 / 256 % 256)); }
le32() { bytes $(($1 % 256)) $(($1 / 256 % 256)) $(($1 / 65536 % 256)) $(($1 / 16777216 % 256)); }
size() { wc -c < "$1" | tr -d ' '; }
# rep <byte> <count>: the byte, count times.
rep() { i=0; while [ "$i" -lt "$2" ]; do bytes "$1"; i=$((i + 1)); done; }

# ---- ISO 21496-1 payloads (from minimum_version): the one-channel, the three-channel and the
# common-denominator form. Headrooms 0 and 3 (log2), gain 0 to 2.5 (5/2), gamma 1, offsets 1/64.
channel() { be32 0; be32 1; be32 5; be32 2; be32 1; be32 1; be32 1; be32 64; be32 1; be32 64; }
{ be16 0; be16 0; bytes 0; be32 0; be32 1; be32 3; be32 1; channel; } > "$t/iso1"
{ be16 0; be16 0; bytes 192; be32 0; be32 1; be32 3; be32 1; channel; channel; channel; } > "$t/iso3"
{ be16 0; be16 0; bytes 8; be32 64; be32 0; be32 192; be32 0; be32 160; be32 64; be32 1; be32 1; } > "$t/isoc"
# The version-only payload of a primary image.
{ be16 0; be16 0; } > "$t/isov"
cp "$t/iso1" "$out/iso/one-channel"
cp "$t/iso3" "$out/iso/three-channels"
cp "$t/isoc" "$out/iso/common-denominator"
cp "$t/isov" "$out/iso/version-only"
head -c 30 "$t/iso1" > "$out/iso/cut"

# ---- XMP packets.
ns_rdf='xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"'
ns_hdrgm='xmlns:hdrgm="http://ns.adobe.com/hdr-gain-map/1.0/"'
wrap() { printf '%s' '<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF '"$ns_rdf"'>'; cat; printf '%s' '</rdf:RDF></x:xmpmeta>'; }
printf '%s' '<rdf:Description rdf:about="" '"$ns_hdrgm"' hdrgm:Version="1.0" hdrgm:GainMapMin="0" hdrgm:GainMapMax="2.5" hdrgm:Gamma="1" hdrgm:OffsetSDR="0.015625" hdrgm:OffsetHDR="0.015625" hdrgm:HDRCapacityMin="0" hdrgm:HDRCapacityMax="3" hdrgm:BaseRenditionIsHDR="False"/>' | wrap > "$t/xmp-attr"
printf '%s' '<rdf:Description rdf:about="" '"$ns_hdrgm"' hdrgm:Version="1.0" hdrgm:HDRCapacityMin="0" hdrgm:HDRCapacityMax="3"><hdrgm:GainMapMin><rdf:Seq><rdf:li>0</rdf:li><rdf:li>0.1</rdf:li><rdf:li>0.2</rdf:li></rdf:Seq></hdrgm:GainMapMin><hdrgm:GainMapMax><rdf:Seq><rdf:li>2.5</rdf:li><rdf:li>2.4</rdf:li><rdf:li>2.3</rdf:li></rdf:Seq></hdrgm:GainMapMax><hdrgm:Gamma><rdf:Seq><rdf:li>1</rdf:li></rdf:Seq></hdrgm:Gamma></rdf:Description>' | wrap > "$t/xmp-seq"
printf '%s' '<rdf:Description rdf:about="" xmlns:HDRGainMap="http://ns.apple.com/HDRGainMap/1.0/"><HDRGainMap:HDRGainMapVersion>131072</HDRGainMap:HDRGainMapVersion><HDRGainMap:HDRGainMapHeadroom>4.5</HDRGainMap:HDRGainMapHeadroom></rdf:Description>' | wrap > "$t/xmp-apple"
printf '%s' '<rdf:Description rdf:about="" '"$ns_hdrgm"' hdrgm:Version="1.0"/>' | wrap > "$t/xmp-version"
for n in attr seq apple version; do cp "$t/xmp-$n" "$out/xmp/$n"; done
head -c 150 "$t/xmp-attr" > "$out/xmp/cut"
printf '%s' '<<<hdrgm:GainMapMax="&bogus;' > "$out/xmp/garbage"

# ---- JPEG pieces. seg <marker> <payload file>: FF <marker> <length> <payload>.
seg() { bytes 255 "$1"; be16 $(($(size "$2") + 2)); cat "$2"; }
{ bytes 0; rep 1 64; } > "$t/dqt"
bytes 8 0 1 0 1 1 1 17 0 > "$t/sof"
{ bytes 0 1; rep 0 15; bytes 0; } > "$t/dht-dc"
{ bytes 16 1; rep 0 15; bytes 0; } > "$t/dht-ac"
bytes 1 1 0 0 63 0 > "$t/sos"
{ seg 219 "$t/dqt"; seg 192 "$t/sof"; seg 196 "$t/dht-dc"; seg 196 "$t/dht-ac"; seg 218 "$t/sos"; bytes 63 255 217; } > "$t/body"
app1() { { printf 'http://ns.adobe.com/xap/1.0/\000'; cat "$1"; } > "$t/app1"; seg 225 "$t/app1"; }
app2_iso() { { printf 'urn:iso:std:iso:ts:21496:-1\000'; cat "$1"; } > "$t/app2"; seg 226 "$t/app2"; }

# The gain-map image: SOI, XMP (hdrgm attributes), ISO payload, the 1x1 image.
{ bytes 255 216; app1 "$t/xmp-attr"; app2_iso "$t/iso1"; cat "$t/body"; } > "$t/gm.jpg"
{ bytes 255 216; app1 "$t/xmp-seq"; cat "$t/body"; } > "$t/gm-seq.jpg"
gm=$(size "$t/gm.jpg")

# The primary XMP: hdrgm:Version, and the Google container directory with the gain-map length.
{
  printf '%s' '<rdf:Description rdf:about="" xmlns:Container="http://ns.google.com/photos/1.0/container/" xmlns:Item="http://ns.google.com/photos/1.0/container/item/" '"$ns_hdrgm"' hdrgm:Version="1.0"><Container:Directory><rdf:Seq><rdf:li rdf:parseType="Resource"><Container:Item Item:Semantic="Primary" Item:Mime="image/jpeg"/></rdf:li><rdf:li rdf:parseType="Resource"><Container:Item Item:Semantic="GainMap" Item:Mime="image/jpeg" Item:Length="'"$gm"'"/></rdf:li></rdf:Seq></Container:Directory></rdf:Description>'
} | wrap > "$t/xmp-primary"

# The MPF payload: <be|le> <primary size> <offset of entry 1> <gain-map size>. The offsets are
# relative to the endian field.
mpf_payload() {
  if [ "$1" = be ]; then w16=be16; w32=be32; printf 'MPF\000MM\000*'; else w16=le16; w32=le32; printf 'MPF\000II*\000'; fi
  $w32 8; $w16 3
  $w16 $((0xB000)); $w16 7; $w32 4; printf '0100'
  $w16 $((0xB001)); $w16 4; $w32 1; $w32 2
  $w16 $((0xB002)); $w16 7; $w32 32; $w32 50
  $w32 0
  $w32 $((0x030000)); $w32 "$2"; $w32 0; $w16 0; $w16 0
  $w32 0; $w32 "$4"; $w32 "$3"; $w16 0; $w16 0
}

# The primary image: SOI, XMP, ISO version only, MPF (just before the image data), the 1x1 image.
{ bytes 255 216; app1 "$t/xmp-primary"; app2_iso "$t/isov"; } > "$t/prefix"
mpf_payload be 0 0 0 > "$t/mpf0"
mpf_seg=$(($(size "$t/mpf0") + 4))
primary=$(($(size "$t/prefix") + mpf_seg + $(size "$t/body")))
entry1=$((primary - $(size "$t/prefix") - 8))
for e in be le; do
  mpf_payload "$e" "$primary" "$entry1" "$gm" > "$t/mpf-$e"
  { cat "$t/prefix"; seg 226 "$t/mpf-$e"; cat "$t/body"; cat "$t/gm.jpg"; } > "$t/uhdr-$e.jpg"
done
# The same without MPF: the gain map is found by the container directory, then by position.
{ cat "$t/prefix"; cat "$t/body"; cat "$t/gm.jpg"; } > "$t/uhdr-gcontainer.jpg"
{ bytes 255 216; app1 "$t/xmp-version"; app2_iso "$t/isov"; cat "$t/body"; cat "$t/gm.jpg"; } > "$t/uhdr-follows.jpg"
# XMP rdf:Seq in the gain map (Adobe), and a gain map with an ISO payload only.
{ cat "$t/prefix"; seg 226 "$t/mpf-be"; cat "$t/body"; cat "$t/gm-seq.jpg"; } > "$t/uhdr-seq.jpg"
{ bytes 255 216; app2_iso "$t/isov"; cat "$t/body"; bytes 255 216; app2_iso "$t/iso1"; cat "$t/body"; } > "$t/uhdr-iso.jpg"
# Not an Ultra HDR file, and truncated ones.
{ bytes 255 216; cat "$t/body"; } > "$t/plain.jpg"
head -c "$((primary + 40))" "$t/uhdr-be.jpg" > "$t/cut-in-gm.jpg"
head -c "$((primary - 20))" "$t/uhdr-be.jpg" > "$t/cut-in-primary.jpg"

# ---- an AVIF with a tmap item: base (1), gain map (2), tmap (3) derived from both, in an altr
# group with the tmap first; the item data is in the mdat. The coded items are 4 stub bytes.
# build_meta <offset of the mdat payload> [xmp]: with "xmp" there is also an XMP item (4, mime
# application/rdf+xml) that describes the base image (cdsc) and holds hdrgm:Version, so the
# HEIF XMP path (item size check, nested XMP parse) has a seed.
box() { p=$(mktemp "$t/b.XXXXXX"); cat > "$p"; be32 $(($(size "$p") + 8)); printf '%s' "$1"; cat "$p"; rm -f "$p"; }
full() { bytes 0 0 0 0; }
infe() { { bytes 2 0 0 0; be16 "$1"; be16 0; printf '%s' "$2"; bytes 0; } | box infe; }
# An infe of type mime with content type application/rdf+xml (the XMP item of HEIF).
infe_xmp() { { bytes 2 0 0 0; be16 "$1"; be16 0; printf 'mime'; bytes 0; printf 'application/rdf+xml'; bytes 0 0; } | box infe; }
{ printf 'avif'; be32 0; printf 'avif'; printf 'mif1'; printf 'tmap'; } | box ftyp > "$t/ftyp"
{ bytes 0; cat "$t/iso1"; } > "$t/tmap-item"
build_meta() {
  x=0; if [ "${2:-}" = xmp ]; then x=1; fi
  {
    full
    { full; be32 0; printf 'pict'; be32 0; be32 0; be32 0; bytes 0; } | box hdlr
    { full; be16 1; } | box pitm
    { full; be16 $((3 + x)); infe 1 av01; infe 2 av01; infe 3 tmap; if [ "$x" = 1 ]; then infe_xmp 4; fi; } | box iinf
    {
      full; { be16 3; be16 2; be16 1; be16 2; } | box dimg
      if [ "$x" = 1 ]; then { be16 4; be16 1; be16 1; } | box cdsc; fi
    } | box iref
    {
      { { full; be32 16; be32 16; } | box ispe; { bytes 129 0 0 0; } | box av1C; } | box ipco
      { full; be32 3; bytes 0 1 2 1 2 0 2 2 1 2 0 3 1 1; } | box ipma
    } | box iprp
    { full; bytes 68 0; be16 $((3 + x))
      be16 1; be16 0; be16 1; be32 "$1"; be32 4
      be16 2; be16 0; be16 1; be32 $(($1 + 4)); be32 4
      be16 3; be16 0; be16 1; be32 $(($1 + 8)); be32 $(size "$t/tmap-item")
      if [ "$x" = 1 ]; then be16 4; be16 0; be16 1; be32 $(($1 + 8 + $(size "$t/tmap-item"))); be32 "$(size "$t/xmp-version")"; fi
    } | box iloc
    { { full; be32 1; be32 2; be32 3; be32 1; } | box altr; } | box grpl
  } | box meta
}
build_meta 0 > "$t/meta0"
mdat_at=$(($(size "$t/ftyp") + $(size "$t/meta0") + 8))
{ cat "$t/ftyp"; build_meta "$mdat_at"; { printf 'AV1a'; printf 'AV1b'; cat "$t/tmap-item"; } | box mdat; } > "$t/tmap.avif"
build_meta 0 xmp > "$t/meta0x"
mdat_at=$(($(size "$t/ftyp") + $(size "$t/meta0x") + 8))
{ cat "$t/ftyp"; build_meta "$mdat_at" xmp; { printf 'AV1a'; printf 'AV1b'; cat "$t/tmap-item"; cat "$t/xmp-version"; } | box mdat; } > "$t/tmap-xmp.avif"

# ---- an iPhone-style HEIC: the primary image (1), the Apple gain map (2) as an auxiliary image
# (auxl reference, auxC urn:com:apple:photo:2020:aux:hdrgainmap), and two XMP items: 3 describes
# the primary (hdrgm:Version) and 4 describes the gain map (the Apple HDRGainMap headroom). The
# coded items are 4 stub bytes behind a minimal hvcC (no NAL arrays).
{ printf 'heic'; be32 0; printf 'mif1'; printf 'heic'; } | box ftyp > "$t/ftyp-heic"
{ bytes 1 0; rep 0 14; bytes 1 0 0 0 0 3 0; } > "$t/hvcc"
{ full; printf 'urn:com:apple:photo:2020:aux:hdrgainmap'; bytes 0; } > "$t/auxc"
build_heic() { # <offset of the mdat payload>
  {
    full
    { full; be32 0; printf 'pict'; be32 0; be32 0; be32 0; bytes 0; } | box hdlr
    { full; be16 1; } | box pitm
    { full; be16 4; infe 1 hvc1; infe 2 hvc1; infe_xmp 3; infe_xmp 4; } | box iinf
    {
      full
      { be16 2; be16 1; be16 1; } | box auxl
      { be16 3; be16 1; be16 1; } | box cdsc
      { be16 4; be16 1; be16 2; } | box cdsc
    } | box iref
    {
      {
        { full; be32 16; be32 16; } | box ispe
        box hvcC < "$t/hvcc"
        box auxC < "$t/auxc"
        { full; be32 8; be32 8; } | box ispe
      } | box ipco
      { full; be32 2; bytes 0 1 2 1 2 0 2 3 4 2 3; } | box ipma
    } | box iprp
    { full; bytes 68 0; be16 4
      be16 1; be16 0; be16 1; be32 "$1"; be32 4
      be16 2; be16 0; be16 1; be32 $(($1 + 4)); be32 4
      be16 3; be16 0; be16 1; be32 $(($1 + 8)); be32 "$(size "$t/xmp-version")"
      be16 4; be16 0; be16 1; be32 $(($1 + 8 + $(size "$t/xmp-version"))); be32 "$(size "$t/xmp-apple")"
    } | box iloc
  } | box meta
}
build_heic 0 > "$t/meta0h"
mdat_at=$(($(size "$t/ftyp-heic") + $(size "$t/meta0h") + 8))
{ cat "$t/ftyp-heic"; build_heic "$mdat_at"; { printf 'HEV1'; printf 'HEV2'; cat "$t/xmp-version"; cat "$t/xmp-apple"; } | box mdat; } > "$t/apple.heic"

# ---- the seeds of each target.
for f in uhdr-be.jpg uhdr-le.jpg uhdr-gcontainer.jpg uhdr-follows.jpg uhdr-seq.jpg uhdr-iso.jpg plain.jpg cut-in-gm.jpg cut-in-primary.jpg tmap.avif tmap-xmp.avif apple.heic; do
  cp "$t/$f" "$out/read/$f"
  { bytes 3; cat "$t/$f"; } > "$out/decode/3-$f"
  { bytes 0; cat "$t/$f"; } > "$out/decode/0-$f"
done
tail -c +5 "$t/mpf-be" > "$out/mpf/be"
tail -c +5 "$t/mpf-le" > "$out/mpf/le"
tail -c +5 "$t/mpf-be" | head -c 30 > "$out/mpf/cut"
cp "$t/uhdr-be.jpg" "$out/scan/primary.jpg"
cp "$t/gm.jpg" "$out/scan/gain-map.jpg"
cp "$t/cut-in-primary.jpg" "$out/scan/cut.jpg"
echo "seeds: $(find "$out" -type f | wc -l) files in $out"
