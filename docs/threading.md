# Threading

This crate has no threads of its own. It never calls `std::thread::spawn`, `thread::scope` or a
thread builder, and it has no pool. `read` runs on the calling thread. A test greps `src/` for the
three calls (tests/resources.rs).

`decode` passes `Limits::max_threads` to `jpeg::decode`, which may use up to that many threads
(restart bands, colour stripes), the calling thread included, from its own pool. `max_threads = 1`
runs on the calling thread and never touches a pool. The two images are decoded one after the
other, so at most one decode runs at a time: the thread count of a call is the thread count of
`jpeg`, never the sum. The output is the same for every `max_threads` (tests/decode.rs,
tests/resources.rs).

Why not decode the base and the gain map in parallel: the base is much larger than the gain map,
so the gain would be small, and both output buffers would be alive at once on top of the planes.
Callers that read many files parallelise across files.

Cancellation from another thread works: every entry point takes `&AtomicBool`, read with relaxed
loads at the points listed in limits.md. `Budget` is not `Sync`, by design: one budget belongs to
one call on one thread.
