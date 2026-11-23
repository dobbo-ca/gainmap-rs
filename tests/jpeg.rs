//! JPEG gain maps through `read`: the three locators, the metadata precedence, the never-drop
//! rule, Apple XMP, and sweeps with damaged files.

mod common;

use common::*;
use gainmap::{Codec, Container, Error, GainMapImage, Kind, Limit, Location};

fn read(data: &[u8]) -> Result<GainMapImage, Error> {
    gainmap::read(data, &limits(), &no_cancel())
}

/// A primary image with the primary XMP (`hdrgm:Version`) and the version-only ISO segment.
fn primary_segs() -> Vec<Vec<u8>> {
    vec![app1_xmp(&primary_xmp()), app2_iso(&[0, 0, 0, 0])]
}

fn gm_with(segs: &[Vec<u8>]) -> Vec<u8> {
    with_segments(&jpeg_gray(8, 8), segs)
}

/// The standard gain map: XMP in attribute form.
fn gm_std() -> Vec<u8> {
    gm_with(&[app1_xmp(&gm_xmp("2", "2.5"))])
}

fn range(r: std::ops::Range<usize>) -> Location {
    Location::Range(r)
}

#[test]
fn mpf_big_and_little_endian() {
    for be in [true, false] {
        let gm = gm_std();
        let b = with_mpf(&primary_segs(), &jpeg_rgb(16, 16), &gm, be);
        let img = read(&b.file).unwrap();
        assert_eq!(img.container, Container::Jpeg);
        assert!(img.heif().is_none());
        assert_eq!(img.base.codec, Codec::Jpeg);
        assert_eq!(
            (img.base.width, img.base.height, img.base.components),
            (16, 16, 3)
        );
        assert_eq!(img.base.location, range(0..b.primary_len));
        let g = img.gain_map.as_ref().unwrap();
        assert_eq!(g.kind, Kind::UltraHdr, "be {be}");
        assert_eq!(
            (g.image.width, g.image.height, g.image.components),
            (8, 8, 1)
        );
        assert_eq!(
            g.image.location,
            range(b.primary_len..b.primary_len + b.gm_len)
        );
        assert_eq!(g.primary_version.as_deref(), Some("1.0"));
        assert!(g.metadata.is_some() && g.xmp.is_some() && g.iso.is_none());
        assert!(g.issues.is_empty(), "{:?}", g.issues);
        // The coded bytes are the two JPEG files.
        let base = img
            .coded_bytes(&b.file, &img.base, &limits(), &no_cancel())
            .unwrap();
        assert_eq!(&base[..2], &[0xFF, 0xD8]);
        assert_eq!(base.len(), b.primary_len);
        let coded = img
            .coded_bytes(&b.file, &g.image, &limits(), &no_cancel())
            .unwrap();
        assert_eq!(&*coded, &gm[..]);
    }
}

#[test]
fn mpf_picks_the_image_with_gain_map_xmp() {
    // Entry 1 is a plain JPEG (a preview), entry 2 the gain map.
    let plain = jpeg_gray(4, 4);
    let gm = gm_std();
    let b = with_mpf_many(
        &primary_segs(),
        &jpeg_rgb(16, 16),
        &[&plain[..], &gm[..]],
        true,
    );
    let img = read(&b.file).unwrap();
    let g = img.gain_map.unwrap();
    assert_eq!(g.image.location, range(b.starts[1]..b.starts[1] + gm.len()));
}

#[test]
fn gcontainer_directory_with_padding() {
    let gm = gm_std();
    let primary = with_segments(&jpeg_rgb(16, 16), &[app1_xmp(&container_xmp(gm.len(), 5))]);
    let mut file = primary.clone();
    file.extend_from_slice(&[0; 5]);
    file.extend_from_slice(&gm);
    let img = read(&file).unwrap();
    let g = img.gain_map.unwrap();
    // The JPEG does not start right after the primary image: only the directory finds it.
    assert_eq!(g.image.location, range(primary.len() + 5..file.len()));
    assert_eq!(g.kind, Kind::UltraHdr);
    assert!(g.metadata.is_some());
}

#[test]
fn gcontainer_semantic_without_gain_map_xmp() {
    // The gain map has no XMP and no ISO: only the directory names it.
    let gm = jpeg_gray(8, 8);
    let primary = with_segments(&jpeg_rgb(16, 16), &[app1_xmp(&container_xmp(gm.len(), 0))]);
    let mut file = primary.clone();
    file.extend_from_slice(&gm);
    let img = read(&file).unwrap();
    let g = img.gain_map.unwrap();
    assert_eq!(g.image.location, range(primary.len()..file.len()));
    assert!(g.metadata.is_none());
    assert!(g.issues.contains(&"metadata-missing"));
}

#[test]
fn fallback_jpeg_after_the_primary() {
    let gm = gm_std();
    let primary = with_segments(&jpeg_rgb(16, 16), &[app1_xmp(&primary_xmp())]);
    let mut file = primary.clone();
    file.extend_from_slice(&gm);
    let img = read(&file).unwrap();
    let g = img.gain_map.unwrap();
    assert_eq!(g.image.location, range(primary.len()..file.len()));
    assert_eq!(g.kind, Kind::UltraHdr);
    assert!(g.issues.is_empty());
}

#[test]
fn iso_only_file() {
    // ISO version-only on the primary, the full payload on the gain map, no XMP at all.
    let gm = gm_with(&[app2_iso(&iso_common(true, 0))]);
    let b = with_mpf(&[app2_iso(&[0, 0, 0, 0])], &jpeg_rgb(16, 16), &gm, true);
    let img = read(&b.file).unwrap();
    let g = img.gain_map.unwrap();
    assert_eq!(g.kind, Kind::Iso);
    assert!(g.primary_version.is_none() && g.xmp.is_none() && g.apple.is_none());
    let m = g.metadata.unwrap();
    assert!(m.multichannel);
    assert_eq!(m.alternate_hdr_headroom, 3.0);
    assert!(g.iso.unwrap().is_multichannel());
    assert!(g.issues.is_empty());
}

#[test]
fn no_gain_map() {
    let plain = jpeg_rgb(16, 16);
    let img = read(&plain).unwrap();
    assert!(img.gain_map.is_none());
    assert_eq!(img.base.location, range(0..plain.len()));
    // Trailing bytes that are not a JPEG.
    let mut tail = with_segments(&plain, &[app1_xmp(&primary_xmp())]);
    tail.extend_from_slice(b"trailing bytes");
    assert!(read(&tail).unwrap().gain_map.is_none());
    // A second JPEG without gain map metadata is not a gain map.
    let mut two = with_segments(&plain, &[app1_xmp(&primary_xmp())]);
    two.extend_from_slice(&jpeg_gray(8, 8));
    assert!(read(&two).unwrap().gain_map.is_none());
}

#[test]
fn metadata_precedence_iso_over_xmp() {
    let both = gm_with(&[
        app1_xmp(&gm_xmp("2", "2.5")),
        app2_iso(&iso_common(false, 0)),
    ]);
    let b = with_mpf(&primary_segs(), &jpeg_rgb(16, 16), &both, true);
    let g = read(&b.file).unwrap().gain_map.unwrap();
    assert_eq!(g.kind, Kind::UltraHdr);
    // ISO says 3.0, the XMP 2.5.
    assert_eq!(g.metadata.as_ref().unwrap().alternate_hdr_headroom, 3.0);
    assert!(g.iso.is_some() && g.xmp.is_some());
    assert!(g.issues.is_empty());
}

#[test]
fn invalid_iso_falls_back_to_xmp() {
    let both = gm_with(&[
        app1_xmp(&gm_xmp("2", "2.5")),
        app2_iso(&iso_common(false, 1)),
    ]);
    let b = with_mpf(&primary_segs(), &jpeg_rgb(16, 16), &both, true);
    let g = read(&b.file).unwrap().gain_map.unwrap();
    assert_eq!(g.metadata.as_ref().unwrap().alternate_hdr_headroom, 2.5);
    assert_eq!(g.issues, vec!["iso-minimum-version"]);
    assert_eq!(g.iso.unwrap().minimum_version, 1);
}

/// Reads a file whose gain map has `segs`; the gain map must be kept.
fn kept(segs: &[Vec<u8>]) -> gainmap::GainMap {
    let b = with_mpf(&primary_segs(), &jpeg_rgb(16, 16), &gm_with(segs), true);
    read(&b.file)
        .unwrap()
        .gain_map
        .expect("the gain map is kept")
}

#[test]
fn never_drop_the_gain_map() {
    // ISO with an unsupported minimum version, no XMP.
    let g = kept(&[app2_iso(&iso_common(false, 1))]);
    assert!(g.metadata.is_none());
    assert_eq!(g.issues, vec!["iso-minimum-version"]);

    // XMP without HDRCapacityMax.
    let xml = xmp_doc(
        HDRGM_DECL,
        "hdrgm:Version=\"1.0\" hdrgm:GainMapMax=\"2\"",
        "",
    );
    let g = kept(&[app1_xmp(&xml)]);
    assert!(g.metadata.is_none() && g.xmp.is_some());
    assert_eq!(g.issues, vec!["xmp-missing-hdrcapacitymax"]);

    // A sequence of two values.
    let body = seq("GainMapMax", &["1", "2"]);
    let xml = xmp_doc(
        HDRGM_DECL,
        "hdrgm:Version=\"1.0\" hdrgm:HDRCapacityMax=\"3\"",
        &body,
    );
    let g = kept(&[app1_xmp(&xml)]);
    assert!(g.metadata.is_none());
    assert_eq!(g.issues, vec!["xmp-bad-count"]);

    // An ISO payload cut short.
    let g = kept(&[app2_iso(&iso_common(false, 0)[..20])]);
    assert!(g.metadata.is_none() && g.iso.is_none());
    assert_eq!(g.issues, vec!["iso-truncated", "metadata-missing"]);

    // Garbage XMP next to a valid ISO payload: the ISO metadata is used.
    let g = kept(&[app1_xmp("this is not xml"), app2_iso(&iso_common(false, 0))]);
    assert!(g.metadata.is_some());
    assert_eq!(g.issues, vec!["xmp-malformed"]);
}

#[test]
fn apple_xmp() {
    let gm = gm_with(&[app1_xmp(&apple_xmp())]);
    let b = with_mpf(&[], &jpeg_rgb(16, 16), &gm, true);
    let g = read(&b.file).unwrap().gain_map.unwrap();
    assert_eq!(g.kind, Kind::Apple);
    let a = g.apple.unwrap();
    assert_eq!(a.version.as_deref(), Some("65536"));
    assert_eq!(a.headroom.as_deref(), Some("4.5"));
    assert!(g.xmp.is_none() && g.primary_version.is_none());
    assert!(g.metadata.is_none());
    assert_eq!(g.issues, vec!["metadata-missing"]);
}

#[test]
fn bad_mpf_falls_through_to_the_next_locator() {
    let gm = gm_std();
    // A segment that has the MPF signature but no endian field.
    let mut segs = vec![app1_xmp(&primary_xmp())];
    segs.push(seg(0xE2, &[&b"MPF\0"[..], &[0u8; 20]].concat()));
    let primary = with_segments(&jpeg_rgb(16, 16), &segs);
    let mut file = primary.clone();
    file.extend_from_slice(&gm);
    let g = read(&file).unwrap().gain_map.unwrap();
    assert!(g.issues.contains(&"mpf-malformed"));
    assert_eq!(g.image.location, range(primary.len()..file.len()));

    // An entry offset far outside the file: skipped, the JPEG after the primary is found.
    let mut segs = vec![app1_xmp(&primary_xmp())];
    segs.push(mpf_seg(true, &[(0x30000, 0, 0), (0, 100, 0x7FFF_0000)]));
    let primary = with_segments(&jpeg_rgb(16, 16), &segs);
    let mut file = primary.clone();
    file.extend_from_slice(&gm);
    let g = read(&file).unwrap().gain_map.unwrap();
    assert_eq!(g.image.location, range(primary.len()..file.len()));

    // The same file without the primary XMP has nothing to fall back on.
    let primary = with_segments(
        &jpeg_rgb(16, 16),
        &[mpf_seg(true, &[(0x30000, 0, 0), (0, 100, 0x7FFF_0000)])],
    );
    let mut file = primary;
    file.extend_from_slice(&gm);
    assert!(read(&file).unwrap().gain_map.is_none());
}

#[test]
fn mpf_entries_over_max_images() {
    let b = with_mpf(&primary_segs(), &jpeg_rgb(16, 16), &gm_std(), true);
    let lim = gainmap::Limits {
        max_images: 1,
        ..limits()
    };
    assert_eq!(
        gainmap::read(&b.file, &lim, &no_cancel()),
        Err(Error::LimitExceeded(Limit::Images))
    );
}

#[test]
fn primary_must_have_an_sof_and_an_eoi() {
    assert_eq!(
        read(&[0xFF, 0xD8, 0xFF, 0xD9]),
        Err(Error::Malformed("jpeg-no-sof"))
    );
    let file = jpeg_rgb(16, 16);
    assert_eq!(read(&file[..file.len() - 2]), Err(Error::Truncated));
    assert_eq!(read(b"hello world"), Err(Error::UnknownFormat));
    assert_eq!(read(&[]), Err(Error::UnknownFormat));
}

#[test]
fn truncated_and_damaged_files_never_panic() {
    let gm = gm_with(&[
        app1_xmp(&gm_xmp("2", "2.5")),
        app2_iso(&iso_common(true, 0)),
    ]);
    let b = with_mpf(&primary_segs(), &jpeg_rgb(16, 16), &gm, true);
    let lim = gainmap::Limits {
        max_alloc: 64 << 20,
        ..limits()
    };
    for n in 0..b.file.len() {
        let _ = gainmap::read(&b.file[..n], &lim, &no_cancel());
    }
    for i in 0..b.file.len() {
        for x in [0xFFu8, 0x01, 0x80] {
            let mut f = b.file.clone();
            f[i] ^= x;
            if let Ok(img) = gainmap::read(&f, &lim, &no_cancel()) {
                let _ = gainmap::decode(&f, &img, &lim, &no_cancel());
            }
        }
    }
}
