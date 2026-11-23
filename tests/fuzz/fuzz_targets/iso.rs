#![no_main]
use libfuzzer_sys::fuzz_target;

#[path = "../common.rs"]
mod common;

// data = an ISO 21496-1 payload from minimum_version on.
fuzz_target!(|data: &[u8]| {
    if let Ok(m) = gainmap::IsoMetadata::parse(data, &common::small_limits()) {
        common::check_iso(&m);
    }
});
