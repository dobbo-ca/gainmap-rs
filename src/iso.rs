//! ISO 21496-1 gain map metadata: the binary payload of the JPEG APP2 segment and of the HEIF
//! `tmap` item (after its version byte).
//!
//! The reader is a port of `decodeGainmapMetadata` and `gainmapMetadataFractionToFloat` from
//! `lib/src/gainmapmetadata.cpp` of libultrahdr v2.0.2 (Copyright 2024 The Android Open Source
//! Project; licensed MIT OR Apache-2.0). See NOTICE.

use crate::{Error, Metadata};

/// A rational number as stored: a signed numerator (an `i64` holds both `s32` and `u32`) and an
/// unsigned denominator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fraction {
    /// The numerator.
    pub numerator: i64,
    /// The denominator. 0 is stored as read; [`Metadata::from_iso`] rejects it.
    pub denominator: u32,
}

/// The values of one colour channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IsoChannel {
    /// log2 of the smallest gain.
    pub gain_map_min: Fraction,
    /// log2 of the largest gain.
    pub gain_map_max: Fraction,
    /// The gamma of the stored gain map.
    pub gamma: Fraction,
    /// The offset added to the base image (linear).
    pub base_offset: Fraction,
    /// The offset added to the alternate image (linear).
    pub alternate_offset: Fraction,
}

/// ISO 21496-1 metadata exactly as stored, as fractions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IsoMetadata {
    /// `minimum_version`. A reader must refuse the map when it is not 0.
    pub minimum_version: u16,
    /// `writer_version`.
    pub writer_version: u16,
    /// The flags byte as stored.
    pub flags: u8,
    /// log2 of the HDR headroom of the base image.
    pub base_hdr_headroom: Fraction,
    /// log2 of the HDR headroom of the alternate image.
    pub alternate_hdr_headroom: Fraction,
    /// The channels. Channels 1 and 2 are copies of channel 0 when the map is not multichannel.
    pub channels: [IsoChannel; 3],
}

const MULTICHANNEL: u8 = 0x80;
const USE_BASE_COLOUR_SPACE: u8 = 0x40;
const COMMON_DENOMINATOR: u8 = 0x08;
const BACKWARD_DIRECTION: u8 = 0x04;

/// A big-endian reader that returns `Truncated` at the end of the payload.
struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl Reader<'_> {
    fn take<const N: usize>(&mut self) -> Result<[u8; N], Error> {
        let end = self.pos.checked_add(N).ok_or(Error::Truncated)?;
        let bytes = self.data.get(self.pos..end).ok_or(Error::Truncated)?;
        self.pos = end;
        <[u8; N]>::try_from(bytes).map_err(|_| Error::Truncated)
    }

    fn u8(&mut self) -> Result<u8, Error> {
        Ok(self.take::<1>()?[0])
    }

    fn u16(&mut self) -> Result<u16, Error> {
        Ok(u16::from_be_bytes(self.take()?))
    }

    fn u32(&mut self) -> Result<u32, Error> {
        Ok(u32::from_be_bytes(self.take()?))
    }

    fn s32(&mut self) -> Result<i32, Error> {
        Ok(i32::from_be_bytes(self.take()?))
    }

    /// A fraction with a signed numerator and a denominator of its own.
    fn signed(&mut self) -> Result<Fraction, Error> {
        Ok(Fraction {
            numerator: i64::from(self.s32()?),
            denominator: self.u32()?,
        })
    }

    /// A fraction with an unsigned numerator and a denominator of its own.
    fn unsigned(&mut self) -> Result<Fraction, Error> {
        Ok(Fraction {
            numerator: i64::from(self.u32()?),
            denominator: self.u32()?,
        })
    }
}

impl IsoMetadata {
    /// Parses the payload that starts at `minimum_version`: no `urn:iso:std:iso:ts:21496:-1`
    /// namespace and no `tmap` version byte. Bytes after the last field are ignored, as in
    /// libultrahdr. A `minimum_version` other than 0 parses; [`Metadata::from_iso`] refuses it.
    ///
    /// # Errors
    ///
    /// [`Error::Truncated`] when the payload ends before the last field (this includes the
    /// 4-byte "version only" payload of a primary image). It allocates nothing.
    pub fn parse(payload: &[u8]) -> Result<IsoMetadata, Error> {
        let mut r = Reader {
            data: payload,
            pos: 0,
        };
        let minimum_version = r.u16()?;
        let writer_version = r.u16()?;
        let flags = r.u8()?;
        let count = if flags & MULTICHANNEL != 0 { 3 } else { 1 };
        let zero = Fraction {
            numerator: 0,
            denominator: 0,
        };
        let blank = IsoChannel {
            gain_map_min: zero,
            gain_map_max: zero,
            gamma: zero,
            base_offset: zero,
            alternate_offset: zero,
        };
        let mut channels = [blank; 3];
        let (base_hdr_headroom, alternate_hdr_headroom) = if flags & COMMON_DENOMINATOR != 0 {
            let d = r.u32()?;
            let with_d = |numerator: i64| Fraction {
                numerator,
                denominator: d,
            };
            let base = with_d(i64::from(r.u32()?));
            let alternate = with_d(i64::from(r.u32()?));
            for ch in channels.iter_mut().take(count) {
                ch.gain_map_min = with_d(i64::from(r.s32()?));
                ch.gain_map_max = with_d(i64::from(r.s32()?));
                ch.gamma = with_d(i64::from(r.u32()?));
                ch.base_offset = with_d(i64::from(r.s32()?));
                ch.alternate_offset = with_d(i64::from(r.s32()?));
            }
            (base, alternate)
        } else {
            let base = r.unsigned()?;
            let alternate = r.unsigned()?;
            for ch in channels.iter_mut().take(count) {
                ch.gain_map_min = r.signed()?;
                ch.gain_map_max = r.signed()?;
                ch.gamma = r.unsigned()?;
                ch.base_offset = r.signed()?;
                ch.alternate_offset = r.signed()?;
            }
            (base, alternate)
        };
        // Channels past the count are copies of channel 0.
        let first = channels[0];
        for ch in channels.iter_mut().skip(count) {
            *ch = first;
        }
        Ok(IsoMetadata {
            minimum_version,
            writer_version,
            flags,
            base_hdr_headroom,
            alternate_hdr_headroom,
            channels,
        })
    }

    /// Flag 0x80: the three channels are stored separately.
    pub fn is_multichannel(&self) -> bool {
        self.flags & MULTICHANNEL != 0
    }

    /// Flag 0x40: the gain map is applied in the colour space of the base image.
    pub fn use_base_colour_space(&self) -> bool {
        self.flags & USE_BASE_COLOUR_SPACE != 0
    }

    /// Flag 0x04: the gain map goes from the HDR image to the SDR image.
    pub fn backward_direction(&self) -> bool {
        self.flags & BACKWARD_DIRECTION != 0
    }

    /// Flag 0x08: the payload stores one denominator for every value.
    pub fn common_denominator(&self) -> bool {
        self.flags & COMMON_DENOMINATOR != 0
    }
}

/// `numerator / denominator` in `f32`, as libultrahdr does; a zero denominator is an error.
fn to_f32(f: Fraction) -> Result<f32, Error> {
    if f.denominator == 0 {
        return Err(Error::Malformed("iso-zero-denominator"));
    }
    Ok(f.numerator as f32 / f.denominator as f32)
}

impl Metadata {
    /// Converts stored fractions to floats (log2 domain for the gains and headrooms).
    ///
    /// # Errors
    ///
    /// [`Error::Unsupported`]`("iso minimum_version")` when `minimum_version` is not 0;
    /// [`Error::Malformed`]`("iso-zero-denominator")` when any denominator is 0.
    pub fn from_iso(iso: &IsoMetadata) -> Result<Metadata, Error> {
        if iso.minimum_version != 0 {
            return Err(Error::Unsupported("iso minimum_version"));
        }
        let mut m = Metadata {
            gain_map_min: [0.0; 3],
            gain_map_max: [0.0; 3],
            gamma: [0.0; 3],
            base_offset: [0.0; 3],
            alternate_offset: [0.0; 3],
            base_hdr_headroom: to_f32(iso.base_hdr_headroom)?,
            alternate_hdr_headroom: to_f32(iso.alternate_hdr_headroom)?,
            use_base_colour_space: iso.use_base_colour_space(),
            multichannel: iso.is_multichannel(),
            backward_direction: iso.backward_direction(),
        };
        for (i, ch) in iso.channels.iter().enumerate() {
            m.gain_map_min[i] = to_f32(ch.gain_map_min)?;
            m.gain_map_max[i] = to_f32(ch.gain_map_max)?;
            m.gamma[i] = to_f32(ch.gamma)?;
            m.base_offset[i] = to_f32(ch.base_offset)?;
            m.alternate_offset[i] = to_f32(ch.alternate_offset)?;
        }
        Ok(m)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_only() {
        assert_eq!(IsoMetadata::parse(&[0, 0, 0, 0]), Err(Error::Truncated));
    }
}
