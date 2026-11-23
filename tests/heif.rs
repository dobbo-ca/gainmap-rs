//! HEIF and AVIF through `read`: the `tmap` item, the Apple auxiliary image, the base image of a
//! `tmap` primary, and damaged files.

mod common;

use common::*;
use gainmap::{Codec, Container, Kind, Location};

fn read(data: &[u8]) -> Result<gainmap::GainMapImage, gainmap::Error> {
    gainmap::read(data, &limits(), &no_cancel())
}

#[test]
fn avif_tmap_gain_map() {
    let data = avif_tmap(0);
    let img = read(&data).unwrap();
    assert_eq!(img.container, Container::Avif);
    // The primary item is the `tmap` item; the base is its first input.
    assert_eq!(img.base.location, Location::Item(1));
    assert_eq!(img.base.codec, Codec::Av1);
    assert_eq!(
        (img.base.width, img.base.height, img.base.components),
        (64, 48, 3)
    );
    let g = img.gain_map.as_ref().unwrap();
    assert_eq!(g.kind, Kind::Iso);
    assert_eq!(g.image.location, Location::Item(2));
    assert_eq!((g.image.width, g.image.height), (32, 24));
    assert!(g.xmp.is_none() && g.apple.is_none() && g.primary_version.is_none());
    assert!(g.issues.is_empty(), "{:?}", g.issues);
    let iso = g.iso.unwrap();
    assert!(iso.common_denominator());
    let m = g.metadata.as_ref().unwrap();
    assert_eq!(m.alternate_hdr_headroom, 3.0);
    assert_eq!(m.gain_map_max, [2.0; 3]);
    assert_eq!(m.gamma, [1.0; 3]);
    assert_eq!(m.base_offset, [15f32 / 1000f32; 3]);
    assert!(!m.multichannel && m.use_base_colour_space == iso.use_base_colour_space());
    // The coded bytes of the two items, and the graph.
    let base = img
        .coded_bytes(&data, &img.base, &limits(), &no_cancel())
        .unwrap();
    assert_eq!(&*base, &[1u8; 4]);
    let gm = img
        .coded_bytes(&data, &g.image, &limits(), &no_cancel())
        .unwrap();
    assert_eq!(&*gm, &[2u8; 4]);
    assert_eq!(img.heif().unwrap().primary, 3);
}

#[test]
fn tmap_with_an_unknown_version_keeps_the_gain_map() {
    let img = read(&avif_tmap(1)).unwrap();
    let g = img.gain_map.unwrap();
    assert!(g.metadata.is_none() && g.iso.is_none());
    assert!(g.issues.contains(&"tmap-version"));
}

#[test]
fn apple_gain_map() {
    let data = heic_apple();
    let img = read(&data).unwrap();
    assert_eq!(img.container, Container::Heif);
    assert_eq!(img.base.codec, Codec::Hevc);
    assert_eq!(
        (img.base.width, img.base.height, img.base.components),
        (64, 48, 3)
    );
    let g = img.gain_map.unwrap();
    assert_eq!(g.kind, Kind::Apple);
    assert_eq!(g.image.location, Location::Item(2));
    let a = g.apple.unwrap();
    assert_eq!(a.version.as_deref(), Some("65536"));
    assert_eq!(a.headroom.as_deref(), Some("4.5"));
    assert!(g.metadata.is_none() && g.iso.is_none() && g.xmp.is_none());
    assert_eq!(g.issues, vec!["metadata-missing"]);
}

#[test]
fn image_without_gain_map() {
    let mut s = Heif::new(b"heic");
    let size = s.prop(ispe(64, 48));
    let cfg = s.prop(hvcc());
    s.item(1, b"hvc1", &[1; 4], &[size, cfg]);
    let img = read(&s.build()).unwrap();
    assert!(img.gain_map.is_none());
    assert_eq!(img.base.location, Location::Item(1));
}

#[test]
fn damaged_files_never_panic() {
    for data in [avif_tmap(0), heic_apple()] {
        for n in 0..data.len() {
            let _ = read(&data[..n]);
        }
        for i in 0..data.len() {
            let mut f = data.clone();
            f[i] ^= 0xFF;
            let _ = read(&f);
        }
    }
    // An ftyp box and nothing else.
    assert!(read(&bx(b"ftyp", b"avif\0\0\0\0avif")).is_err());
}
