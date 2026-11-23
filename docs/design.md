# Design

gainmap-rs finds the gain map in an Ultra HDR, ISO 21496-1 or Apple HDR file and reads its
metadata. It decodes JPEG pixels with jpeg-rs. It is read only. It is a new crate (no Go
original). The oracles are libultrahdr, libjpeg-turbo, libavif and ExifTool (docs/parity.md).

```
 JPEG  ─► src/jpeg/scan.rs   markers: SOF, XMP, MPF, ISO APP2, EOI
          src/jpeg/mpf.rs    MPF index ──┐
          src/xmp.rs         GContainer ─┼─► candidates ─► first gain map ─┐
          primary EOI ───────────────────┘                                 ├─► GainMapImage
 HEIF  ─► isobmff (items, iref, altr) ─► src/heif.rs ──────────────────────┘
          tmap payload ─► src/iso.rs (one ISO parser for JPEG and HEIF)
 decode ─► src/decode.rs ─► jpeg::decode (base RGBA, gain map gray or RGBA)
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
`GainMapImage::coded_bytes` gives the bytes; `GainMapImage::heif` gives the whole HEIF graph for a
later HEVC or AV1 layer.

## Locate order (JPEG)

The primary image is scanned from the SOI to its EOI. Candidates for the gain map are collected
in this order, at most `max_images`, and each one must start with an SOI:

1. The MPF entries after the first (src/jpeg/mpf.rs). Offsets are relative to the MP endian
   field. A bad MPF is the issue `mpf-malformed`; the next locator still runs.
2. The Google container directory in the primary XMP: items after `Primary`, at the primary EOI
   plus the primary padding plus the length and padding of each earlier item.
3. The JPEG right after the primary EOI, when the primary has `hdrgm:Version` or an ISO APP2.

The first candidate whose own XMP has `hdrgm:GainMapMax` or an Apple `HDRGainMap` property, or
whose ISO APP2 holds more than the 4 version bytes, is the gain map. If there is none, the first
candidate that the container directory marks `GainMap` is used. Else the file has no gain map.

`exif::Xmp` reports only the attributes of `rdf:Description`, and the directory items carry
`Item:Semantic`, `Item:Length` and `Item:Padding` as attributes of `Container:Item`, so
src/xmp.rs reads those start tags itself (local names only). The element form of the items is not
read.

## Locate (HEIF and AVIF)

The base is the primary item; when the primary item is a `tmap` item, the base is the first
input of its `dimg` reference. The gain map comes from isobmff-rs (`Image::gain_map`): a `tmap`
item in an `altr` group, or an Apple `auxl` image. `tmap` payload: a version byte (must be 0, else
the issue `tmap-version`), then the ISO 21496-1 payload (src/iso.rs). The XMP items of the gain
map are read too: Apple values, and `hdrgm` values when present. The container is `Avif` when the
`ftyp` major or a compatible brand is `avif` or `avis` and the base is AV1, else `Heif`.

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
`iso-truncated`, `iso-zero-denominator`, `xmp-missing-gainmapmax`, `xmp-missing-hdrcapacitymax`,
`xmp-bad-count`, `xmp-bad-number`, `xmp-malformed`, `mpf-malformed`, `tmap-version`,
`tmap-unreadable`, `xmp-unreadable`, `gainmap-image-unreadable`, `metadata-missing`. Only a limit,
an allocation failure or the cancel flag ends `read` early.

## Decode

`decode` decodes the base to RGBA and the gain map to gray (1 component) or RGBA (3 components),
serially, base first. The base pixels stay charged while the gain map decodes, so the gain map
gets the rest of `max_alloc`. A JPEG that decodes with an error beside its pixels (truncated scan)
is `Error::Jpeg`: parity needs whole images. HEIF and AVIF give `Unsupported("hevc decode")` or
`Unsupported("av1 decode")` until hevc-rs and av1-rs exist; the `Codec` switch is already in the
API.

## Not in v1

- **No writer.** There is no rewrite helper. exif-rs `strip` keeps the MPF trailer (and with it
  the gain map) of a JPEG, so a caller that strips metadata keeps the gain map; a writer that
  moves bytes would have to fix the MPF offsets and the container lengths.
- **No HDR rendering.** Applying a gain map exactly as libultrahdr does needs its gain LUT and
  its Shepard-IDW sampling to match; that is a lot of work for no use yet. Plan: a separate
  `apply` module that takes `Pixels`, `Metadata` and a display boost, with the formula of the
  Android spec (`weight`, `(sdr + offset_sdr) * 2^(log_boost * weight) - offset_hdr`), first
  without the LUT, then tested against libultrahdr with a tolerance, then bit-exact if it pays.
- **HEIF and AVIF pixels** wait for hevc-rs and av1-rs.
