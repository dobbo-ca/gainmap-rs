# gainmap-rs

Reads Ultra HDR and ISO 21496-1 gain maps from JPEG, HEIF and AVIF files: the SDR base image,
the gain-map image and the metadata that says how to combine them. JPEG pixels decode through
jpeg-rs; HEIF and AVIF pixels wait for hevc-rs and av1-rs. Pure Rust, no C dependencies,
`#![forbid(unsafe_code)]`, bounded on hostile input. Read only: no writer, no HDR rendering yet.

```toml
gainmap = { git = "https://github.com/dobbo-ca/gainmap-rs", rev = "<sha>" }
```

## Quick start

```rust
use std::sync::atomic::AtomicBool;
use gainmap::Limits;

let limits = Limits { max_threads: 1, max_alloc: 64 << 20, ..Limits::default() };
let cancel = AtomicBool::new(false);
let image = gainmap::read(file, &limits, &cancel)?;
if let Some(gm) = &image.gain_map {
    println!("{:?} {:?} {:?}", gm.kind, gm.metadata, gm.issues);
    let pixels = gainmap::decode(file, &image, &limits, &cancel)?; // JPEG only
}
```

## Resource limits

Every call takes `&Limits` and a cancel flag (`&AtomicBool`); there is no unlimited entry point.
Details: [docs/limits.md](docs/limits.md).

| Field | Default | Unit | Error (`Limit::...`) |
|---|---|---|---|
| `max_alloc` | 1 GiB | bytes, enforced peak | `Alloc` |
| `max_threads` | `available_parallelism` | threads | none |
| `max_pixels` | 2^30 | pixels per image | `Pixels` |
| `max_xmp_bytes` | 16 MiB | bytes | `XmpBytes` |
| `max_segments` | 100 000 | JPEG markers per image | `Segments` |
| `max_images` | 64 | MPF entries and candidate images | `Images` |

- `max_alloc` is the enforced peak, including the nested calls into jpeg-rs, exif-rs and
  isobmff-rs, which run under what is left.
- `max_threads = 1` spawns no thread. `read` never spawns one; `decode` passes the value to
  jpeg-rs.
- Set the cancel flag from another thread: the call returns `Error::Cancelled`.
- A gain map is never dropped: bad metadata gives `metadata: None` and the reason in `issues`.

HEVC is covered by patent pools; AV1 is royalty-free. Low risk for a private app; review before
anything public.

## Docs

[design](docs/design.md), [limits](docs/limits.md), [threading](docs/threading.md),
[safety](docs/safety.md), [dependencies](docs/dependencies.md), [testing](docs/testing.md),
[parity](docs/parity.md), [corpus](docs/corpus.md), [fuzzing](docs/fuzzing.md),
[benchmarks](docs/benchmarks.md).
