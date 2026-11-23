#![no_main]
use libfuzzer_sys::fuzz_target;
use std::sync::atomic::AtomicBool;

#[path = "../common.rs"]
mod common;

// data = the file; `read` twice (the result must not depend on anything but the input), then the
// checks of common::check_read on a success.
fuzz_target!(|data: &[u8]| {
    let lim = common::small_limits();
    let cancel = AtomicBool::new(false);
    let first = gainmap::read(data, &lim, &cancel);
    if let Ok(image) = &first {
        common::check_read(image, data, &lim);
    }
    assert_eq!(
        first,
        gainmap::read(data, &lim, &cancel),
        "read is not deterministic"
    );
});
