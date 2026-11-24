# Parity

There is no Go original for this crate. Four C and C++ libraries are the oracles. They run only in CI,
never on a developer machine, and are never a dependency of the crate.

| Oracle | Version (tag, commit) | What it checks |
|---|---|---|
| libultrahdr + libjpeg-turbo | v2.0.2 `e5f5a022fe96`, 3.2.0 `c85e6b905bf2` | JPEG: the gain-map metadata (bit for bit), the coded base image and gain map, and their decoded pixels |
| libavif with dav1d | v1.4.2 `c5240fc79fe5`, dav1d 1.0.0 (Debian bookworm) | AVIF: the gain-map metadata as stored fractions, and the decoded base and gain-map planes with their transforms |
| libheif with libde265 | v1.23.7 `12a259a5882d`, v1.0.15 `17bb8d9fcea6` | HEIC: the decoded base and gain-map planes (by item id) with their transforms |
| ExifTool | 13.64 `3d7a0538324e` | JPEG: the `hdrgm` and `HDRGainMap` XMP values |

All are built from source in `.github/ci/build-oracle-libs.sh`, each checked out by tag and by
commit (the script fails if the tag points to another commit). The corpus is in docs/corpus.md.

`.github/workflows/ci.yml` runs the parity on `main` and on `workflow_dispatch`, never on a pull
request: job `parity` (all channels) and job `parity-arm64`. A crate is done when both are green.

## The Rust side

`examples/oracle.rs` has five commands. It uses `gainmap`, `isobmff` (the transform types) and the standard library only.

```
oracle uhdr <in-dir> <out-dir> [--no-raw]
oracle avif <in-dir> <out-dir>
oracle planes <in-dir> <out-dir>
oracle xmp  <in-dir> <out.jsonl>
oracle gen  <in-dir> <out-dir>
```

The C++ tool `tests/parity/uhdr_oracle.cpp` takes the same arguments as `oracle uhdr` and writes
the same files; `tests/parity/avif_oracle.c` does the same for `oracle avif`.

### `uhdr`: files and text

For each regular file `<f>` of the input dir (sorted) it writes `<out>/<f>.txt` and, as far as
they exist:

| File | Content |
|---|---|
| `<f>.base.jpg` | the coded bytes of the base image (what `uhdr_dec_get_base_image` returns) |
| `<f>.gm.jpg` | the coded bytes of the gain-map image (`uhdr_dec_get_gainmap_image`) |
| `<f>.base.rgba` | the base image, RGBA8888, tightly packed |
| `<f>.gm.raw` | the gain map: 8-bit gray, or RGBA8888 when it has three components |

`<f>.txt` is exactly this, with `\n` line ends and floats as 8 lowercase hex digits of the `f32`
bits:

```
status ok|<error class>
base.width N
base.height N
gainmap.width N
gainmap.height N
gainmap.channels 1|4
max_content_boost H H H
min_content_boost H H H
gamma H H H
offset_sdr H H H
offset_hdr H H H
hdr_capacity_min H
hdr_capacity_max H
use_base_cg 0|1
```

The five size and channel lines follow when the status is `ok`, and also when it is `no-metadata`
(pixels but no usable metadata: the Apple files); the metadata lines follow only when it is `ok`.
These are libultrahdr's linear `uhdr_gainmap_metadata_t` fields. `gainmap` produces them from `Metadata` the way libultrahdr
does:

- `max_content_boost` is `exp2(gain_map_max)`, `min_content_boost` is `exp2(gain_map_min)`,
  `hdr_capacity_min` is `exp2(base_hdr_headroom)`, `hdr_capacity_max` is
  `exp2(alternate_hdr_headroom)`. `gamma` and the offsets are used as they are
  (`offset_sdr` is `base_offset`, `offset_hdr` is `alternate_offset`).
- **ISO payload.** libultrahdr's `gainmapMetadataFractionToFloat` (`gainmapmetadata.cpp`) computes
  `exp2((float)n / d)`. That file includes `<cmath>` and has no `using namespace std`, so the call
  is the C `exp2(double)` and the result is rounded to `float`. The oracle does the same:
  `f64::from(x).exp2() as f32`. `Metadata::from_iso` already divides in `f32`
  (`n as f32 / d as f32`), as libultrahdr does.
- **XMP.** `XMPXmlHandler` (`jpegrutils.cpp`) reads the text with `stringstream >> float`, which
  is a correctly rounded decimal-to-`float` conversion, as `str::parse::<f32>`. `exp2(val)` there
  has `using namespace std` and a `float` argument, so it is `exp2f`. The oracle does the same:
  `x.exp2()` on `f32`.
- `use_base_cg` is the ISO flag 0x40 for an ISO payload and always 1 for XMP (libultrahdr sets
  it to true there).
- Which one applies: the metadata came from the ISO payload when `Metadata::from_iso` of the
  stored `IsoMetadata` is equal to the resolved `Metadata`.

If libultrahdr's `exp2` turns out to be the `float` one after all, the only change is
`exp2_iso` in `examples/oracle.rs`.

### Status

The C++ tool writes `status ok` or `status error`. The Rust side writes `status ok` or the error
class: `Error::class()` of a failed `read` or `decode`, or one of `no-gain-map`, `not-jpeg`,
`no-metadata` (a gain map without usable metadata), `backward-direction` (ISO flag 0x04, XMP
`BaseRenditionIsHDR` True: libultrahdr refuses these), `no-gain-map-pixels`. For the `uhdr`
channels `run.sh` compares only whether the first line is `status ok`: the error classes of the
two sides differ by design. The classes of every input are listed in the report.

That makes "both refused" equal, so it is never enough alone: a file that libultrahdr refuses but
that has a gain map is gated by the fallback channels and the gain-map gates below, and the
minimum counts of the gate count only files that the C tool read.

`ok` means the same on both sides: a base image and a gain map were found, both decode, and the
metadata is usable. The coded bytes and the pixels are written whenever they exist, whatever the
status, so a file that only one side refuses still has its other channels compared.

### Pixels

The C++ tool decodes the coded bytes with the TurboJPEG 3 API (`tj3Decompress8`): accurate DCT,
fancy upsampling (the defaults), `TJPARAM_STOPONWARNING` on, so a truncated or damaged image is
an error as in `gainmap::decode`. One component gives 8-bit gray, anything else RGBA8888. jpeg-rs
is meant to match libjpeg-turbo bit for bit in this mode (docs of jpeg-rs), so the raw files are
compared with `cmp`.

### `avif`

`<f>.txt`, same style (the Rust side reads the file with `gainmap::read`, the C side with
`avifDecoderParse`; no pixel is decoded and libavif is built with no codec, because only
`avifDecoderNextImage` creates a codec):

```
status ok|no-gainmap|error            (libavif)
status ok|no-metadata|no-gain-map|<error class>   (Rust)
base.width N
base.height N
gainmap.width N
gainmap.height N
flags U
base_hdr_headroom n/d
alternate_hdr_headroom n/d
c.min n/d
c.max n/d
c.gamma n/d
c.base_offset n/d
c.alternate_offset n/d
```

The last five lines are repeated for `c` = 0, 1, 2. `U` is `use_base_colour_space` (0 or 1). The
fractions are the stored ones (`Fraction::numerator/denominator`, `avifGainMap` fields), so no
float rounding is involved. libavif is lenient here (`AVIF_STRICT_DISABLED`) and ignores a gain
map it cannot use (unsupported version, no `tmap` brand, wrong `altr` group): that is
`no-gainmap`; a `tmap` payload it rejects (gamma 0, bytes after the last field of a writer
version it supports) fails the whole parse: `error`.

The Rust oracle tells three outcomes apart: `ok`, `no-metadata` (the gain map is kept, but there
is no usable metadata: an unsupported `tmap` version, for example) and `no-gain-map` (dropped),
and a failed `read` prints its error class. `run.sh` reduces the first lines before it compares
them (`norm_avif`): libavif `ok`, `no-gainmap`, `error` stay as they are; Rust `ok` stays,
`no-metadata` becomes `no-gainmap` (keeping a gain map it cannot use is the same outcome as
libavif ignoring it), `no-gain-map` becomes `dropped` and an error class becomes `rust-<class>`.
Only the same outcome is equal, so a read error or a dropped gain map never equals a libavif
outcome. When both say `ok` the rest of the text, the fractions, must be identical. Only AVIF
inputs run this channel; libavif refuses a HEIC (no `avif` brand), which is gated as described
under "HEIC" below.

### `xmp`

`oracle xmp` writes one JSON object per input:

```
{"SourceFile":"<name>","base":[w,h],"gm":[w,h],"primary":{"Version":"1.0"},"gainmap":{"GainMapMax":["2.5"],...}}
{"SourceFile":"<name>","Error":"<class>"}
```

The values are the raw strings of `GainMapImage`: `primary_version`, `HdrgmXmp` and
`AppleGainMap` of the gain map (a list for the `rdf:Seq` fields). `run.sh` runs ExifTool
(`-j -n -G1 -a -b -XMP-hdrgm:all -XMP-HDRGainMap:all -XMP-GContainer:all -MPF:NumberOfImages`
`-ImageWidth -ImageHeight`) over the inputs (the primary image) and
over the `<f>.gm.jpg` files that `oracle uhdr` wrote (the gain-map image), and
`tests/parity/compare.py` compares the values. Only number formatting is normalised (`1.0` and
`1`; a list of one value and the value; `True` and `true`). The GContainer directory is not in
the public API, so its values are not compared; ExifTool's view of it is used only for the
gain-map gate below. For an AVIF with an Adobe auxiliary gain map the `hdrgm` XMP is read from
the file by ExifTool and compared with the same values. If a tag name of ExifTool differs from the one here
(`HDRGainMapVersion`, `HDRGainMapHeadroom`), the first run shows it in `xmp-mismatches.tsv`.

### `gen`

Deterministic mutations of every JPEG of at most 8 MB, at most 64 per input, written as
`<f>.<kind><nn>`: truncations at the start and end of the first 16 segments, at the end of the
primary image and 1 to 5 and 40 bytes into the next one; a flipped bit in every third byte of the
first 40 of the MPF payload and every fifth of the first 70 of each ISO payload; MPF entry 1
offset set to 0, `u32::MAX`, the file length, and plus and minus one; MPF entry 1 size set to 0 and
`u32::MAX`. Used by the no-crash sweep; also good fuzz seeds.

## The run

`tests/parity/run.sh <corpus> <oracle-binary> <report-dir>` works on one input at a time, to
bound the disk use: it runs both oracles into temp dirs, compares the `.txt` files with `diff`
(uhdr: after reducing the first line to `ok` or not; avif: see above) and the raw files with
`cmp`, and deletes them. The inputs are the names listed in `corpus.tsv` (`inputs.sh`), not
whatever else the shared cache holds, and `fetch.sh` checks size and SHA-256 of every cached file
on every run. Channels: `uhdr-txt` (the status line and the metadata lines), `uhdr-dims` (the
base and gain-map sizes and the channel count, lines 2 to 6; compared only when libultrahdr read
the file), `uhdr-base-jpg`, `uhdr-gm-jpg`, `uhdr-base-rgba`, `uhdr-gm-raw`, `avif`,
`turbo-base-rgba`, `turbo-gm-raw`, `base-prefix`, `base-size`, `exiftool-mpimage2`, `heifid`, `xmp` and the eight pixel channels `heifpix-*` and `avifpix-*` (see `planes`).
A Rust `<f>.<which>.err` from `oracle planes` (an image that `read` reported and `decode` failed
on) is a failure (`planes-rust-err` in `failures.tsv`), so the images that no C tool decodes (the
Adobe auxiliary gain maps and the other libavif refusals) are gated too; and a known
`avifpix-*` line only covers a result of `only-rust`.
The report (`summary.md`) has, per channel, the counts inputs, ok (the C tool produced it), equal, differ, known and n/a. It also
has the Rust status of every input and the sweep result.

Two fallback channels make sure a file libultrahdr cannot read is still checked by independent C
tools:

- `turbo-base-rgba`, `turbo-gm-raw`: only when libultrahdr's status is not ok. For a JPEG the
  Rust oracle must have written `base.jpg` / `gm.jpg`; if it did not, the channel is
  `missing-rust`, a difference that no `uhdr` line of `known-differences.tsv` covers (the corpus
  JPEGs all carry a gain map, so a missing file means `gainmap` dropped it).
  `uhdr_oracle decode <in.jpg> <out.raw>` decodes that JPEG with libjpeg-turbo alone (same
  settings and pixel format as above), and `cmp` compares it with the Rust `base.rgba` /
  `gm.raw`. A decode failure of the C tool counts as a difference.
- `base-prefix`, `base-size`: only when libultrahdr's status is not ok and the Rust oracle wrote
  `base.jpg`. `base-prefix` checks that `base.jpg` is the first bytes of the input file;
  `base-size` checks that the Rust `base.width` and `base.height` are the `ImageWidth` and
  `ImageHeight` ExifTool reports for the file. Where the base image ends is not checked
  independently for these files (the `turbo-base-rgba` decode stops at the first EOI, so it does
  not show a base range that runs too far).
- `exiftool-mpimage2`: for every JPEG where the Rust oracle wrote `gm.jpg`.
  `exiftool -b -MPImage2 <file>` gives the second MPF image, and `cmp` compares it with the Rust
  `gm.jpg`. This checks where `gainmap` found the gain map, for all files, also those
  libultrahdr reads. If ExifTool succeeds and gives no `MPImage2` (no MPF, for example a
  Google-container-only file) the result is `n/a` and is not a difference. An ExifTool that
  exits with an error is a failure of the run (`failures.tsv`) and a difference, never `n/a`.

### `planes`: HEIC and AVIF pixels

`oracle planes <in-dir> <out-dir>` decodes, for every HEIF and AVIF input, the base image and the
gain map (each alone, as the base of a copy of the read result, so one failure does not hide the
other) and writes, for `<which>` = `base`, `gm`:

- `<f>.<which>.yuv`: the planes Y, Cb, Cr (Y only for 4:0:0), tight rows, one byte per sample at
  bit depth 8, else two bytes little-endian. No colour conversion, no transform applied.
- `<f>.<which>.info`: `W H chroma depthY depthC` (chroma 400, 420, 422 or 444; for 4:0:0 depthC is
  depthY), then the transforms, kind by kind in this order: `crop L T R B` (the borders; `crop
  inexact` when the `clap` is not an exact integer rectangle), `imir axis` (the raw `axis` byte:
  0 top and bottom swap, 1 left and right swap) and `irot degrees` (anticlockwise). The order
  across kinds is fixed because libavif cannot report the `ipma` order; the raw `ipma` order is in
  `Planes::transforms`.
- `<f>.ids`: the item ids that gainmap reports (`base N`, `gm N`), which the libheif oracle
  decodes; `<f>.<which>.err` with the error class when the decode failed; `<f>.txt`.

The same command also checks, for every image it decodes, that `decode` gives the identical result
for `max_threads` 1, 2 and 3, `Cancelled` with the flag set, `LimitExceeded(Alloc)` under half of
what the result holds and `LimitExceeded(Pixels)` one pixel under the image, and that decoding
the base and the gain map together gives what each gives alone. A failed check is a non-zero
exit, which `run.sh` records as a failure of the run (`planes-rust`).

The C tools write the same files:

- `tests/parity/heif_oracle.c` (libheif v1.23.7 with libde265 v1.0.15, static): for each id in
  `<f>.ids` it calls `heif_context_get_image_handle`, which finds hidden and auxiliary items, and
  decodes with `ignore_transformations = 1`, an undefined colour space and chroma (the stored
  format is kept) and `output_image_nclx_profile_passthrough` (no range conversion), as hevc-rs's
  oracle does. The transforms come from `heif_item_get_transformation_properties` and the
  property getters (the crop as libheif's borders for the image size). The ids come from
  gainmap because libheif v1.23.7 has no `tmap` support.
- `tests/parity/avif_oracle.c` (libavif v1.4.2 with dav1d, built with the system dav1d of Debian
  bookworm): `avifDecoderNextImage` with `COLOR_AND_ALPHA | GAIN_MAP`, then the Y, U, V planes of
  `image` and of `gainMap->image` and `transformFlags`, `clap` (through
  `avifCropRectFromCleanApertureBox`), `irot` and `imir`.

Why the transforms are not applied and libheif is not run with them on is in docs/design.md.

Channels (each compares the `.info` or `.yuv` file of the C tool with the Rust one with `cmp`):
`heifpix-base-info`, `heifpix-base-yuv`, `heifpix-gm-info`, `heifpix-gm-yuv` (HEIC, libheif) and
`avifpix-base-info`, `avifpix-base-yuv`, `avifpix-gm-info`, `avifpix-gm-yuv` (AVIF, libavif). A
known-differences line for `avifpix-gm` covers the gm channels and nothing else; the bare
`avifpix` would also hide the base image, so it is not used.

Where libavif or libheif cannot give planes for a file, the file is in `known-differences.tsv`
with the reason, and only that image is left unchecked:

- libavif ignores the gain map of the eight Adobe-aux AVIFs (`auxl` type
  `urn:com:photo:aux:hdrgainmap`), of `seine_sdr_gainmap_notmapbrand.avif`,
  `seine_hdr_gainmap_wrongaltr.avif`, `unsupported_gainmap_version.avif`,
  `unsupported_gainmap_minimum_version.avif`, the gm channel only
  (libavif reads `unsupported_gainmap_writer_version_with_extra_bytes.avif` fully, so it has no line). Their gain-map planes are decoded
  by gainmap only. Their base planes are compared.
- libavif fails the parse of `seine_sdr_gainmap_gammazero.avif` and
  `supported_gainmap_writer_version_with_extra_bytes.avif`, so it decodes neither image.

Known limits of this comparison: a `clap` that is not an exact integer rectangle is `crop
inexact` for gainmap and libavif but rounded by libheif, so such a HEIC would differ in `.info`
(none is in the corpus: the libultrahdr HEIC has `clap` 16x16 at offset -24/-24 on 64x64, which
is exact); more than one `irot`, `imir` or `clap` on an item is an error in libheif even with
transforms ignored, and gainmap returns them all. The gain-map planes of a HEIC `tmap` file are
decoded from the item id gainmap reports, not found by libheif itself.

### Result of the first green run (run 45290454359, 56 inputs)

| channel | inputs | C tool decoded | equal | differ | known |
|---|---:|---:|---:|---:|---:|
| `heifpix-base-info` / `-yuv` | 2 | 2 | 2 | 0 | 0 |
| `heifpix-gm-info` / `-yuv` | 2 | 2 | 2 | 0 | 0 |
| `avifpix-base-info` / `-yuv` | 23 | 21 | 21 | 0 | 2 |
| `avifpix-gm-info` / `-yuv` | 23 | 9 | 9 | 0 | 14 |

So 2 HEIC files and 23 AVIFs were compared: 25 base images and 11 gain maps match libheif and
libavif plane for plane (every sample, the size, chroma format, bit depths and transform list),
with 0 mismatches. The known lines are listed above. The pre-existing channels stayed equal
(`uhdr-txt` 49 equal and 7 known, `avif` 19 equal and 4 known, XMP: 412 values compared, 412 match).

### Generated files (run 45297466465, 77 inputs)

`tests/parity/gen.sh` (docs/corpus.md "Generated files") adds 21 files to the 56 fetched ones: 17
AVIFs from libavif v1.4.2 (`avifenc`, `avifgainmaputil combine`, aom v3.14.1) and 4 HEICs from
libultrahdr v2.0.2 (libheif PR 1503 patch, x265). The stage `gen-run` of `.github/ci/Dockerfile`
holds the generators; the `parity` stage runs `gen.sh` after `fetch.sh`, on every run (the files
are not cached; `gen-pins.tsv` pins their SHA-256). They go through every channel above with no
special case. The counts of that run:

| channel | inputs | C tool decoded | equal | differ | known |
|---|---:|---:|---:|---:|---:|
| `heifpix-base-info` / `-yuv` | 6 | 6 | 6 | 0 | 0 |
| `heifpix-gm-info` / `-yuv` | 6 | 6 | 6 | 0 | 0 |
| `avifpix-base-info` / `-yuv` | 40 | 38 | 38 | 0 | 2 |
| `avifpix-gm-info` / `-yuv` | 40 | 26 | 26 | 0 | 14 |
| `avif` (metadata) | 40 | 26 | 36 | 0 | 4 |

So 6 HEICs and 40 AVIFs were compared: 44 base images and 32 gain maps match libheif and libavif
sample for sample, in size, chroma format, bit depths and transform list, with 0 mismatches. The
existing channels did not change (`uhdr-txt`: 70 equal, 7 known, which is 49 plus the 21 new
files that libultrahdr refuses as not JPEG; XMP: 441 values compared, 441 match, none dropped. The run of main after PR 5 (45308886373),
which added an ExifTool `hdrgm` compare for the Adobe auxiliary XMP of the AVIF files and the
`heifid` channel, has XMP: 505 values compared, 505 match, and `heifid`: 1 file, equal).
`oracle planes` also passed its limit, thread and cancel checks on every new file.

Two oracle facts the generated files showed. libavif keeps the `clap`, `irot` and `imir` of a
`tmap` file on the decoded base image only (`gainMap->image` has none; `read.c` requires the
gain-map item to carry the same ones), so `avif_oracle.c` writes the base image's transforms into
`<f>.gm.info`; before that the oracle wrote none and the gain-map `.info` of the three
`gen_tmap_irot*`/`imir*` files differed from Rust's, which lists the gain-map item's own
properties. And libavif's JPEG reader finds the gain map of an Ultra HDR JPEG from the `hdrgm`
XMP, so the libultrahdr build writes XMP too (`UHDR_WRITE_XMP=ON`); without it `avifenc` wrote an
AVIF with no gain map, which `gen.sh` now rejects (an encoder log with no gain-map line fails the
file).

### Never-drop gates

`gainmap` keeps every gain map that a file links to its base image. The run enforces that from
three independent sources, so a regression cannot hide behind "libultrahdr refuses it too":

- **libultrahdr**: a file it reads (`status ok`) must have the same status, sizes, metadata, bytes
  and pixels on the Rust side; a Rust `no-gain-map` is a difference in `uhdr-txt`.
- **libavif** (AVIF): where it reports a gain map, the Rust text must be `ok` and equal; where it
  reports none or fails, the Rust outcome must be `no-metadata`, never `no-gain-map` or a read
  error, except for the files in `known-differences.tsv`. A line there for the `avif` channel
  names the one Rust outcome it covers (`ok` or `no-gainmap`); a dropped gain map or a read
  error on that file is still a difference.
- **ExifTool** (`compare.py`): a file counts as having a gain map when ExifTool shows an `hdrgm`
  or `HDRGainMap` tag, `MPF:NumberOfImages` of 2 or more, or a Google container item with the
  semantic `GainMap`; every HEIC/HEIF input counts too. If `oracle xmp` reports no gain map for
  it, that is a mismatch that `known-differences.tsv` cannot list. An input, or a gain-map JPEG
  that Rust wrote, that ExifTool has no entry for is a mismatch too.

### HEIC

libavif refuses a HEIC, so the six HEICs of the corpus (an iPhone file with an auxiliary
`hdrgainmap` image and no `tmap`, the libultrahdr `tmap` fixture and four generated by `gen.sh`) are gated by ExifTool and by Rust itself, and all six by the `heifpix-*` plane channels (libheif and libde265): `oracle xmp` must
report a gain map; the Apple `HDRGainMapVersion` / `HDRGainMapHeadroom` values are compared with
the ExifTool entry of the file (when ExifTool shows any; the report counts the files where it did
not); and the base-image size must be among the `ImageWidth` / `ImageHeight` values ExifTool
reports. `oracle xmp` writes `"base":[w,h]` for that. The gain-map size of the HEIC is not
compared with anything: ExifTool's JSON holds one `ImageWidth` and one `ImageHeight` per file, so
there is no second size to compare with, and libavif does not read HEIC. These ExifTool tag names are the ones ExifTool 13.64 is
expected to use; if it names or places them differently, the first run shows it in
`xmp-mismatches.tsv`.

### The gate

The gate (`/report/status` is 0) needs all of this:

- the corpus is complete (77 inputs) and the C tools ran for every file;
- no difference outside `known-differences.tsv`, in any channel, and no gain-map gate failure;
- at least 15 `uhdr-txt` files (libultrahdr read them and the text is equal), 26 `avif` files
  (libavif read them and the text is equal) and 10 `exiftool-mpimage2` files are equal, so a
  run that compared nothing, or where both sides refused every file, fails; and at least 6 `heifpix-base-yuv`, 6 `heifpix-gm-yuv`, 38 `avifpix-base-yuv` and 26 `avifpix-gm-yuv` files are equal;
- no ExifTool failure (a non-zero exit of any ExifTool run);
- the no-crash sweep has no panic, abort, hang or tool failure. Every mutation goes through
  `oracle uhdr --no-raw` and `oracle xmp`, one batch per input; on a failure the files are tried
  one by one and the first that fails is kept in `crashers/`. A batch that fails with no single
  file reproducing it is reported for that input. An error is fine, a panic is not.

`known-differences.tsv` (file, channel, reason, and for `avif` the expected Rust outcome) lists documented, expected deviations only.
Channel `uhdr` covers every `uhdr-*` channel of the file (likewise `turbo`, `exiftool`). A line
that is not used is counted in the summary. Each line needs its reason before it is listed. Today
there are eleven for the metadata, pixel and XMP channels, and 16 for the pixel channels (the gm channels of 12 AVIFs, and both channels of two more, listed under `planes`):

- The five JPEGs with `rdf:Seq` XMP (`paris_exif_xmp_gainmap_bigendian.jpg`,
  `paris_exif_xmp_gainmap_littleendian.jpg`, `paris_exif_xmp_icc_gainmap_bigendian.jpg`,
  `seine_sdr_different_gainmap_srgb.jpg`, `seine_sdr_gainmap_srgb.jpg`), channel `uhdr`:
  libultrahdr reads attributes only and refuses them. They are gated by `turbo-*`,
  `exiftool-mpimage2`, the ExifTool XMP channel and the gates above.
- `apple_gainmap_old.jpg` and `apple_gainmap_new.jpg`, channel `uhdr-txt` only: libultrahdr
  derives metadata from the Apple headroom, `gainmap` keeps it raw, so the status line and the
  metadata lines differ. The sizes and the channel count (`uhdr-dims`), the coded bytes and the
  pixels must compare equal.
- Four AVIFs, channel `avif`, where `gainmap` keeps a gain map that libavif drops or refuses
  (the never-drop rule):
  - `seine_sdr_gainmap_notmapbrand.avif`: libavif needs the `tmap` brand (`no-gainmap`).
  - `seine_hdr_gainmap_wrongaltr.avif`: the `altr` group lists the base image first, so libavif
    does not prefer the `tmap` item (`no-gainmap`); `gainmap` keeps the gain map and its metadata.
  - `seine_sdr_gainmap_gammazero.avif`: libavif rejects a gamma of 0 in the `tmap` payload and
    the whole parse fails (`error`); `gainmap` keeps the gain map.
  - `supported_gainmap_writer_version_with_extra_bytes.avif`: writer version 0 with bytes after
    the last field: libavif fails the parse (`error`); `gainmap` ignores trailing bytes, as
    libultrahdr does, and keeps the gain map.

Predicted to agree with libavif (not listed): `unsupported_gainmap_version.avif` and
`unsupported_gainmap_minimum_version.avif` (libavif ignores the gain map; `gainmap` keeps it with
`metadata: None`, which is `no-metadata`, the same outcome) and
`unsupported_gainmap_writer_version_with_extra_bytes.avif` (a writer version above 0 may have
extra bytes; both read the same fractions). These were derived from libavif v1.4.2 `read.c` and
its tests, not run; if a first run shows another outcome, the difference needs its own reason.

### Where libultrahdr differs by design

These are known from its sources (v2.0.2); none was run.

- **Adobe `rdf:Seq` XMP.** libultrahdr's XMP handler reads `hdrgm:*` attributes only. Photoshop
  and Lightroom write `GainMapMin`, `GainMapMax` and `Gamma` as `rdf:Seq` elements. Without an ISO
  payload libultrahdr fails with "could not find attribute hdrgm:GainMapMax". `gainmap` reads
  the Seq (as libavif does). For such a file the gate is the ExifTool channel.
- **Apple `HDRGainMapHeadroom`.** libultrahdr reads it as a log2 value (`exp2`) and builds
  metadata from it. libavif and Apple's description treat it as linear, which fits Apple's range.
  `gainmap` keeps the Apple gain map with `metadata: None`.
- **First two images.** libultrahdr takes the first two JPEG images of the file; it ignores MPF
  and the Google container. `gainmap` uses MPF, then the container, then the following image.
  They agree on normal files.
- **ISO or XMP.** libultrahdr uses the ISO payload when there is one, and fails if it is invalid
  (for example `minimum_version` not 0). `gainmap` falls back to a valid XMP.
- **Direction.** libultrahdr refuses a map whose base is the HDR rendition. `gainmap` returns the
  map with `backward_direction` set, and the oracle prints `backward-direction`.
- **Size limits.** libultrahdr limits the image size (`UHDR_MAX_DIMENSION`); the Rust side limits
  `max_pixels` (2^30; 2^28 for the HEVC and AV1 decode) and `max_alloc`.

## parity-arm64

`tests/parity/arm64.sh` builds the oracle for x86_64 and for aarch64 (run under `qemu-user`),
and runs `uhdr`, `avif`, `planes` and `xmp` over every 4th input with both; the output dirs are compared
with `diff -r` and must be identical. This covers jpeg-rs's NEON and AVX2 decode paths and the
`libm` `exp2` / `exp2f` of the two architectures. If a text file ever differs by one bit in an
`exp2` line only, the cause is the `libm` of the two targets (glibc picks an FMA variant on
x86_64); say so here and decide then.

## Not covered

- The apply step (`HDR = (sdr + offset) * gain - offset`): the crate does not apply the gain map
  in v1 (docs/design.md).
- HDR rendering of HEIC and AVIF (docs/design.md). The HEICs have no libavif or libultrahdr channel (both refuse
  it); it is gated by ExifTool and the never-drop gates (see "HEIC" above).
- libheif has no `tmap` support (an open pull request), so the HEIC gain-map item of a `tmap` file is decoded by the id gainmap reports; how gainmap finds that item is gated by ExifTool and the never-drop gates. For a file with an Apple auxiliary image, libheif picks the primary item and the `hdrgainmap` auxiliary image itself and `run.sh` compares them with the ids of gainmap (channel `heifid`).

## Licences of what is ported

`src/iso.rs` is a port of `decodeGainmapMetadata` and `gainmapMetadataFractionToFloat` from
libultrahdr v2.0.2 `lib/src/gainmapmetadata.cpp` (MIT OR Apache-2.0): attribution is in the file
header and in NOTICE. The oracle sources in `tests/parity` call the public APIs only and copy no
code. ExifTool (GPL or Artistic) is used as a data source and an oracle; no code is copied.
