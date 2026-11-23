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
    /// A JPEG with `hdrgm` XMP (Ultra HDR; the file may also carry ISO 21496-1), or a HEIF or AVIF
    /// file whose gain map is an Adobe `auxl` image (`urn:com:photo:aux:hdrgainmap`) with `hdrgm`
    /// XMP.
    UltraHdr,
    /// ISO 21496-1 only: a JPEG APP2 segment or a HEIF `tmap` item.
    Iso,
    /// An Apple `HDRGainMap` auxiliary image or XMP (also an Adobe auxiliary image that has no
    /// `hdrgm` XMP).
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

    /// The full HEIF graph (HEIF and AVIF only): items, transforms, colours, auxiliary images.
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

/// The chroma format of decoded [`Planes`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Chroma {
    /// 4:0:0, one plane (Y).
    Mono,
    /// 4:2:0, three planes (Y, Cb, Cr), chroma at half width and half height.
    Yuv420,
    /// 4:2:2, three planes, chroma at half width.
    Yuv422,
    /// 4:4:4, three planes.
    Yuv444,
}

/// The samples of one [`Plane`], rows without padding (stride is the plane width).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Samples {
    /// Samples of a plane with a bit depth of 8.
    U8(Vec<u8>),
    /// Samples of a plane with a bit depth above 8, in the low bits.
    U16(Vec<u16>),
}

/// One plane of decoded [`Planes`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plane {
    /// Width in samples.
    pub width: u32,
    /// Height in samples.
    pub height: u32,
    /// The bit depth of this plane (HEVC can code luma and chroma at different depths).
    pub bit_depth: u8,
    /// `width * height` samples.
    pub samples: Samples,
}

/// The colour description (CICP, ITU-T H.273 codes) that the bitstream carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cicp {
    /// `colour_primaries`.
    pub primaries: u8,
    /// `transfer_characteristics`.
    pub transfer: u8,
    /// `matrix_coefficients`.
    pub matrix: u8,
    /// Full range (`video_full_range_flag`, `color_range`) rather than studio range.
    pub full_range: bool,
}

/// One metadata message of the bitstream, raw: an HEVC SEI message or an AV1 metadata OBU.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodecMetadata {
    /// HEVC: the SEI `payloadType` (prefix or suffix NAL unit is not recorded). AV1: the
    /// `metadata_type`.
    pub kind: u64,
    /// The payload bytes as stored (HEVC without emulation prevention bytes).
    pub payload: Vec<u8>,
}

/// An image decoded by HEVC or AV1: the coded planes at their bit depth and chroma format, with
/// no colour conversion and no transform applied.
///
/// The transforms are not applied because their result is not one thing. `libheif` applies each
/// `irot`, `imir` and `clap` as it meets it and converts odd geometry to 4:4:4 first, and its
/// `clap` rounding differs from `libavif` and `isobmff`, which round nothing. [`Planes::transforms`]
/// lists them in `ipma` order for the caller, who also decides what an inexact `clap` means.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Planes {
    /// Luma width (a grid is assembled and cropped to its output size).
    pub width: u32,
    /// Luma height.
    pub height: u32,
    /// The chroma format.
    pub chroma: Chroma,
    /// The planes: Y, then Cb and Cr unless [`Chroma::Mono`].
    pub planes: Vec<Plane>,
    /// `clap`, `irot` and `imir` of the decoded item, raw, in `ipma` order, not applied.
    pub transforms: Vec<isobmff::Transform>,
    /// Every `colr` property of the decoded item, raw (an ICC profile is an extent of the input).
    pub colour: Vec<isobmff::Colour>,
    /// The colour description of the bitstream: the HEVC VUI (`None` when the SPS has none) or
    /// the AV1 `color_config`.
    pub cicp: Option<Cicp>,
    /// The SEI messages (HEVC, of the first tile of a grid) or metadata OBUs (AV1), raw.
    pub metadata: Vec<CodecMetadata>,
}

/// A decoded image: 8-bit pixels (JPEG) or coded planes (HEIF and AVIF).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Picture {
    /// A JPEG: RGBA, or gray for a one-component gain map.
    Pixels(Pixels),
    /// An HEIC or AVIF image.
    Planes(Planes),
}

impl Picture {
    /// The pixels of a JPEG picture.
    pub fn as_pixels(&self) -> Option<&Pixels> {
        match self {
            Self::Pixels(p) => Some(p),
            Self::Planes(_) => None,
        }
    }

    /// The planes of a HEIF or AVIF picture.
    pub fn as_planes(&self) -> Option<&Planes> {
        match self {
            Self::Planes(p) => Some(p),
            Self::Pixels(_) => None,
        }
    }

    /// Width in pixels (luma).
    pub fn width(&self) -> u32 {
        match self {
            Self::Pixels(p) => p.width,
            Self::Planes(p) => p.width,
        }
    }

    /// Height in pixels (luma).
    pub fn height(&self) -> u32 {
        match self {
            Self::Pixels(p) => p.height,
            Self::Planes(p) => p.height,
        }
    }
}

/// The decoded base image and gain map.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decoded {
    /// The base image.
    pub base: Picture,
    /// The gain map.
    pub gain_map: Option<Picture>,
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
