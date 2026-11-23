# Safety

- `#![forbid(unsafe_code)]` at the crate root. There is no `unsafe` anywhere in the crate. (The
  counting allocator in tests/resources.rs needs `unsafe impl GlobalAlloc`; integration tests are
  separate crates, so the `forbid` does not reach them.)
- No SIMD: the crate walks markers and boxes and parses small payloads. The pixel work is in
  jpeg-rs, which has its own kernels and safety notes.
- Non-test code denies `clippy::unwrap_used`, `expect_used`, `panic`, `todo`, `unimplemented`
  (`cfg_attr(not(test), deny(...))` in src/lib.rs). CI runs clippy with `-D warnings` on all
  targets.

## Hostile input

Every input is untrusted: file bytes, MPF offsets, XMP, ISO payloads, HEIF boxes.

- The JPEG walk is bounded by `max_segments`, checks every length against the slice, and allocates
  nothing. A candidate is walked from its own start, so a hostile MPF cannot make the walk loop:
  at most `max_images` candidates, each a linear walk.
- Every offset and size from the input goes through `get`, `checked_add` or `try_from`. A bad MPF
  index, a bad XMP packet or a candidate that is not a JPEG is skipped or becomes an issue; only a
  limit, an allocation failure or the cancel flag is a hard error after the primary image.
- The ISO reader has fixed-size reads and returns `Truncated` at the first missing byte. It
  allocates nothing.
- Budgeted allocation: buffers sized from the input are charged against `max_alloc` before they
  are allocated (limits.md), with `try_reserve_exact`.
- The XMP reader is exif-rs: no DTD, no external entities.
- Cancel flag checked on every loop that can run long.
- No panics: the fuzz targets (docs/fuzzing.md) run with `-malloc_limit_mb=512`, so an allocation
  outside the limits is a crash too.
