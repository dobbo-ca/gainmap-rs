#![no_main]
use libfuzzer_sys::fuzz_target;

#[path = "../common.rs"]
mod common;

// data = an ISO 21496-1 payload from minimum_version on.
fuzz_target!(|data: &[u8]| {
    let lim = common::small_limits();
    if let Ok(m) = gainmap::IsoMetadata::parse(data, &lim) {
        common::check_iso(&m, &lim);
    }
});
