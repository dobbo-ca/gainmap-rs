//! gainmap
//! Reads Ultra HDR and ISO 21496-1 gain maps from JPEG, HEIF and AVIF files: the SDR base image,
//! the gain-map image and the metadata that says how to combine them.
//!
//! [`read`] finds the gain map without decoding anything: in a JPEG through the MPF index, the
//! Google container directory or the image that follows the primary one; in HEIF and AVIF
//! through the `tmap` item, the Apple auxiliary image or the Adobe one
//! (`urn:com:photo:aux:hdrgainmap`). The metadata comes from the ISO 21496-1
//! payload when it is valid, else from the `hdrgm` XMP. A gain map is never dropped: if its
//! metadata is missing or invalid, [`GainMap::metadata`] is `None` and [`GainMap::issues`] says
//! why. [`decode`] decodes the base image and the gain map: a JPEG to RGBA or gray pixels
//! ([`Picture::Pixels`]), an HEIC (HEVC) or AVIF (AV1) image to its coded Y, Cb and Cr planes
//! ([`Picture::Planes`]) with the transforms, `colr` properties and codec metadata, raw. It
//! converts no colour, applies no transform and does not apply the gain map (see
//! docs/design.md). The Cargo features `heif` and `avif` (both on by default) pull in the HEVC and
//! AV1 decoders; with one off, `decode` gives [`Error::Unsupported`] for that codec.
//!
//! # Example
//!
//! ```
//! use std::sync::atomic::AtomicBool;
//! use gainmap::{Error, Limits};
//!
//! let limits = Limits { max_threads: 1, max_alloc: 64 << 20, ..Limits::default() };
//! let cancel = AtomicBool::new(false);
//! # let file: &[u8] = b"not an image";
//! match gainmap::read(file, &limits, &cancel) {
//!     Ok(image) => {
//!         if let Some(gm) = &image.gain_map {
//!             println!("{:?} {:?} {:?}", gm.kind, gm.metadata, gm.issues);
//!             let pixels = gainmap::decode(file, &image, &limits, &cancel)?;
//!             println!("{}x{}", pixels.base.width(), pixels.base.height());
//!         }
//!     }
//!     Err(e) => println!("not readable: {}", e.class()),
//! }
//! # assert_eq!(gainmap::read(file, &limits, &cancel), Err(Error::UnknownFormat));
//! # Ok::<(), Error>(())
//! ```
//!
//! # Resource limits
//!
//! [`read`], [`decode`] and [`GainMapImage::coded_bytes`] take [`Limits`] and a cancel flag
//! (`&AtomicBool`). [`IsoMetadata::parse`], [`Metadata::from_iso`] and [`Metadata::from_xmp`] take
//! [`Limits`] only (they allocate nothing and are bounded by their input).
//!
//! - `max_alloc` (default 1 GiB) is the enforced peak of the working memory of one call,
//!   including the nested calls into `jpeg`, `exif`, `isobmff`, `hevc` and `av1`, which run under
//!   what is left; the planes and metadata that [`decode`] returns are charged to it.
//!   More is [`Error::LimitExceeded`]`(`[`Limit::Alloc`]`)`. One exception is closed on purpose:
//!   `jpeg` reads a `max_alloc` of exactly 1 GiB as its unset default and may then exceed it by
//!   page rounding, so [`decode`] never passes exactly 1 GiB to it; it passes one byte less.
//! - `max_threads` (default `available_parallelism`) is the most threads one call uses, the
//!   calling thread included. [`read`] spawns none. [`decode`] passes the value to the JPEG and
//!   AV1 decoders (the HEVC decoder is serial); 1 runs on the calling thread.
//! - `max_pixels` (per image), `max_xmp_bytes`, `max_segments` (JPEG markers per image) and
//!   `max_images` (MPF entries and candidate images) each give [`Error::LimitExceeded`] with
//!   their own [`Limit`].
//! - Set the cancel flag from another thread to stop a call: it returns [`Error::Cancelled`].

#![forbid(unsafe_code)]
#![cfg_attr(
    not(test),
    deny(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::todo,
        clippy::unimplemented
    )
)]

use std::sync::atomic::AtomicBool;

mod budget;
mod decode;
mod error;
mod heif;
mod iso;
mod jpeg;
mod limits;
mod types;
mod xmp;

pub use decode::decode;
pub use error::{Error, Limit};
pub use iso::{Fraction, IsoChannel, IsoMetadata};
pub use limits::Limits;
pub use types::{
    AppleGainMap, Chroma, Cicp, Codec, CodecMetadata, CodedImage, Container, Decoded, GainMap,
    GainMapImage, HdrgmXmp, Kind, Location, Metadata, Picture, Pixels, Plane, Planes, Samples,
};

use budget::Budget;

/// Reads the base image and the gain map of a JPEG, HEIF or AVIF file. Nothing is decoded:
/// the result says where the coded images are ([`GainMapImage::coded_bytes`]) and what the
/// metadata is. `gain_map` is `None` when the file has no gain map.
///
/// A gain map whose metadata is missing or invalid is still returned, with
/// [`GainMap::metadata`] `None` and the reason in [`GainMap::issues`]. A malformed XMP packet or
/// MPF index is an issue, not an error.
///
/// # Errors
///
/// [`Error::UnknownFormat`] when `data` is not a JPEG or a file with an `ftyp` box;
/// [`Error::Truncated`] or [`Error::Malformed`] when the primary JPEG has no EOI or SOF, or is
/// otherwise invalid; [`Error::Heif`] for an invalid HEIF structure;
/// [`Error::LimitExceeded`] for `max_alloc`, `max_pixels`, `max_xmp_bytes`, `max_segments` and
/// `max_images`; [`Error::OutOfMemory`]; [`Error::Cancelled`].
pub fn read(data: &[u8], limits: &Limits, cancel: &AtomicBool) -> Result<GainMapImage, Error> {
    let budget = Budget::new(limits, cancel);
    budget.check_cancel()?;
    if data.starts_with(&[0xFF, 0xD8]) {
        crate::jpeg::read(data, limits, &budget)
    } else if data.get(4..8) == Some(b"ftyp".as_slice()) {
        heif::read(data, limits, &budget)
    } else {
        Err(Error::UnknownFormat)
    }
}

/// Entry points for the fuzz targets. Not part of the API.
#[doc(hidden)]
pub mod __fuzz {
    use std::sync::atomic::AtomicBool;

    use crate::budget::Budget;
    use crate::{Error, HdrgmXmp, Limits, Metadata};

    /// Walks the markers of the JPEG at the start of `data`; returns the offset after its EOI.
    pub fn jpeg_scan(data: &[u8], limits: &Limits) -> Result<usize, Error> {
        let cancel = AtomicBool::new(false);
        let budget = Budget::new(limits, &cancel);
        crate::jpeg::scan::scan(data, 0, limits, &budget).map(|s| s.end)
    }

    /// Parses an MPF payload (from the endian field, after `MPF\0`); returns the entry count.
    pub fn mpf(data: &[u8]) -> Result<usize, Error> {
        let limits = Limits::default();
        let cancel = AtomicBool::new(false);
        let budget = Budget::new(&limits, &cancel);
        crate::jpeg::mpf::parse(data, limits.max_images, &budget).map(|e| e.len())
    }

    /// Parses an XMP packet and returns its `hdrgm` values; also converts them to metadata.
    pub fn hdrgm_from_packet(data: &[u8], limits: &Limits) -> Result<Option<HdrgmXmp>, Error> {
        let cancel = AtomicBool::new(false);
        let budget = Budget::new(limits, &cancel);
        let mut issues = Vec::new();
        let Some(parsed) = crate::xmp::parse(data, limits, &budget, &mut issues)? else {
            return Ok(None);
        };
        let hdrgm = crate::xmp::hdrgm_from_xmp(&parsed.xmp, &budget)?;
        if let Some(h) = &hdrgm {
            let _ = Metadata::from_xmp(h, limits);
        }
        Ok(hdrgm)
    }
}
