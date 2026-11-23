# Limits

`Limits` (src/limits.rs) has public fields and a finite `Default`. Every public entry point takes
`&Limits` and `&AtomicBool`. A limit gives `Error::LimitExceeded(Limit::X)`; its class token (the
parity oracle writes it) is `limit-<field>`.

| Field | Default | Unit | `Limit` | Class token | Checked |
|---|---|---|---|---|---|
| `max_alloc` | 1 GiB | bytes, peak | `Alloc` | `limit-alloc` | `Budget::charge`, before each allocation sized from input; nested crates get the rest |
| `max_threads` | `available_parallelism()` (1 if unknown) | threads | none | none | passed to `jpeg` by `decode`; see threading.md |
| `max_pixels` | 2^30 | pixels per image | `Pixels` | `limit-pixels` | `jpeg` (from the SOF, before any allocation) and `isobmff` (`ispe`) |
| `max_xmp_bytes` | 16 MiB | bytes | `XmpBytes` | `limit-xmp-bytes` | `exif::Xmp::parse`; the size of a HEIF XMP item before it is read |
| `max_segments` | 100 000 | JPEG markers with a length, per image | `Segments` | `limit-segments` | src/jpeg/scan.rs, per marker |
| `max_images` | 64 | MPF entries; candidate images | `Images` | `limit-images` | src/jpeg/mpf.rs from the entry count, before the entries are read; the candidate list |

The other errors are not limits: `UnknownFormat`, `Truncated`, `Malformed(token)`,
`Unsupported(token)`, `OutOfMemory` (the allocator refused a size within the limits), `Cancelled`,
and the errors of the nested crates (`Jpeg`, `Heif`). The limit, out-of-memory and cancel errors of
`jpeg` and `isobmff` are mapped to ours (`AllocLimit` and `Limit::Alloc` to `Alloc`, `PixelLimit`
and `Limit::Pixels` to `Pixels`). Other `isobmff` limits (items, boxes, ...) stay `Error::Heif`.

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
  HEIF graph (a size estimate from its item, extent, property, reference and group counts), the
  decoded base pixels.

What is charged: the XMP packet and its properties, the `hdrgm` strings of the result, the
container items, the MPF entries, the candidate list, the HEIF graph, item bytes that need a copy,
decoded pixels. What is not: small fixed-size values and the `issues` list (a handful of
`&'static str`). tests/resources.rs measures the real peak with a counting allocator.

jpeg-rs treats `max_alloc` equal to 1 GiB as Go's unset default and may exceed it by the page
rounding of its ledger. The default `max_alloc` here is also 1 GiB, so a decode with nothing else
charged is in that mode; any other value is a hard peak.

## Cancel

`Budget::check_cancel` runs at the start of each call, per candidate image, every 1024 JPEG
markers and every 1024 stuffed bytes of entropy data, between the two decodes of `decode`, and
inside the nested crates (per MCU row, per XML node batch, per box batch). A set flag gives
`Error::Cancelled`.
