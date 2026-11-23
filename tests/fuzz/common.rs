//! Shared by the targets (`#[path = "../common.rs"] mod common;`): the small limits they run
//! under, and the checks that hold for any input.
#![allow(dead_code)]

use std::sync::atomic::AtomicBool;

use gainmap::{GainMapImage, IsoMetadata, Limits, Location, Metadata};

/// Limits that keep one input well under the `-malloc_limit_mb=512` of the fuzz run, so an
/// allocation outside the limits is a crash.
pub fn small_limits() -> Limits {
    Limits {
        max_alloc: 64 << 20,
        max_threads: 1,
        max_pixels: 1 << 22,
        max_xmp_bytes: 1 << 20,
        max_segments: 10_000,
        max_images: 16,
    }
}

/// The checks on a successful `read` of `data`: JPEG locations are ranges inside the input,
/// `coded_bytes` returns them, and every metadata accessor can be called.
pub fn check_read(image: &GainMapImage, data: &[u8], lim: &Limits) {
    let cancel = AtomicBool::new(false);
    let gm = image.gain_map.as_ref();
    for img in [Some(&image.base), gm.map(|g| &g.image)]
        .into_iter()
        .flatten()
    {
        if let Location::Range(r) = &img.location {
            assert!(
                r.start < r.end && r.end <= data.len(),
                "range outside the input"
            );
            if let Ok(b) = image.coded_bytes(data, img, lim, &cancel) {
                assert_eq!(b.len(), r.len(), "coded_bytes and the range differ");
            }
        } else {
            let _ = image.coded_bytes(data, img, lim, &cancel);
        }
    }
    if let Some(gm) = gm {
        if let Some(iso) = &gm.iso {
            check_iso(iso, lim);
        }
        if let Some(x) = &gm.xmp {
            let _ = Metadata::from_xmp(x, lim);
        }
    }
    let _ = image.heif();
}

/// The checks on parsed ISO metadata.
pub fn check_iso(m: &IsoMetadata, lim: &Limits) {
    let _ = (
        m.is_multichannel(),
        m.use_base_colour_space(),
        m.backward_direction(),
        m.common_denominator(),
    );
    if !m.is_multichannel() {
        assert_eq!(m.channels[1], m.channels[0], "channel 1 is not a copy of 0");
        assert_eq!(m.channels[2], m.channels[0], "channel 2 is not a copy of 0");
    }
    let _ = Metadata::from_iso(m, lim);
}
