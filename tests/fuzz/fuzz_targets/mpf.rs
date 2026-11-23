#![no_main]
use libfuzzer_sys::fuzz_target;

// data = an MPF payload from the MP endian field on (after `MPF\0`).
fuzz_target!(|data: &[u8]| {
    if let Ok(entries) = gainmap::__fuzz::mpf(data) {
        // At most `max_images` of the default limits (64) are accepted.
        assert!(
            entries <= gainmap::Limits::default().max_images,
            "too many entries"
        );
    }
});
