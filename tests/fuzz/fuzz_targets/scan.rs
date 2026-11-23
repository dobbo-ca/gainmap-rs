#![no_main]
use libfuzzer_sys::fuzz_target;

#[path = "../common.rs"]
mod common;

// data = a JPEG image from its SOI: the marker walk ends just after an EOI inside the input.
fuzz_target!(|data: &[u8]| {
    if let Ok(end) = gainmap::__fuzz::jpeg_scan(data, &common::small_limits()) {
        assert!((4..=data.len()).contains(&end), "end outside the input");
        assert_eq!(data[end - 2..end], [0xFF, 0xD9], "end is not after an EOI");
    }
});
