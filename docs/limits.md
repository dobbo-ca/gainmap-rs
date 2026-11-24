# Limits

`Limits` (src/limits.rs) has public fields and a finite `Default`. `read`, `decode` and `GainMapImage::coded_bytes` take `&Limits` and a `&AtomicBool` cancel flag;
`IsoMetadata::parse`, `Metadata::from_iso` and `Metadata::from_xmp` take `&Limits` only (they allocate nothing and read at most their input).
A limit gives `Error::LimitExceeded(Limit::X)`; its class token (the
parity oracle writes it) is `limit-<field>`.

| Field | Default | Unit | `Limit` | Class token | Checked |
|---|---|---|---|---|---|
| `max_alloc` | 1 GiB | bytes, peak | `Alloc` | `limit-alloc` | `Budget::charge`, before each allocation sized from input; nested crates get the rest |
| `max_threads` | `available_parallelism()` (1 if unknown) | threads | none | none | passed to `jpeg` and `av1` by `decode` (`hevc` is serial); see threading.md |
| `max_pixels` | 2^30 | pixels per image | `Pixels` | `limit-pixels` | `read` for JPEG (the SOF of the base and of the chosen gain map, src/jpeg/mod.rs), `jpeg` in `decode` (from the SOF, before any allocation) and `isobmff` (`ispe`) |
| `max_xmp_bytes` | 16 MiB | bytes | `XmpBytes` | `limit-xmp-bytes` | `exif::Xmp::parse`; the size of a HEIF XMP item before it is read. exif's own XML node limit is reported as this error too |
| `max_segments` | 100 000 | JPEG markers outside entropy data, per image | `Segments` | `limit-segments` | src/jpeg/scan.rs, per segment, TEM or RST marker and fill byte (restart markers inside entropy data are not counted) |
| `max_images` | 64 | MPF entries; candidate images; HEIF `tmap` items that link an image and `altr` groups that hold it | `Images` | `limit-images` | src/jpeg/mpf.rs from the entry count, before the entries are read; the candidate list; src/heif.rs before each `Heif::image` call (one pass over the references and groups, cancel checked every 1024 steps), because `isobmff` looks for a gain map in time proportional to their product |

The other errors are not limits: `UnknownFormat`, `Truncated`, `Malformed(token)`,
`Unsupported(token)`, `OutOfMemory` (the allocator refused a size within the limits), `Cancelled`,
and the errors of the nested crates (`Jpeg`, `Heif`, `Hevc`, `Av1`). The limit, out-of-memory and cancel errors of
`jpeg`, `isobmff`, `hevc` and `av1` are mapped to ours (`AllocLimit` and `Limit::Alloc` to `Alloc`, `PixelLimit`
and `Limit::Pixels` to `Pixels`; the `WidthLimit` and `HeightLimit` of `hevc` and `av1` to `Pixels`, and the `MetadataLimit` of `av1`, which is carved out of `max_alloc`, to `Alloc`). Other `isobmff` limits (items, boxes, ...) stay `Error::Heif`.

## max_alloc and the budget

`max_alloc` is the peak working memory of one call, not a total. `src/budget.rs` keeps one ledger
per public call:

- `charge(n)` adds `n` to the bytes in use, or fails with `LimitExceeded(Alloc)` and charges
  nothing. The sum uses `checked_add`.
- `vec(n)` charges `n * size_of::<T>()` (checked), then `try_reserve_exact(n)`; if the allocator
  refuses, the charge is given back and the error is `OutOfMemory`. `string(s)` is a charged copy.
- `release(n)` gives bytes back when a buffer is dropped: a parsed XMP packet once its values are
  copied, the padded pixel buffer after it is repacked.
- `remaining()` is passed down: `exif::Limits.max_alloc`, `isobmff::Limits.max_alloc` and
  `jpeg::Limits.max_alloc` are what is left when the nested call starts. The pieces a nested call
  returns are charged when they come back: the parsed XMP (its raw bytes and properties), the
  HEIF graph (a size estimate from its item, extent, overlay input, property, reference and group
  counts), the decoded base pixels.

What is charged: the XMP packet and its properties, the `hdrgm` strings of the result, the
container items, the MPF entries, the candidate list, the HEIF graph, item bytes that need a copy,
decoded pixels. What is not: small fixed-size values and the `issues` list (a handful of
`&'static str`). tests/resources.rs measures the real peak with a counting allocator.

jpeg-rs treats a `max_alloc` of exactly 1 GiB as Go's unset default and may exceed it by the page
rounding of its ledger. `decode` never passes exactly that value: when the remaining budget is
1 GiB it passes 1 GiB minus one byte, so the nested cap is always a hard peak. `max_alloc` is
therefore the enforced peak for every value, the 1 GiB default included.

## HEIF and AVIF decode

`decode` runs `hevc::decode_heic_item` or `av1::decode_avif_item` on the item of the parsed graph:

- **Memory.** The codec gets `Budget::remaining()` as its `max_alloc`, so the peak stays within
  `max_alloc`. Each codec splits it itself: an eighth for the item bytes read through `isobmff`,
  the rest for the decoder, the grid canvas and the returned picture. `av1` books the metadata
  OBUs and the grid tile frames it keeps in that same ledger (all decoders of the call, not
  `max_metadata` per frame and per decoder), so `decode` carves nothing out for them. `hevc` books
  its SEI in its ledger too. The planes and the metadata that come back are charged to the budget,
  so the gain map decodes under what the base left. `oracle planes` checks on the whole corpus
  that half of what the result holds gives `LimitExceeded(Alloc)`.
- **Item graph.** The `isobmff::Image` values that are live at once (the one `decode` parses, the
  one the codec parses again, and for a grid the tile images: one for `hevc`, up to `max_threads`
  workers for `av1`) are charged with the same bound as `read` (`image_cost`), for
  `2 + min(max_threads, 64)` images, and released when the codec returns; the transforms and
  `colr` properties that move into `Planes` stay charged. The `max_images` count (`tmap` items
  and `altr` groups) runs for the item and, for a grid, for every distinct tile in one pass over
  the references and groups, because the codecs call `Heif::image` on each tile and that call
  has no cancel check.
- **`iden`.** An `iden` item is decoded as its input (a chain of up to 8, followed by `decode`
  before the codec runs). The planes carry the `iden` item's own transforms and `colr`
  properties; the input item's are not reported (read them from `GainMapImage::heif()`).
- **SEI.** `decode` passes `hevc` a `max_metadata` equal to what is left of `max_alloc`, so SEI
  is never dropped silently: it is kept, or the decode fails with `LimitExceeded(Alloc)`.
- **Pixels.** `max_pixels` is `min(ours, 2^28)`; `hevc` also has a width and height cap of 16384
  and `av1` one of 65536, left at their defaults.
- **Threads.** `max_threads` goes to `av1` (grid tiles decode on up to that many workers, and
  each worker gets its share); `hevc` is serial. The result does not depend on it (checked for 1,
  2 and 3 on the corpus).
- **Cancel.** Checked before the base, between the two decodes and by the codecs inside.

## Cancel

`Budget::check_cancel` runs at the start of each call, per candidate image, every 1024 JPEG
markers and every 1024 stuffed bytes of entropy data, between the two decodes of `decode`, and
inside the nested crates (per MCU row, per XML node batch, per box batch). A set flag gives
`Error::Cancelled`.
