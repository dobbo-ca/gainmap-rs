//! `decode` of a synthetic base image and gain map: sizes, channels, the serial result for every
//! `max_threads`, errors, and the HEIF placeholder.

mod common;

use common::*;
use gainmap::{Error, GainMapImage, Limit, Limits};

fn file(gm: &[u8]) -> (Vec<u8>, GainMapImage) {
    let primary = vec![app1_xmp(&primary_xmp())];
    let b = with_mpf(&primary, &jpeg_rgb(16, 16), gm, true);
    let img = gainmap::read(&b.file, &limits(), &no_cancel()).unwrap();
    (b.file, img)
}

#[test]
fn base_rgba_and_gray_gain_map() {
    let gm = with_segments(&jpeg_gray(8, 8), &[app1_xmp(&gm_xmp("2", "2.5"))]);
    let (data, img) = file(&gm);
    let d = gainmap::decode(&data, &img, &limits(), &no_cancel()).unwrap();
    assert_eq!((d.base.width, d.base.height, d.base.channels), (16, 16, 4));
    assert_eq!(d.base.data.len(), 16 * 16 * 4);
    assert!(d.base.data.chunks(4).all(|p| p[3] == 255));
    let g = d.gain_map.unwrap();
    assert_eq!((g.width, g.height, g.channels), (8, 8, 1));
    assert_eq!(g.data.len(), 8 * 8);
}

#[test]
fn rgb_gain_map_is_rgba() {
    let gm = with_segments(&jpeg_rgb(8, 8), &[app1_xmp(&gm_xmp("2", "2.5"))]);
    let (data, img) = file(&gm);
    assert_eq!(img.gain_map.as_ref().unwrap().image.components, 3);
    let d = gainmap::decode(&data, &img, &limits(), &no_cancel()).unwrap();
    let g = d.gain_map.unwrap();
    assert_eq!((g.width, g.height, g.channels), (8, 8, 4));
    assert_eq!(g.data.len(), 8 * 8 * 4);
}

#[test]
fn image_without_gain_map() {
    let data = jpeg_rgb(16, 16);
    let img = gainmap::read(&data, &limits(), &no_cancel()).unwrap();
    let d = gainmap::decode(&data, &img, &limits(), &no_cancel()).unwrap();
    assert_eq!((d.base.width, d.base.height), (16, 16));
    assert!(d.gain_map.is_none());
}

#[test]
fn the_result_does_not_depend_on_max_threads() {
    let gm = with_segments(&jpeg_gray(8, 8), &[app1_xmp(&gm_xmp("2", "2.5"))]);
    let (data, img) = file(&gm);
    let one = Limits {
        max_threads: 1,
        ..limits()
    };
    let want = gainmap::decode(&data, &img, &one, &no_cancel()).unwrap();
    for t in [2, 4, Limits::default().max_threads] {
        let l = Limits {
            max_threads: t,
            ..limits()
        };
        assert_eq!(
            gainmap::decode(&data, &img, &l, &no_cancel()).unwrap(),
            want
        );
    }
}

#[test]
fn decoded_pixels_match_the_jpeg_decoder() {
    // The base of `decode` is what `jpeg::decode` gives for the same bytes, tightly packed.
    let gm = with_segments(&jpeg_gray(8, 8), &[app1_xmp(&gm_xmp("2", "2.5"))]);
    let (data, img) = file(&gm);
    let bytes = img
        .coded_bytes(&data, &img.base, &limits(), &no_cancel())
        .unwrap();
    let o = jpeg::DecodeOptions {
        format: jpeg::Format::Rgba,
        ..jpeg::DecodeOptions::default()
    };
    let want = jpeg::decode(&bytes, &o, &jpeg::Limits::default(), &no_cancel()).unwrap();
    let jpeg::Image::Rgba(rgba) = want.image else {
        panic!("not rgba");
    };
    let d = gainmap::decode(&data, &img, &limits(), &no_cancel()).unwrap();
    for y in 0..16 {
        let at = y * rgba.stride;
        assert_eq!(
            d.base.data[y * 64..(y + 1) * 64],
            rgba.pix[at..at + 64],
            "row {y}"
        );
    }
}

#[test]
fn limits_and_cancel() {
    let gm = with_segments(&jpeg_gray(8, 8), &[app1_xmp(&gm_xmp("2", "2.5"))]);
    let (data, img) = file(&gm);
    let tiny = Limits {
        max_pixels: 10,
        ..limits()
    };
    assert_eq!(
        gainmap::decode(&data, &img, &tiny, &no_cancel()),
        Err(Error::LimitExceeded(Limit::Pixels))
    );
    let tiny = Limits {
        max_alloc: 1024,
        ..limits()
    };
    assert_eq!(
        gainmap::decode(&data, &img, &tiny, &no_cancel()),
        Err(Error::LimitExceeded(Limit::Alloc))
    );
    let cancel = std::sync::atomic::AtomicBool::new(true);
    assert_eq!(
        gainmap::decode(&data, &img, &limits(), &cancel),
        Err(Error::Cancelled)
    );
}

#[test]
fn wrong_data_is_an_error_not_a_panic() {
    let gm = with_segments(&jpeg_gray(8, 8), &[app1_xmp(&gm_xmp("2", "2.5"))]);
    let (data, img) = file(&gm);
    // The input is shorter than the ranges.
    assert_eq!(
        gainmap::decode(&data[..100], &img, &limits(), &no_cancel()),
        Err(Error::Truncated)
    );
    // Same size, but the bytes are not JPEG.
    let zeros = vec![0u8; data.len()];
    assert!(gainmap::decode(&zeros, &img, &limits(), &no_cancel()).is_err());
}

#[test]
fn heif_pixels_are_not_supported_yet() {
    let data = avif_tmap(0);
    let img = gainmap::read(&data, &limits(), &no_cancel()).unwrap();
    assert_eq!(
        gainmap::decode(&data, &img, &limits(), &no_cancel()),
        Err(Error::Unsupported("av1 decode"))
    );
    let data = heic_apple();
    let img = gainmap::read(&data, &limits(), &no_cancel()).unwrap();
    assert_eq!(
        gainmap::decode(&data, &img, &limits(), &no_cancel()),
        Err(Error::Unsupported("hevc decode"))
    );
}
