# Testing

All builds and tests run in CI (nothing runs on the owner's Mac). The tests build their inputs in
code: valid baseline JPEGs come from the `jpeg` encoder, then XMP, ISO 21496-1 and MPF segments
are added around them (tests/common/mod.rs). No binary fixtures.

## Test files

| File | Covers |
|---|---|
| `src/iso.rs` (unit) | the 4-byte version-only payload |
| `tests/iso.rs` | `IsoMetadata::parse`: both denominator modes, 1 and 3 channels, truncation at every byte, trailing bytes, version-only, flags; `Metadata::from_iso` values, minimum version, zero denominator |
| `tests/xmp.rs` | `hdrgm` XMP: attribute form, `rdf:Seq` with 1 and 3 values, defaults, `BaseRenditionIsHDR`, every error token, no `hdrgm`, not XML |
| `tests/jpeg.rs` | locate by MPF (big and little endian, several images), container directory with padding, directory without gain-map XMP, the fallback after the primary EOI, ISO-only files, no gain map, ISO over XMP, invalid ISO then XMP, never-drop cases, Apple XMP, bad MPF falling through, entries over `max_images`, truncation and byte flips of a whole file |
| `tests/decode.rs` | `decode`: sizes and channels (gray and RGBA gain map), same result for every `max_threads`, equal to `jpeg::decode`, limits, cancel, wrong data, HEIF placeholder errors |
| `tests/heif.rs` | AVIF with a `tmap` primary, `tmap` version, Apple HEIC with XMP, no gain map, truncation and byte flips |
| `tests/limits.rs` | `Limits::default()`, `Error::class` tokens, every `Limit` variant through the public API, cancel, the mapping of `jpeg` and `isobmff` errors |
| `tests/resources.rs` | counting global allocator: peak within `max_alloc` + slack for `read` and `decode` at several caps and for 1 and 2 threads; the nested JPEG decode stays under `max_threads` (`jpeg::decode_probe`); no thread spawn in `src/` |

Doc-tests (the crate-level example in `src/lib.rs`) are compiled by `cargo test`.

The parity tests, the fuzz targets, the corpus and the benchmarks are in docs/parity.md,
docs/fuzzing.md, docs/corpus.md and docs/benchmarks.md.

## CI jobs (`.github/workflows/ci.yml`)

- `test`: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`, and the
  aarch64 release build. Runs on pull requests; the only required check.
- `arm64`: `cargo test` for aarch64 under `qemu-aarch64`. Push to `main` and dispatch only.
- `parity`: the C and C++ oracles against `examples/oracle.rs` on the same inputs.
- `parity-arm64`: the oracle example under qemu on a subset of the inputs.
- `fuzz` and `bench`: weekly or on dispatch.

The crate is done when `arm64`, `parity` and `parity-arm64` are green on `main`.
