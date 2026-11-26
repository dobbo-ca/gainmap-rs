# Fuzzing

`tests/fuzz` is a cargo-fuzz crate (run it with `--fuzz-dir tests/fuzz`). It is the only place
that depends on `libfuzzer-sys`. It has its own `[workspace]` and its own `Cargo.lock`. One
target per entry point. Each runs under small `Limits` (`tests/fuzz/common.rs`: `max_alloc` 64 MiB,
`max_pixels` 2^22, one thread), so one input stays far below `-malloc_limit_mb=512`; an
allocation outside the limits is a crash. Each target checks that the call returns (no panic, no
hang) and the invariants below. An error may be any variant; the targets do not match on it.

| Target | Entry point | Checks |
|---|---|---|
| `read` | `read` on arbitrary bytes, twice | the two results are equal; a JPEG location is a range inside the input, and `coded_bytes` returns exactly that range; the ISO and XMP metadata conversions can be called |
| `decode` | `read`, then `decode_with` | data is `[selector byte][file]`; bits 0-1 of the selector pick `max_alloc` from 4 KiB, 64 KiB, 1 MiB, 64 MiB, bit 2 picks 1 or 4 threads, bit 3 sets `DecodeOptions::apply_transforms`; a success has tightly packed buffers of 1 or 4 channels and no more bytes than `max_alloc` together |
| `iso` | `IsoMetadata::parse`, then `Metadata::from_iso` | channels 1 and 2 are copies of channel 0 unless the payload is multichannel; the flag accessors can be called |
| `mpf` | `__fuzz::mpf` on an MPF payload (from the endian field) | at most `max_images` entries |
| `xmp` | `__fuzz::hdrgm_from_packet` on an XMP packet, then `Metadata::from_xmp` | the tolerant XML reader ends, no panic on bad UTF-8, entities or nesting |
| `scan` | `__fuzz::jpeg_scan` on a JPEG image | the returned end is inside the input and just after an `FF D9` |

The `mpf`, `xmp` and `scan` entry points are in the `#[doc(hidden)] pub mod __fuzz` of the crate.

## Seeds

There are no binary fixtures in git. `tests/fuzz/make-seeds.sh <dir>` writes the seeds with
`printf`, byte by byte: Ultra HDR JPEGs with MPF in both byte orders, with the Google container
only, with the gain map just after the primary image, with XMP `rdf:Seq`, with an ISO payload
only, and truncated ones; a JPEG without a gain map; a small AVIF with a `tmap` item, and the
same with an XMP item (`mime`, `application/rdf+xml`, `cdsc` to the base image); an iPhone-style
HEIC (`hvc1` primary, an `hvc1` auxiliary image with `auxC`
`urn:com:apple:photo:2020:aux:hdrgainmap` and an `auxl` reference, and two XMP items: `hdrgm`
version for the primary, Apple `HDRGainMap` for the gain map). These reach the Apple gain-map
source, the XMP item size check and the primary-version read of the HEIF reader, which no
mutation of the AVIF seed would build (the `auxC` URN alone is 40 bytes). The AVIF and HEIC seeds
go to `read/` and `decode/`. The other targets get ISO payloads (one channel, three channels,
common denominator, truncated); MPF payloads; XMP packets (attributes, `rdf:Seq`, Apple, garbage). The image data of every JPEG is a 1x1 grey
baseline image, so the seeds decode. The fuzz stage of the CI image runs it with `/seeds`. With a corpus dir as the second argument it also copies four public HEIC and AVIF files (a `tmap` HEIC, a grid, a tone-mapped and an Adobe auxiliary gain map AVIF) as decode and read seeds, so the Planes branch of `decode` is reached; the Dockerfile fuzz stage fetches the corpus for that.

## CI

`.github/workflows/fuzz.yml` runs weekly, **Saturday 12:23 UTC** (`cron: '23 12 * * 6'`; the free
day-of-week slot at 12:23 after png-rs on Wednesday and exif-rs on Sunday), and on
`workflow_dispatch`; never on push or pull request. It has a matrix over the six targets with
`max-parallel: 2`. There is no arm64 job: the crate has no SIMD kernel, so the code is the same on
both architectures (jpeg-rs fuzzes its own kernels). Each target runs

```
cargo +nightly fuzz run --fuzz-dir tests/fuzz <target> <corpus dir> <seed dirs> -- \
  -max_total_time=$MAX_TOTAL_TIME -max_len=1048576 -timeout=60 -rss_limit_mb=2048 -malloc_limit_mb=512
```

The corpus a run grows is kept in the Actions cache, one entry per target, and the next run goes on
from it. Crash artifacts are uploaded as `crashers-<target>`.

Dispatch a 15-minute run per target:

```
gh workflow run fuzz.yml --ref <branch> -f max_total_time=900
```

The default is 600 s. The fuzz hours of a run are `max_total_time` x 6 targets.

## Locally

Do not run it on the owner's machine: CI only.
