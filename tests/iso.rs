//! ISO 21496-1 payload parse: both denominator modes, one and three channels, truncation at every
//! byte, version-only, and the conversion to `Metadata`.

mod common;

use common::{iso_common, iso_separate, limits};
use gainmap::{Error, Fraction, IsoMetadata, Metadata};

fn f(numerator: i64, denominator: u32) -> Fraction {
    Fraction {
        numerator,
        denominator,
    }
}

#[test]
fn common_denominator_one_channel() {
    let iso = IsoMetadata::parse(&iso_common(false, 0), &limits()).unwrap();
    assert_eq!((iso.minimum_version, iso.writer_version), (0, 0));
    assert_eq!(iso.flags, 0x48);
    assert!(iso.common_denominator() && iso.use_base_colour_space());
    assert!(!iso.is_multichannel() && !iso.backward_direction());
    assert_eq!(iso.base_hdr_headroom, f(0, 1000));
    assert_eq!(iso.alternate_hdr_headroom, f(3000, 1000));
    let c = iso.channels[0];
    assert_eq!(c.gain_map_min, f(0, 1000));
    assert_eq!(c.gain_map_max, f(2000, 1000));
    assert_eq!(c.gamma, f(1000, 1000));
    assert_eq!(c.base_offset, f(15, 1000));
    assert_eq!(c.alternate_offset, f(25, 1000));
    // Channels 1 and 2 are copies of channel 0.
    assert_eq!(iso.channels[1], c);
    assert_eq!(iso.channels[2], c);
}

#[test]
fn common_denominator_three_channels() {
    let iso = IsoMetadata::parse(&iso_common(true, 0), &limits()).unwrap();
    assert!(iso.is_multichannel());
    for (i, c) in iso.channels.iter().enumerate() {
        let i = i as i64;
        assert_eq!(c.gain_map_min, f(i * 100, 1000));
        assert_eq!(c.gain_map_max, f(2000 + i * 100, 1000));
    }
}

#[test]
fn separate_denominators() {
    let one = IsoMetadata::parse(&iso_separate(false), &limits()).unwrap();
    assert!(!one.common_denominator() && !one.is_multichannel());
    assert_eq!(one.base_hdr_headroom, f(1, 2));
    assert_eq!(one.alternate_hdr_headroom, f(6, 2));
    let c = one.channels[0];
    assert_eq!(c.gain_map_min, f(0, 4));
    assert_eq!(c.gain_map_max, f(8, 4));
    assert_eq!(c.gamma, f(3, 3));
    assert_eq!(c.base_offset, f(1, 64));
    assert_eq!(c.alternate_offset, f(2, 64));
    assert_eq!(one.channels[2], c);

    let three = IsoMetadata::parse(&iso_separate(true), &limits()).unwrap();
    assert_eq!(three.channels[1].gain_map_min, f(-1, 4));
    assert_eq!(three.channels[2].gain_map_min, f(-2, 4));
    assert_eq!(three.channels[2].gain_map_max, f(10, 4));
}

#[test]
fn sizes_match_libultrahdr() {
    assert_eq!(iso_common(false, 0).len(), 37);
    assert_eq!(iso_common(true, 0).len(), 77);
    assert_eq!(iso_separate(false).len(), 61);
    assert_eq!(iso_separate(true).len(), 141);
}

#[test]
fn truncation_at_every_byte_is_an_error() {
    for bytes in [
        iso_common(false, 0),
        iso_common(true, 0),
        iso_separate(false),
        iso_separate(true),
    ] {
        assert!(IsoMetadata::parse(&bytes, &limits()).is_ok());
        for n in 0..bytes.len() {
            assert_eq!(
                IsoMetadata::parse(&bytes[..n], &limits()),
                Err(Error::Truncated),
                "length {n} of {}",
                bytes.len()
            );
        }
    }
}

#[test]
fn trailing_bytes_are_ignored() {
    let mut bytes = iso_common(false, 0);
    bytes.extend_from_slice(&[0xAA; 9]);
    assert_eq!(
        IsoMetadata::parse(&bytes, &limits()),
        IsoMetadata::parse(&iso_common(false, 0), &limits())
    );
}

#[test]
fn version_only_payload_is_not_metadata() {
    // The 4-byte payload of a primary image: minimum and writer version only.
    assert_eq!(
        IsoMetadata::parse(&[0, 0, 0, 0], &limits()),
        Err(Error::Truncated)
    );
    assert_eq!(IsoMetadata::parse(&[], &limits()), Err(Error::Truncated));
}

#[test]
fn metadata_values() {
    let m = Metadata::from_iso(
        &IsoMetadata::parse(&iso_common(false, 0), &limits()).unwrap(),
        &limits(),
    )
    .unwrap();
    assert_eq!(m.base_hdr_headroom, 0.0);
    assert_eq!(m.alternate_hdr_headroom, 3.0);
    assert_eq!(m.gain_map_min, [0.0; 3]);
    assert_eq!(m.gain_map_max, [2.0; 3]);
    assert_eq!(m.gamma, [1.0; 3]);
    assert_eq!(m.base_offset, [15f32 / 1000f32; 3]);
    assert_eq!(m.alternate_offset, [25f32 / 1000f32; 3]);
    assert!(m.use_base_colour_space && !m.multichannel && !m.backward_direction);

    let m = Metadata::from_iso(
        &IsoMetadata::parse(&iso_common(true, 0), &limits()).unwrap(),
        &limits(),
    )
    .unwrap();
    assert!(m.multichannel);
    assert_eq!(m.gain_map_min, [0.0, 100f32 / 1000f32, 200f32 / 1000f32]);

    let m = Metadata::from_iso(
        &IsoMetadata::parse(&iso_separate(false), &limits()).unwrap(),
        &limits(),
    )
    .unwrap();
    assert_eq!(m.base_hdr_headroom, 0.5);
    assert_eq!(m.alternate_hdr_headroom, 3.0);
    assert_eq!(m.gain_map_max, [2.0; 3]);
    assert_eq!(m.gamma, [1.0; 3]);
    assert!(!m.use_base_colour_space);
}

#[test]
fn minimum_version_parses_but_is_unsupported() {
    let iso = IsoMetadata::parse(&iso_common(false, 1), &limits()).unwrap();
    assert_eq!(iso.minimum_version, 1);
    assert_eq!(
        Metadata::from_iso(&iso, &limits()),
        Err(Error::Unsupported("iso minimum_version"))
    );
}

#[test]
fn unknown_layout_keeps_the_header_only() {
    // Version 1 with a short body: not the v0 layout, so it is not "truncated".
    let iso = IsoMetadata::parse(&[0, 1, 0, 1, 0x48], &limits()).unwrap();
    assert_eq!(
        (iso.minimum_version, iso.writer_version, iso.flags),
        (1, 1, 0x48)
    );
    assert_eq!(iso.base_hdr_headroom, f(0, 0));
    assert_eq!(
        Metadata::from_iso(&iso, &limits()),
        Err(Error::Unsupported("iso minimum_version"))
    );
    // Without the flags byte the flags read as 0.
    let iso = IsoMetadata::parse(&[0, 1, 0, 1], &limits()).unwrap();
    assert_eq!(iso.flags, 0);
    // Inside the two versions it is still truncated.
    assert_eq!(
        IsoMetadata::parse(&[0, 1, 0], &limits()),
        Err(Error::Truncated)
    );
}

#[test]
fn zero_denominator_is_malformed() {
    let mut bytes = iso_separate(false);
    // The denominator of the base headroom is bytes 9..13.
    bytes[9..13].copy_from_slice(&0u32.to_be_bytes());
    let iso = IsoMetadata::parse(&bytes, &limits()).unwrap();
    assert_eq!(
        Metadata::from_iso(&iso, &limits()),
        Err(Error::Malformed("iso-zero-denominator"))
    );
}

#[test]
fn backward_direction_flag_is_kept() {
    let mut bytes = iso_common(false, 0);
    bytes[4] |= 0x04;
    let iso = IsoMetadata::parse(&bytes, &limits()).unwrap();
    assert!(iso.backward_direction());
    assert!(
        Metadata::from_iso(&iso, &limits())
            .unwrap()
            .backward_direction
    );
}
