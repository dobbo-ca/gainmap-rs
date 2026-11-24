# Design

gainmap-rs finds the gain map in an Ultra HDR, ISO 21496-1 or Apple HDR file and reads its
metadata. It decodes JPEG pixels with jpeg-rs and HEIC and AVIF planes with hevc-rs and av1-rs. It is read only. It is a new crate (no Go
original). The oracles are libultrahdr, libjpeg-turbo, libavif (with dav1d), libheif (with libde265) and ExifTool (docs/parity.md).

```
 JPEG  ─► src/jpeg/scan.rs   markers: SOF, XMP, MPF, ISO APP2, EOI
          src/jpeg/mpf.rs    MPF index ──┐
          src/xmp.rs         GContainer ─┼─► candidates ─► first gain map ─┐
          primary EOI ───────────────────┘                                 ├─► GainMapImage
 HEIF  ─► isobmff (items, iref, altr) ─► src/heif.rs ──────────────────────┘
          tmap payload ─► src/iso.rs (one ISO parser for JPEG and HEIF)
 decode ─► src/decode.rs ─► jpeg::decode (base RGBA, gain map gray or RGBA)
                              hevc::decode_heic_item, av1::decode_avif_item (coded planes)
```

## Rules for all code

- `#![forbid(unsafe_code)]`. No threads of its own (docs/threading.md).
- No panics on any input: `get`, `checked_*`, no unchecked indexing of input.
- Every buffer sized from the input is charged to `src/budget.rs` before it is allocated, and
  released when it is dropped. Nested calls get `Budget::remaining()` as their `max_alloc`.
- The cancel flag is checked per candidate, every 1024 JPEG markers and stuffed bytes, and by the
  nested crates.

## Public API

```rust
pub fn read(data: &[u8], limits: &Limits, cancel: &AtomicBool) -> Result<GainMapImage, Error>;
pub fn decode(data: &[u8], image: &GainMapImage, limits: &Limits, cancel: &AtomicBool)
    -> Result<Decoded, Error>;
```

`read` decodes nothing. It returns where the coded images are (`CodedImage::location`: a byte
range for JPEG, an item id for HEIF), their size and component count, and the gain-map metadata.
`GainMapImage::coded_bytes` gives the bytes; `GainMapImage::heif` gives the whole HEIF graph (items,
transforms, colours, auxiliary images).

## Locate order (JPEG)

The primary image is scanned from the SOI to its EOI. Candidates for the gain map are collected
in this order, at most `max_images`, and each one must start with an SOI:

1. The MPF entries after the first (src/jpeg/mpf.rs). Offsets are relative to the MP endian
   field. A bad MPF is the issue `mpf-malformed`; the next locator still runs.
2. The Google container directory in the primary XMP: items after `Primary`, at the primary EOI
   plus the primary padding plus the length and padding of each earlier item.
3. The JPEG right after the primary EOI, when the primary has `hdrgm:Version` or an ISO APP2.

The end of a candidate is where its own walk finds the EOI; the MPF entry size is not used, so a
size that is too small does not drop the gain map. The first candidate whose own XMP has an
`hdrgm` property other than `Version` (`Version` alone marks a primary image) or an Apple
`HDRGainMap` property, or whose ISO APP2 holds more than the 4 version bytes, is the gain map. If
there is none, the first candidate that the container directory marks `GainMap` is used. Else the
file has no gain map.

`exif::Xmp` reports only the attributes of `rdf:Description`, and the directory items carry
`Item:Semantic`, `Item:Length` and `Item:Padding` as attributes of `Container:Item`, so
src/xmp.rs reads those start tags itself (local names only). The element form of the items is not
read.

## Locate (HEIF and AVIF)

The base is the primary item; when the primary item is a `tmap` item, the base is the first
input of its `dimg` reference. The gain map comes from isobmff-rs (`Image::gain_map`): a `tmap`
item in an `altr` group, an Apple `auxl` image, or an Adobe `auxl` image (found in src/heif.rs from the
`auxl` references and `auxC`, which isobmff-rs does not look for). `tmap` payload: a version byte (must be 0, else
the issue `tmap-version`), then the ISO 21496-1 payload (src/iso.rs). The XMP items of the gain
map are read too: Apple values, and `hdrgm` values when present. The container is `Avif` when the
`ftyp` major or a compatible brand is `avif` or `avis` and the base is AV1, else `Heif`.
The base or the gain map may also be a `grid` (the codec is that of its first tile) or an `iden` item
(the codec of its input).

## Metadata precedence

1. The ISO 21496-1 payload of the gain-map image, when it parses and `Metadata::from_iso`
   accepts it (`minimum_version` 0, no zero denominator).
2. The `hdrgm` XMP of the gain-map image, when `Metadata::from_xmp` accepts it (it needs
   `GainMapMax` and `HDRCapacityMax`; a sequence has 1 or 3 values).
3. `None`.

`GainMap::iso`, `xmp` and `apple` keep what was stored, whatever was used. `Kind` is `UltraHdr`
when the primary or the gain-map XMP has `hdrgm`, else `Iso` when there is an ISO payload (or a
`tmap` item), else `Apple`.

## Never drop the gain map

When a gain-map image is found but its metadata is missing or invalid, `read` still returns
`Some(GainMap)` with `metadata: None` and the reasons in `issues`. A bad XMP packet is the issue
`xmp-malformed`, not an error. The issue tokens are stable: `iso-minimum-version`,
`iso-truncated`, `iso-zero-denominator`, `iso-gamma-zero`, `xmp-missing-gainmapmax`, `xmp-missing-hdrcapacitymax`,
`xmp-bad-count`, `xmp-bad-number`, `xmp-malformed`, `mpf-malformed`, `tmap-version`,
`tmap-not-preferred`, `tmap-too-large`, `tmap-unreadable`, `xmp-unreadable`, `gainmap-image-unreadable`, `metadata-missing`. Only a limit,
an allocation failure or the cancel flag ends `read` early.

## Decode

`decode` decodes the base and then the gain map, serially, each as a `Picture`:

- **JPEG:** `Picture::Pixels`, the base as RGBA and the gain map as gray (1 component) or RGBA (3
  components). A JPEG that decodes with an error beside its pixels (truncated scan) is
  `Error::Jpeg`: parity needs whole images.
- **HEIC and AVIF:** `Picture::Planes`, the coded Y, Cb and Cr planes of one item (a `hvc1`, `av01`
  or `iden` item, or a `grid` of tiles assembled and cropped to the grid size) at their coded bit
  depth (per plane: HEVC can code luma and chroma at different depths) and chroma format. The sample
  vectors are moved out of hevc-rs and av1-rs, never copied. `Planes` also carry the `colr`
  properties of the item as stored (an ICC profile stays an extent of the input), the codec CICP
  (HEVC VUI, `None` when the SPS has none; AV1 `color_config`) and the codec metadata (HEVC SEI
  messages, AV1 metadata OBUs), raw. Nothing is dropped by `decode` (metadata rule: keep) except
  the SEI past the 1 MiB `hevc` keeps per picture; `decode` has no
  options parameter, so there is no way to drop the metadata yet.

Budget: the base stays charged while the gain map decodes, so the gain map gets the rest of
`max_alloc`. Each codec call gets `remaining()` as its own `max_alloc` (av1-rs counts
`max_metadata` on top of `max_alloc`, so it is carved out: `min(16 MiB, remaining / 16)`), and the
planes and metadata it returns are charged when they come back. `max_pixels` is the smaller of ours
and 2^28, the cap hevc-rs and av1-rs were built and fuzzed with; their width and height defaults
apply. `max_threads` goes to jpeg-rs and av1-rs; hevc-rs decodes on the calling thread. The cancel
flag is checked between the two decodes and inside each codec. With the Cargo feature `heif` or
`avif` off, that codec gives `Unsupported("hevc decode")` or `Unsupported("av1 decode")`. The
errors of hevc-rs and av1-rs are `Error::Hevc` and `Error::Av1` (class tokens `hevc`, `av1`);
their limit, out-of-memory and cancel errors map to ours (av1-rs reports isobmff limits as
`Container("limit-alloc")` and the like, which map too).

### Why the transforms are not applied

Each `Planes` lists its `clap`, `irot` and `imir` in `ipma` order and applies none of them:

1. libheif applies each transform as it meets it and, on odd geometry, first converts 4:2:0 and
   4:2:2 to 4:4:4 with its colour converter (crop with an odd left or top, rotation of an odd
   width or height, mirroring an odd dimension). The output format would depend on the geometry,
   and matching it bit for bit means porting libheif's chroma upsampler.
2. libheif rounds a `clap` (left down, top to nearest, right and bottom after clamping);
   isobmff-rs `Clap::crop` and libavif accept only an exact integer rectangle.
3. libavif never applies them, it reports them.

The caller applies them where RGB conversion has already ended bit-exactness, which is the later
HDR rendering step.

### Which item is the base and which is the gain map

| Layout | Base | Gain map |
|---|---|---|
| libavif writer | `pitm` (or the first `dimg` input of a primary `tmap`) | the second `dimg` input of a `tmap` in an `altr` group before the base; may be a grid or an `iden` item |
| Apple HEIC | `pitm`, normally a grid | the `auxl` item of type `urn:com:apple:photo:2020:aux:hdrgainmap` |
| Adobe AVIF | `pitm` | the `auxl` item of type `urn:com:photo:aux:hdrgainmap`; `Kind::UltraHdr` when its XMP has `hdrgm`, else `Kind::Apple` (metadata `None`, issue `metadata-missing`) |

An `iden` base or gain map takes its codec from its input (a chain of up to 8). Transforms stay on
the item they are stored on: libavif requires the gain map's to equal the base's, libheif uses
each item's own; gainmap returns both lists as stored. hevc-rs decodes only 4:2:0 and 4:0:0, so a
4:2:2 or 4:4:4 HEVC gain map is `Error::Hevc(Unsupported)`.

## Not in v1

- **No writer.** There is no rewrite helper. exif-rs `strip` keeps the MPF trailer (and with it
  the gain map) of a JPEG, so a caller that strips metadata keeps the gain map; a writer that
  moves bytes would have to fix the MPF offsets and the container lengths.
- **No HDR rendering.** Applying a gain map needs RGB conversion, the transforms and the formula
  of the Android spec (`weight`, `(sdr + offset_sdr) * 2^(log_boost * weight) - offset_hdr`).
  Neither the Android spec v1.1 nor ISO 21496-1 (from its table of contents) defines a numeric
  tolerance: the gain-map resampling is left to the implementation ("bilinear or better"). So
  only bit-exact output could meet a conformance criterion, and that is reachable on one target
  only, x86_64 Linux with glibc in CI (libultrahdr and libavif call libm per pixel, and their
  aarch64 builds fuse multiply-adds, which Rust never does). It would also be two ports that
  disagree by design (libultrahdr's Shepard IDW with lookup tables, libavif's libyuv resize and
  clamped linear output), each copying its library's quirks. Left for when a consumer exists; the
  first step would be libultrahdr-compat on x86_64 only.
