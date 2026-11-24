//! `decode_with` and `DecodeOptions::apply_transforms`. The tests have no HEVC or AV1 stream
//! (the repo has no binary fixtures), so the transform step on real planes is covered by the unit
//! tests in src/transforms.rs; here the default, JPEG, and the paths that run before the codec.

mod common;

use common::*;
use gainmap::{DecodeOptions, GainMapImage, Limits};

fn jpeg_file() -> (Vec<u8>, GainMapImage) {
    let primary = vec![app1_xmp(&primary_xmp())];
    let b = with_mpf(&primary, &jpeg_rgb(16, 16), &jpeg_gray(8, 8), true);
    let img = gainmap::read(&b.file, &limits(), &no_cancel()).unwrap();
    (b.file, img)
}

#[test]
fn the_default_is_raw() {
    assert!(!DecodeOptions::default().apply_transforms);
}

#[test]
fn default_decode_with_equals_decode() {
    let (data, img) = jpeg_file();
    let a = gainmap::decode(&data, &img, &limits(), &no_cancel()).unwrap();
    let b = gainmap::decode_with(
        &data,
        &img,
        &DecodeOptions::default(),
        &limits(),
        &no_cancel(),
    )
    .unwrap();
    assert_eq!(a, b);
}

#[test]
fn jpeg_pictures_are_unaffected() {
    let (data, img) = jpeg_file();
    let on = DecodeOptions {
        apply_transforms: true,
    };
    let a = gainmap::decode(&data, &img, &limits(), &no_cancel()).unwrap();
    let b = gainmap::decode_with(&data, &img, &on, &limits(), &no_cancel()).unwrap();
    assert_eq!(a, b);
}

#[test]
fn cancel_and_limits_apply_to_decode_with() {
    let (data, img) = jpeg_file();
    let on = DecodeOptions {
        apply_transforms: true,
    };
    let cancel = std::sync::atomic::AtomicBool::new(true);
    assert_eq!(
        gainmap::decode_with(&data, &img, &on, &limits(), &cancel).map(|_| ()),
        Err(gainmap::Error::Cancelled)
    );
    let tiny = Limits {
        max_alloc: 1024,
        ..limits()
    };
    assert_eq!(
        gainmap::decode_with(&data, &img, &on, &tiny, &no_cancel()).map(|_| ()),
        Err(gainmap::Error::LimitExceeded(gainmap::Limit::Alloc))
    );
}

/// A grid base whose tile has an `irot`: raw decode reaches the codec (the stream is a stub), with
/// the option the codec is never called.
#[cfg(feature = "avif")]
#[test]
fn grid_tile_transforms_are_unsupported_only_when_applying() {
    let mut s = Heif::new(b"avif");
    s.primary = 5;
    let size = s.prop(ispe(128, 48));
    let tile = s.prop(ispe(64, 48));
    let cfg = s.prop(av1c());
    let irot = s.prop(bx(b"irot", &[1]));
    s.item(1, b"av01", &[1; 4], &[tile, cfg, irot]);
    s.item(2, b"av01", &[2; 4], &[tile, cfg]);
    s.item(5, b"grid", &[0, 0, 0, 1, 0, 128, 0, 48], &[size]);
    s.link(b"dimg", 5, &[1, 2]);
    let data = s.build();
    let img = gainmap::read(&data, &limits(), &no_cancel()).unwrap();

    let raw = gainmap::decode(&data, &img, &limits(), &no_cancel()).unwrap_err();
    assert_eq!(raw.class(), "av1");

    let on = DecodeOptions {
        apply_transforms: true,
    };
    assert_eq!(
        gainmap::decode_with(&data, &img, &on, &limits(), &no_cancel()).map(|_| ()),
        Err(gainmap::Error::Unsupported("grid-tile-transforms"))
    );
}

/// With the option on and no tile transforms the codec runs as before (the stub stream fails
/// there, with the codec's own error).
#[cfg(feature = "heif")]
#[test]
fn apply_transforms_reaches_the_codec() {
    let mut s = Heif::new(b"heic");
    let size = s.prop(ispe(64, 48));
    let cfg = s.prop(hvcc());
    let irot = s.prop(bx(b"irot", &[1]));
    s.item(1, b"hvc1", &[1; 4], &[size, cfg, irot]);
    let data = s.build();
    let img = gainmap::read(&data, &limits(), &no_cancel()).unwrap();
    let on = DecodeOptions {
        apply_transforms: true,
    };
    let e = gainmap::decode_with(&data, &img, &on, &limits(), &no_cancel()).unwrap_err();
    assert_eq!(e.class(), "hevc");
}
