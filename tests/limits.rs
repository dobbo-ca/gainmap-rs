//! `Limits` defaults, `Error::class` tokens, every `Limit` variant reached through the public
//! API, and cancel.

mod common;

use std::sync::atomic::AtomicBool;

use common::*;
use gainmap::{Error, Limit, Limits};

fn ultra_hdr() -> Vec<u8> {
    let gm = with_segments(&jpeg_gray(8, 8), &[app1_xmp(&gm_xmp("2", "2.5"))]);
    with_mpf(&[app1_xmp(&primary_xmp())], &jpeg_rgb(16, 16), &gm, true).file
}

#[test]
fn defaults() {
    let d = Limits::default();
    assert_eq!(d.max_alloc, 1 << 30);
    assert_eq!(d.max_pixels, 1 << 30);
    assert_eq!(d.max_xmp_bytes, 16 << 20);
    assert_eq!(d.max_segments, 100_000);
    assert_eq!(d.max_images, 64);
    assert!(d.max_threads >= 1);
}

#[test]
fn alloc() {
    let data = ultra_hdr();
    let l = Limits {
        max_alloc: 64,
        ..limits()
    };
    assert_eq!(
        gainmap::read(&data, &l, &no_cancel()).map(|_| ()),
        Err(Error::LimitExceeded(Limit::Alloc))
    );
    // HEIF: the parsed graph does not fit.
    assert_eq!(
        gainmap::read(&avif_tmap(0), &l, &no_cancel()).map(|_| ()),
        Err(Error::LimitExceeded(Limit::Alloc))
    );
}

#[test]
fn pixels() {
    let data = ultra_hdr();
    let img = gainmap::read(&data, &limits(), &no_cancel()).unwrap();
    let l = Limits {
        max_pixels: 100,
        ..limits()
    };
    assert_eq!(
        gainmap::decode(&data, &img, &l, &no_cancel()).map(|_| ()),
        Err(Error::LimitExceeded(Limit::Pixels))
    );
}

#[test]
fn xmp_bytes() {
    let l = Limits {
        max_xmp_bytes: 16,
        ..limits()
    };
    assert_eq!(
        gainmap::read(&ultra_hdr(), &l, &no_cancel()).map(|_| ()),
        Err(Error::LimitExceeded(Limit::XmpBytes))
    );
    // The Apple XMP item of a HEIC.
    assert_eq!(
        gainmap::read(&heic_apple(), &l, &no_cancel()).map(|_| ()),
        Err(Error::LimitExceeded(Limit::XmpBytes))
    );
}

#[test]
fn segments() {
    let l = Limits {
        max_segments: 1,
        ..limits()
    };
    assert_eq!(
        gainmap::read(&ultra_hdr(), &l, &no_cancel()).map(|_| ()),
        Err(Error::LimitExceeded(Limit::Segments))
    );
}

#[test]
fn images() {
    let l = Limits {
        max_images: 1,
        ..limits()
    };
    assert_eq!(
        gainmap::read(&ultra_hdr(), &l, &no_cancel()).map(|_| ()),
        Err(Error::LimitExceeded(Limit::Images))
    );
    // The MPF hook has the default of 64.
    let entries: Vec<(u32, u32, u32)> = vec![(0, 0, 0); 65];
    assert_eq!(
        gainmap::__fuzz::mpf(&mpf_payload(true, &entries)),
        Err(Error::LimitExceeded(Limit::Images))
    );
    assert_eq!(
        gainmap::__fuzz::mpf(&mpf_payload(true, &entries[..64])),
        Ok(64)
    );
}

#[test]
fn cancel() {
    let cancel = AtomicBool::new(true);
    for data in [ultra_hdr(), avif_tmap(0)] {
        assert_eq!(
            gainmap::read(&data, &limits(), &cancel).map(|_| ()),
            Err(Error::Cancelled)
        );
    }
}

#[test]
fn class_tokens() {
    let nested_jpeg = Error::Jpeg(jpeg::Error::Truncated);
    let nested_heif = Error::Heif(isobmff::Error::NoMeta);
    let cases = [
        (Error::UnknownFormat, "unknown-format"),
        (Error::Truncated, "truncated"),
        (Error::Malformed("x"), "malformed"),
        (Error::Unsupported("x"), "unsupported"),
        (Error::LimitExceeded(Limit::Alloc), "limit-alloc"),
        (Error::LimitExceeded(Limit::Pixels), "limit-pixels"),
        (Error::LimitExceeded(Limit::XmpBytes), "limit-xmp-bytes"),
        (Error::LimitExceeded(Limit::Segments), "limit-segments"),
        (Error::LimitExceeded(Limit::Images), "limit-images"),
        (Error::OutOfMemory, "out-of-memory"),
        (Error::Cancelled, "cancelled"),
        (nested_jpeg, "jpeg"),
        (nested_heif, "heif"),
    ];
    for (e, class) in cases {
        assert_eq!(e.class(), class);
        assert!(!e.to_string().is_empty());
    }
}

#[test]
fn nested_errors_are_mapped() {
    assert_eq!(
        Error::from(jpeg::Error::AllocLimit { got: 2, max: 1 }),
        Error::LimitExceeded(Limit::Alloc)
    );
    assert_eq!(
        Error::from(jpeg::Error::PixelLimit { got: 2, max: 1 }),
        Error::LimitExceeded(Limit::Pixels)
    );
    assert_eq!(Error::from(jpeg::Error::OutOfMemory), Error::OutOfMemory);
    assert_eq!(Error::from(jpeg::Error::Cancelled), Error::Cancelled);
    assert_eq!(
        Error::from(jpeg::Error::NoSof),
        Error::Jpeg(jpeg::Error::NoSof)
    );
    assert_eq!(
        Error::from(isobmff::Error::LimitExceeded(isobmff::Limit::Alloc)),
        Error::LimitExceeded(Limit::Alloc)
    );
    assert_eq!(
        Error::from(isobmff::Error::LimitExceeded(isobmff::Limit::Pixels)),
        Error::LimitExceeded(Limit::Pixels)
    );
    assert_eq!(
        Error::from(isobmff::Error::LimitExceeded(isobmff::Limit::Items)),
        Error::Heif(isobmff::Error::LimitExceeded(isobmff::Limit::Items))
    );
    assert_eq!(Error::from(isobmff::Error::Cancelled), Error::Cancelled);
}

/// An AVIF file with base 1, gain map 2 and `tmaps` `tmap` items that all link [1, 2], and one
/// `altr` group per `groups` that holds the base. The primary is item 1, so no tmap is chosen.
fn many_links(tmaps: u32, groups: u32) -> Vec<u8> {
    let mut s = Heif::new(b"avif");
    let size = s.prop(ispe(64, 48));
    let small = s.prop(ispe(32, 24));
    let cfg = s.prop(av1c());
    s.item(1, b"av01", &[1; 4], &[size, cfg]);
    s.item(2, b"av01", &[2; 4], &[small, cfg]);
    for k in 0..tmaps {
        s.item(10 + k, b"tmap", &tmap_payload(0), &[]);
        s.link(b"dimg", 10 + k, &[1, 2]);
    }
    for k in 0..groups {
        s.group(b"altr", 100 + k, &[1, 2]);
    }
    s.build()
}

#[test]
fn images_bound_the_heif_gain_map_lookup() {
    let l = Limits {
        max_images: 2,
        ..limits()
    };
    // At the limit: read.
    assert!(gainmap::read(&many_links(2, 2), &l, &no_cancel()).is_ok());
    // One `tmap` item too many.
    assert_eq!(
        gainmap::read(&many_links(3, 0), &l, &no_cancel()).map(|_| ()),
        Err(Error::LimitExceeded(Limit::Images))
    );
    // One `altr` group too many.
    assert_eq!(
        gainmap::read(&many_links(0, 3), &l, &no_cancel()).map(|_| ()),
        Err(Error::LimitExceeded(Limit::Images))
    );
}
