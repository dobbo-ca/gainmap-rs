//! The public data types of a read gain-map image.

use std::borrow::Cow;
use std::ops::Range;
use std::sync::atomic::AtomicBool;

use crate::{Error, IsoMetadata, Limits};

/// The file format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Container {
    /// A JPEG file (Ultra HDR, ISO 21496-1 or Apple).
    Jpeg,
    /// A HEIF or HEIC file.
    Heif,
    /// An AVIF file: the `ftyp` major or a compatible brand is `avif` or `avis`, and the base
    /// image is AV1.
    Avif,
}

/// The codec of a coded image.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Codec {
    /// Baseline or progressive JPEG.
    Jpeg,
    /// HEVC (HEIC).
    Hevc,
    /// AV1 (AVIF).
    Av1,
}

/// Where the bytes of a coded image are.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Location {
    /// JPEG: the range in the input from the SOI marker to just after the EOI marker.
    Range(Range<usize>),
    /// HEIF: the item id.
    Item(u32),
}

/// A coded image (not decoded): what it is and where its bytes are.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodedImage {
    /// The codec. For a grid it is the codec of the tiles.
    pub codec: Codec,
    /// JPEG: the SOF width. HEIF: the `ispe` width (the grid size for a grid).
    pub width: u32,
    /// JPEG: the SOF height. HEIF: the `ispe` height.
    pub height: u32,
    /// 1 or 3. JPEG: the SOF component count. HEIF: 1 for a monochrome image (`av1C`
    /// monochrome, `hvcC` chroma format 0), else 3.
    pub components: u8,
    /// Where the coded bytes are.
    pub location: Location,
}

/// How the gain map was found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A JPEG with `hdrgm` XMP (Ultra HDR; the file may also carry ISO 21496-1).
    UltraHdr,
    /// ISO 21496-1 only: a JPEG APP2 segment or a HEIF `tmap` item.
    Iso,
    /// An Apple `HDRGainMap` auxiliary image or XMP.
    Apple,
}

/// A file with its base image and, when it has one, its gain map.
#[derive(Debug, Clone, PartialEq)]
pub struct GainMapImage {
    /// The file format.
    pub container: Container,
    /// The SDR base image (JPEG: the primary image; HEIF: the primary item, or the base of a
    /// primary `tmap` item).
    pub base: CodedImage,
    /// The gain map, when the file has one.
    pub gain_map: Option<GainMap>,
    /// The parsed HEIF graph (HEIF and AVIF only).
    pub(crate) heif: Option<isobmff::Heif>,
}

impl GainMapImage {
    /// The coded bytes of `img` (the base or the gain map of this value): borrowed from `data`
    /// when they are one contiguous range, else a copy charged to `limits.max_alloc`. `data`
    /// must be the input that [`read`](crate::read) got. For a HEIF grid, the bytes are those of
    /// the grid item, not of its tiles (see [`heif`](Self::heif)).
    ///
    /// # Errors
    ///
    /// [`Error::Truncated`] when the range or item data is not inside `data`;
    /// [`Error::Malformed`] when a HEIF location is used on a JPEG; the errors of the HEIF item
    /// data ([`Error::Heif`]); [`Error::LimitExceeded`] for `max_alloc`; [`Error::OutOfMemory`];
    /// [`Error::Cancelled`].
    pub fn coded_bytes<'a>(
        &self,
        data: &'a [u8],
        img: &CodedImage,
        limits: &Limits,
        cancel: &AtomicBool,
    ) -> Result<Cow<'a, [u8]>, Error> {
        match &img.location {
            Location::Range(r) => data
                .get(r.clone())
                .map(Cow::Borrowed)
                .ok_or(Error::Truncated),
            Location::Item(id) => {
                let heif = self.heif.as_ref().ok_or(Error::Malformed("location"))?;
                let l = isobmff::Limits {
                    max_alloc: limits.max_alloc,
                    max_pixels: limits.max_pixels,
                    ..isobmff::Limits::default()
                };
                heif.item_bytes(data, *id, &l, cancel).map_err(Error::from)
            }
        }
    }

    /// The full HEIF graph (HEIF and AVIF only), for a later HEVC or AV1 decode layer.
    pub fn heif(&self) -> Option<&isobmff::Heif> {
        self.heif.as_ref()
    }
}

/// A gain map image with its metadata.
#[derive(Debug, Clone, PartialEq)]
pub struct GainMap {
    /// How it was found.
    pub kind: Kind,
    /// The coded gain-map image.
    pub image: CodedImage,
    /// The resolved metadata: valid ISO 21496-1 first, else valid `hdrgm` XMP, else `None`
    /// (the reason is in `issues`).
    pub metadata: Option<Metadata>,
    /// The ISO 21496-1 payload as stored.
    pub iso: Option<IsoMetadata>,
    /// The `hdrgm` XMP of the gain-map image as stored.
    pub xmp: Option<HdrgmXmp>,
    /// `hdrgm:Version` in the XMP of the primary image (JPEG), raw text.
    pub primary_version: Option<String>,
    /// The Apple `HDRGainMap` XMP values.
    pub apple: Option<AppleGainMap>,
    /// Stable tokens for what was wrong or missing, for example `iso-minimum-version`,
    /// `xmp-missing-gainmapmax`, `mpf-malformed`, `xmp-malformed`, `metadata-missing`.
    pub issues: Vec<&'static str>,
}

/// The `hdrgm` values of an XMP packet, raw text exactly as stored: one entry per `rdf:li` of a
/// `rdf:Seq`, one entry for an attribute or a simple element.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HdrgmXmp {
    /// `hdrgm:Version`.
    pub version: Option<String>,
    /// `hdrgm:GainMapMin` (log2).
    pub gain_map_min: Vec<String>,
    /// `hdrgm:GainMapMax` (log2).
    pub gain_map_max: Vec<String>,
    /// `hdrgm:Gamma`.
    pub gamma: Vec<String>,
    /// `hdrgm:OffsetSDR`.
    pub offset_sdr: Vec<String>,
    /// `hdrgm:OffsetHDR`.
    pub offset_hdr: Vec<String>,
    /// `hdrgm:HDRCapacityMin` (log2).
    pub hdr_capacity_min: Option<String>,
    /// `hdrgm:HDRCapacityMax` (log2).
    pub hdr_capacity_max: Option<String>,
    /// `hdrgm:BaseRenditionIsHDR`.
    pub base_rendition_is_hdr: Option<String>,
}

/// The Apple HDR gain map XMP values, raw text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppleGainMap {
    /// `HDRGainMap:HDRGainMapVersion`.
    pub version: Option<String>,
    /// `HDRGainMap:HDRGainMapHeadroom`.
    pub headroom: Option<String>,
}

/// Gain map metadata with ISO 21496-1 meaning, as `f32`. The gains and the headrooms are log2;
/// the gammas and the offsets are linear. Single-channel values are repeated for all three.
#[derive(Debug, Clone, PartialEq)]
pub struct Metadata {
    /// log2 of the smallest gain, per channel.
    pub gain_map_min: [f32; 3],
    /// log2 of the largest gain, per channel.
    pub gain_map_max: [f32; 3],
    /// The gamma of the stored gain map, per channel.
    pub gamma: [f32; 3],
    /// The offset of the base image, per channel (linear).
    pub base_offset: [f32; 3],
    /// The offset of the alternate image, per channel (linear).
    pub alternate_offset: [f32; 3],
    /// log2 of the HDR headroom of the base image.
    pub base_hdr_headroom: f32,
    /// log2 of the HDR headroom of the alternate image.
    pub alternate_hdr_headroom: f32,
    /// The gain map is applied in the colour space of the base image.
    pub use_base_colour_space: bool,
    /// The three channels were stored separately.
    pub multichannel: bool,
    /// The base image is the HDR rendition (the gain map goes from HDR to SDR).
    pub backward_direction: bool,
}

/// Decoded 8-bit pixels, tightly packed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pixels {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// 1 (gray) or 4 (RGBA).
    pub channels: u8,
    /// `width * height * channels` bytes, rows without padding.
    pub data: Vec<u8>,
}

/// The decoded base image and gain map.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decoded {
    /// The base image, RGBA.
    pub base: Pixels,
    /// The gain map: gray for a 1-component image, else RGBA.
    pub gain_map: Option<Pixels>,
}

/// Picks the metadata of a gain map: valid ISO first, then valid `hdrgm` XMP. What was wrong goes
/// to `issues`; a missing source adds `metadata-missing` only when there is none at all.
pub(crate) fn resolve_metadata(
    iso: Option<&IsoMetadata>,
    xmp: Option<&HdrgmXmp>,
    limits: &Limits,
    issues: &mut Vec<&'static str>,
) -> Option<Metadata> {
    if let Some(iso) = iso {
        match Metadata::from_iso(iso, limits) {
            Ok(m) => return Some(m),
            Err(Error::Malformed(token)) => issues.push(token),
            Err(_) => issues.push("iso-minimum-version"),
        }
    }
    if let Some(xmp) = xmp {
        match Metadata::from_xmp(xmp, limits) {
            Ok(m) => return Some(m),
            Err(Error::Malformed(token)) => issues.push(token),
            Err(_) => issues.push("xmp-invalid"),
        }
    }
    if iso.is_none() && xmp.is_none() {
        issues.push("metadata-missing");
    }
    None
}
