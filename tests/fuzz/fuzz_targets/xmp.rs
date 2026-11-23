#![no_main]
use libfuzzer_sys::fuzz_target;

#[path = "../common.rs"]
mod common;

// data = an XMP packet: the hdrgm values and their conversion to metadata.
fuzz_target!(|data: &[u8]| {
    let lim = common::small_limits();
    if let Ok(Some(h)) = gainmap::__fuzz::hdrgm_from_packet(data, &lim) {
        let _ = gainmap::Metadata::from_xmp(&h);
    }
});
