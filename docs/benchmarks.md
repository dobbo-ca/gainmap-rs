# Benchmarks

There is no speed bar: the numbers are information only and never fail a job.

`.github/workflows/bench.yml` (`workflow_dispatch` and push to `main`, not required) builds the CI
image and times, on the JPEG files of the parity corpus in one container:

- libultrahdr 2.0.2 with libjpeg-turbo 3.2.0 (`tests/parity/uhdr_oracle.cpp --no-raw`): probe of
  the file, then libjpeg-turbo decode of the base image and the gain map, one thread, one run,
  process start-up included;
- the Rust bench example (`examples/bench.rs`, `std::time::Instant`, no framework), which runs
  `read` and `decode` over the same files, with the files already in memory, best of 3 rounds,
  with one thread and with the default thread count.

HEIF and AVIF files are left out on both sides (no pixel decode yet). `tests/parity/bench.sh`
writes `bench.md`, and the job summary shows it.

jpeg-rs does the work of the decode; `gainmap` adds the marker walk, the MPF and XMP parse and
two buffer copies at most. A large gap to libjpeg-turbo is a jpeg-rs finding.

## Numbers

Main c285acb, `bench.yml` run 45726522411, on a GitHub-hosted `ubuntu-24.04` runner (the baseline for
later runs; information only).

| files | MB | decoded | read ms | decode ms (1 thread) | decode ms (N threads) | N |
|---|---|---|---|---|---|---|
| 31 | 119.7 | 31 | 47.4 | 2525.2 | 2211.8 | 2 |

| | wall ms |
|---|---|
| libultrahdr probe + libjpeg-turbo decode of base and gain map (1 thread) | 5634 |
| gainmap-rs `read` + `decode`, 1 thread | 2572.6 |
| gainmap-rs `decode`, 2 threads | 2211.8 (decode only; `read` 47.4) |
