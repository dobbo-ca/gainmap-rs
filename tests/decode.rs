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
    let b = d.base.as_pixels().unwrap();
    assert_eq!((b.width, b.height, b.channels), (16, 16, 4));
    assert_eq!(b.data.len(), 16 * 16 * 4);
    assert!(b.data.chunks(4).all(|p| p[3] == 255));
    let g = d.gain_map.unwrap();
    let g = g.as_pixels().unwrap();
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
    let g = g.as_pixels().unwrap();
    assert_eq!((g.width, g.height, g.channels), (8, 8, 4));
    assert_eq!(g.data.len(), 8 * 8 * 4);
}

#[test]
fn image_without_gain_map() {
    let data = jpeg_rgb(16, 16);
    let img = gainmap::read(&data, &limits(), &no_cancel()).unwrap();
    let d = gainmap::decode(&data, &img, &limits(), &no_cancel()).unwrap();
    assert_eq!((d.base.width(), d.base.height()), (16, 16));
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
    let base = d.base.as_pixels().unwrap();
    for y in 0..16 {
        let at = y * rgba.stride;
        assert_eq!(
            base.data[y * 64..(y + 1) * 64],
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

// The HEIF and AVIF files of tests/common have items that are not valid HEVC or AV1 streams: the
// decode reaches the codec, which refuses them. Pixel parity with libheif and libavif is the job
// of tests/parity (docs/parity.md).
#[test]
fn heif_items_that_are_not_streams_are_codec_errors() {
    let data = avif_tmap(0);
    let img = gainmap::read(&data, &limits(), &no_cancel()).unwrap();
    let r = gainmap::decode(&data, &img, &limits(), &no_cancel());
    #[cfg(feature = "avif")]
    assert_eq!(r.map(|_| ()).unwrap_err().class(), "av1");
    #[cfg(not(feature = "avif"))]
    assert_eq!(r, Err(Error::Unsupported("av1 decode")));

    let data = heic_apple();
    let img = gainmap::read(&data, &limits(), &no_cancel()).unwrap();
    let r = gainmap::decode(&data, &img, &limits(), &no_cancel());
    #[cfg(feature = "heif")]
    assert_eq!(r.map(|_| ()).unwrap_err().class(), "hevc");
    #[cfg(not(feature = "heif"))]
    assert_eq!(r, Err(Error::Unsupported("hevc decode")));
}

#[test]
fn heif_decode_checks_cancel_first() {
    for data in [avif_tmap(0), heic_apple()] {
        let img = gainmap::read(&data, &limits(), &no_cancel()).unwrap();
        let cancel = std::sync::atomic::AtomicBool::new(true);
        assert_eq!(
            gainmap::decode(&data, &img, &limits(), &cancel),
            Err(Error::Cancelled)
        );
    }
}

#[test]
fn damaged_heif_files_never_panic_in_decode() {
    for data in [avif_tmap(0), heic_apple()] {
        let one = Limits {
            max_threads: 1,
            ..limits()
        };
        for n in 0..data.len() {
            if let Ok(img) = gainmap::read(&data[..n], &one, &no_cancel()) {
                let _ = gainmap::decode(&data[..n], &img, &one, &no_cancel());
            }
        }
        for i in 0..data.len() {
            let mut f = data.clone();
            f[i] ^= 0xFF;
            if let Ok(img) = gainmap::read(&f, &limits(), &no_cancel()) {
                let _ = gainmap::decode(&f, &img, &limits(), &no_cancel());
            }
        }
    }
}

// A grid base whose second tile is linked by more `tmap` items than `max_images`: `read` checks
// only the first tile, so `decode` has to count every tile before the codec calls `Heif::image`.
#[cfg(feature = "avif")]
#[test]
fn grid_tiles_are_counted_before_the_codec_runs() {
    let mut s = Heif::new(b"avif");
    s.primary = 5;
    let size = s.prop(ispe(128, 48));
    let tile = s.prop(ispe(64, 48));
    let cfg = s.prop(av1c());
    s.item(1, b"av01", &[1; 4], &[tile, cfg]);
    s.item(2, b"av01", &[2; 4], &[tile, cfg]);
    s.item(5, b"grid", &[0, 0, 0, 1, 0, 128, 0, 48], &[size]);
    s.link(b"dimg", 5, &[1, 2]);
    for k in 0..3 {
        s.item(10 + k, b"tmap", &tmap_payload(0), &[]);
        s.link(b"dimg", 10 + k, &[2]);
    }
    let data = s.build();
    let l = Limits {
        max_images: 2,
        ..limits()
    };
    let img = gainmap::read(&data, &l, &no_cancel()).unwrap();
    assert_eq!(
        gainmap::decode(&data, &img, &l, &no_cancel()).map(|_| ()),
        Err(Error::LimitExceeded(Limit::Images))
    );
}

// One tile id repeated across a wide grid is counted once, in one pass, and still fails when
// more `tmap` items than `max_images` link it.
#[cfg(feature = "avif")]
#[test]
fn repeated_grid_tile_is_counted() {
    let mut s = Heif::new(b"avif");
    s.primary = 5;
    let size = s.prop(ispe(12800, 48));
    let tile = s.prop(ispe(64, 48));
    let cfg = s.prop(av1c());
    s.item(1, b"av01", &[1; 4], &[tile, cfg]);
    s.item(5, b"grid", &[0, 0, 0, 199, 0x32, 0, 0, 48], &[size]);
    s.link(b"dimg", 5, &[1; 200]);
    for k in 0..3 {
        s.item(10 + k, b"tmap", &tmap_payload(0), &[]);
        s.link(b"dimg", 10 + k, &[1]);
    }
    let data = s.build();
    let l = Limits {
        max_images: 2,
        ..limits()
    };
    let img = gainmap::read(&data, &l, &no_cancel());
    // `read` may already refuse the base (it counts the first tile); either way it is `Images`.
    let r = img.and_then(|i| gainmap::decode(&data, &i, &l, &no_cancel()).map(|_| ()));
    assert_eq!(r, Err(Error::LimitExceeded(Limit::Images)));
}

// An `iden` base reaches the codec as its input: hevc refuses an `iden` item itself.
#[cfg(feature = "heif")]
#[test]
fn heic_iden_base_is_decoded_as_its_input() {
    let mut s = Heif::new(b"heic");
    s.primary = 3;
    let size = s.prop(ispe(64, 48));
    let cfg = s.prop(hvcc());
    s.item(1, b"hvc1", &[1; 4], &[size, cfg]);
    s.item(3, b"iden", &[], &[size]);
    s.link(b"dimg", 3, &[1]);
    let data = s.build();
    let img = gainmap::read(&data, &limits(), &no_cancel()).unwrap();
    let e = gainmap::decode(&data, &img, &limits(), &no_cancel())
        .map(|_| ())
        .unwrap_err();
    assert_eq!(e.class(), "hevc");
    assert!(!format!("{e:?}").contains("not HEVC"), "{e:?}");
}
