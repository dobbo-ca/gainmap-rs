#![no_main]
use libfuzzer_sys::fuzz_target;
use std::sync::atomic::AtomicBool;

use gainmap::{Limits, Picture, Samples};

#[path = "../common.rs"]
mod common;

/// `max_alloc` of the call, chosen by the low two bits of the first byte: a tiny one fails inside
/// the first buffer, the others in the second or not at all. Bit 2 picks 4 threads instead of 1,
/// so the parallel AV1 grid path runs.
const ALLOCS: [usize; 4] = [4 << 10, 64 << 10, 1 << 20, 64 << 20];

// data = [selector byte][file]; `read`, then `decode` under a small `max_alloc`. The buffers of a
// success are tightly packed and never exceed `max_alloc` together.
fuzz_target!(|data: &[u8]| {
    let Some((sel, file)) = data.split_first() else {
        return;
    };
    let lim = Limits {
        max_alloc: ALLOCS[usize::from(sel & 3)],
        max_threads: if sel & 4 == 0 { 1 } else { 4 },
        ..common::small_limits()
    };
    let cancel = AtomicBool::new(false);
    let Ok(image) = gainmap::read(file, &lim, &cancel) else {
        return;
    };
    let Ok(d) = gainmap::decode(file, &image, &lim, &cancel) else {
        return;
    };
    let mut total = 0usize;
    for p in std::iter::once(&d.base).chain(d.gain_map.as_ref()) {
        match p {
            Picture::Pixels(p) => {
                assert!(p.channels == 1 || p.channels == 4, "channels");
                let want = u64::from(p.width) * u64::from(p.height) * u64::from(p.channels);
                assert_eq!(
                    p.data.len() as u64,
                    want,
                    "pixel buffer is not tightly packed"
                );
                total += p.data.len();
            }
            Picture::Planes(p) => {
                for pl in &p.planes {
                    let (n, bytes) = match &pl.samples {
                        Samples::U8(v) => (v.len(), v.len()),
                        Samples::U16(v) => (v.len(), v.len() * 2),
                    };
                    assert_eq!(
                        n as u64,
                        u64::from(pl.width) * u64::from(pl.height),
                        "plane is not tightly packed"
                    );
                    total += bytes;
                }
            }
        }
    }
    assert!(total <= lim.max_alloc, "buffers over max_alloc");
});
