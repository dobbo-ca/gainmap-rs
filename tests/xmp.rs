//! `hdrgm` XMP: attribute form and `rdf:Seq` form (one and three values), defaults, errors.

mod common;

use common::{HDRGM_DECL, gm_xmp, limits, seq, xmp_doc};
use gainmap::{__fuzz, Error, HdrgmXmp, Metadata};

fn parse(xml: &str) -> HdrgmXmp {
    __fuzz::hdrgm_from_packet(xml.as_bytes(), &limits())
        .unwrap()
        .expect("hdrgm values")
}

fn strings(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

#[test]
fn attribute_form() {
    let x = parse(&gm_xmp("2", "2.5"));
    assert_eq!(x.version.as_deref(), Some("1.0"));
    assert_eq!(x.gain_map_min, strings(&["0"]));
    assert_eq!(x.gain_map_max, strings(&["2"]));
    assert_eq!(x.gamma, strings(&["1"]));
    assert_eq!(x.offset_sdr, strings(&["0.015625"]));
    assert_eq!(x.offset_hdr, strings(&["0.015625"]));
    assert_eq!(x.hdr_capacity_min.as_deref(), Some("0"));
    assert_eq!(x.hdr_capacity_max.as_deref(), Some("2.5"));
    assert_eq!(x.base_rendition_is_hdr.as_deref(), Some("False"));
    let m = Metadata::from_xmp(&x).unwrap();
    assert_eq!(m.gain_map_max, [2.0; 3]);
    assert_eq!(m.gain_map_min, [0.0; 3]);
    assert_eq!(m.gamma, [1.0; 3]);
    assert_eq!(m.base_offset, [0.015625; 3]);
    assert_eq!(m.alternate_offset, [0.015625; 3]);
    assert_eq!(m.base_hdr_headroom, 0.0);
    assert_eq!(m.alternate_hdr_headroom, 2.5);
    assert!(m.use_base_colour_space && !m.multichannel && !m.backward_direction);
}

#[test]
fn seq_with_one_value_applies_to_all_channels() {
    let body = [
        seq("GainMapMin", &["0.5"]),
        seq("GainMapMax", &["2"]),
        seq("Gamma", &["1.5"]),
    ]
    .concat();
    let xml = xmp_doc(
        HDRGM_DECL,
        "hdrgm:Version=\"1.0\" hdrgm:HDRCapacityMax=\"3\"",
        &body,
    );
    let x = parse(&xml);
    assert_eq!(x.gain_map_min, strings(&["0.5"]));
    assert_eq!(x.gamma, strings(&["1.5"]));
    let m = Metadata::from_xmp(&x).unwrap();
    assert_eq!(m.gain_map_min, [0.5; 3]);
    assert_eq!(m.gain_map_max, [2.0; 3]);
    assert_eq!(m.gamma, [1.5; 3]);
    assert!(!m.multichannel);
    // The defaults for what is missing.
    assert_eq!(m.base_offset, [1.0 / 64.0; 3]);
    assert_eq!(m.base_hdr_headroom, 0.0);
}

#[test]
fn seq_with_three_values_is_one_per_channel() {
    let body = [
        seq("GainMapMin", &["-0.25", "-0.5", "-0.75"]),
        seq("GainMapMax", &["1", "2", "3"]),
        seq("Gamma", &[" 1 ", "1.25", "1.5"]),
    ]
    .concat();
    let xml = xmp_doc(
        HDRGM_DECL,
        "hdrgm:Version=\"1.0\" hdrgm:HDRCapacityMin=\"0.5\" hdrgm:HDRCapacityMax=\"3\"",
        &body,
    );
    let x = parse(&xml);
    assert_eq!(x.gain_map_max, strings(&["1", "2", "3"]));
    // The text is kept as stored, with its spaces.
    assert_eq!(x.gamma, strings(&[" 1 ", "1.25", "1.5"]));
    let m = Metadata::from_xmp(&x).unwrap();
    assert_eq!(m.gain_map_min, [-0.25, -0.5, -0.75]);
    assert_eq!(m.gain_map_max, [1.0, 2.0, 3.0]);
    assert_eq!(m.gamma, [1.0, 1.25, 1.5]);
    assert_eq!(m.base_hdr_headroom, 0.5);
    assert!(m.multichannel);
}

#[test]
fn base_rendition_is_hdr_sets_the_direction() {
    let mut x = parse(&gm_xmp("2", "2.5"));
    x.base_rendition_is_hdr = Some("True".to_string());
    assert!(Metadata::from_xmp(&x).unwrap().backward_direction);
}

#[test]
fn required_values_and_bad_values() {
    let mut x = parse(&gm_xmp("2", "2.5"));
    x.hdr_capacity_max = None;
    assert_eq!(
        Metadata::from_xmp(&x),
        Err(Error::Malformed("xmp-missing-hdrcapacitymax"))
    );
    let mut x = parse(&gm_xmp("2", "2.5"));
    x.gain_map_max.clear();
    assert_eq!(
        Metadata::from_xmp(&x),
        Err(Error::Malformed("xmp-missing-gainmapmax"))
    );
    let mut x = parse(&gm_xmp("2", "2.5"));
    x.gain_map_max = strings(&["1", "2"]);
    assert_eq!(
        Metadata::from_xmp(&x),
        Err(Error::Malformed("xmp-bad-count"))
    );
    let mut x = parse(&gm_xmp("2", "2.5"));
    x.gamma = strings(&["one"]);
    assert_eq!(
        Metadata::from_xmp(&x),
        Err(Error::Malformed("xmp-bad-number"))
    );
    let mut x = parse(&gm_xmp("2", "2.5"));
    x.gamma = strings(&["NaN"]);
    assert_eq!(
        Metadata::from_xmp(&x),
        Err(Error::Malformed("xmp-bad-number"))
    );
}

#[test]
fn packets_without_hdrgm_or_not_xml() {
    let plain = xmp_doc("xmlns:x2=\"http://example.com/\"", "x2:a=\"1\"", "");
    assert_eq!(
        __fuzz::hdrgm_from_packet(plain.as_bytes(), &limits()),
        Ok(None)
    );
    // Not XML: the packet is dropped, it is not an error.
    assert_eq!(
        __fuzz::hdrgm_from_packet(b"plain text", &limits()),
        Ok(None)
    );
}

#[test]
fn base_rendition_is_hdr_swaps_the_sdr_and_hdr_values() {
    let xml = xmp_doc(
        HDRGM_DECL,
        "hdrgm:Version=\"1.0\" hdrgm:GainMapMax=\"2\" hdrgm:OffsetSDR=\"0.01\" \
         hdrgm:OffsetHDR=\"0.05\" hdrgm:HDRCapacityMin=\"0.5\" hdrgm:HDRCapacityMax=\"3\"",
        "",
    );
    let mut x = parse(&xml);
    let m = Metadata::from_xmp(&x).unwrap();
    assert!(!m.backward_direction);
    assert_eq!((m.base_offset, m.alternate_offset), ([0.01; 3], [0.05; 3]));
    assert_eq!((m.base_hdr_headroom, m.alternate_hdr_headroom), (0.5, 3.0));

    // The base is the HDR rendition: its offset and headroom are the HDR ones.
    x.base_rendition_is_hdr = Some("True".to_string());
    let m = Metadata::from_xmp(&x).unwrap();
    assert!(m.backward_direction);
    assert_eq!((m.base_offset, m.alternate_offset), ([0.05; 3], [0.01; 3]));
    assert_eq!((m.base_hdr_headroom, m.alternate_hdr_headroom), (3.0, 0.5));
}
