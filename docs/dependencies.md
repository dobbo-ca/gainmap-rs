# Dependencies

## Crate dependencies

Three direct dependencies, all owner crates, git dependencies pinned by rev in Cargo.toml.

| Crate | Source | Why |
|---|---|---|
| `jpeg` | `dobbo-ca/jpeg-rs` | Baseline and progressive JPEG decode of the base image and the gain map, under the remaining `max_alloc` and `max_threads`. Writing a decoder here would duplicate a crate we own and fuzz. |
| `exif` | `dobbo-ca/exif-rs` | XMP packet parse (`Xmp::parse`): the `hdrgm`, Apple `HDRGainMap` and container properties as text. |
| `isobmff` | `dobbo-ca/isobmff-rs` | HEIF and AVIF boxes: items, `iref`, `altr` groups, `auxC`, item data, and which item is the gain map. The `tmap` payload is read by src/iso.rs, the same parser as for JPEG. |

## Transitive dependencies

These are in Cargo.lock because of the three crates above. gainmap does not use them directly.

| Crate | Pulled in by | Why |
|---|---|---|
| `deflate` (`dobbo-ca/deflate-rs`, git, rev pinned) | `exif` | zlib inflate for PNG `zTXt` and compressed `iTXt` chunks. gainmap reads JPEG and HEIF only, so this code is never reached from here; it is built because `exif` has it as an unconditional dependency. Private repository, so CI fetches it with the same token. |
| `rayon` 1.12.0 | `jpeg` | The worker pool of the threaded decode (restart bands, colour stripes). It is a thread-pool crate: the pool belongs to `jpeg`, gainmap starts no thread of its own, and `max_threads = 1` never touches it (docs/threading.md). `jpeg` pins it exactly. |
| `rayon-core` 1.13.0, `crossbeam-deque` 0.8.8, `crossbeam-epoch` 0.9.21, `crossbeam-utils` 0.8.23, `either` 1.19.0 | `rayon` | The work-stealing deque, its memory reclamation and helpers. Registry crates; no choice, they are rayon's own dependencies. |

No C dependency, no `-sys` crate, no `cc`/`bindgen`, no proc macro, no XML crate.
CI fetches the private git dependencies with the `dobbo-ca-ci-read` GitHub App token
(`repositories: jpeg-rs,exif-rs,isobmff-rs,deflate-rs`), as in conventions.

## Ported code

`src/iso.rs` is a port of `decodeGainmapMetadata` and `gainmapMetadataFractionToFloat` from
libultrahdr v2.0.2 `lib/src/gainmapmetadata.cpp` (Copyright 2024 The Android Open Source Project,
MIT OR Apache-2.0). The attribution is in the file header and in NOTICE.

## Test-only

`libfuzzer-sys` lives only in `tests/fuzz` (the separate cargo-fuzz crate), never in the library.

## CI-only tools (never crate dependencies)

- libultrahdr v2.0.2 (MIT OR Apache-2.0): oracle for the metadata and the coded base and gain-map
  bytes of Ultra HDR JPEGs.
- libjpeg-turbo 3.2.0: oracle for the decoded pixels.
- libavif v1.4.2 (BSD-2-Clause): oracle for the AVIF gain map fractions.
- ExifTool 13.64 (GPL or Artistic): oracle for the XMP values. Read as a behaviour reference; no
  code or table is copied.

None of these is needed to build or use the crate.
