//! Pixel decode of the base image and the gain map: JPEG to pixels, HEIC (HEVC) and AVIF (AV1) to
//! coded planes.

use std::sync::atomic::AtomicBool;

use jpeg::{DecodeOptions as JpegOptions, Format, Image};

use crate::budget::Budget;
#[cfg(any(feature = "heif", feature = "avif"))]
use crate::{Chroma, CodecMetadata, Plane, Planes, Samples};
use crate::{Codec, CodedImage, Decoded, Error, GainMapImage, Limits, Location, Picture, Pixels};

/// The pixel cap that `hevc` and `av1` were built and fuzzed with. A larger `max_pixels` of ours
/// is lowered to it for those two.
#[cfg(any(feature = "heif", feature = "avif"))]
const CODEC_MAX_PIXELS: u64 = 1 << 28;

/// What [`decode_with`] does beyond the raw decode. The default is the raw decode of [`decode`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DecodeOptions {
    /// Apply the `clap`, `irot` and `imir` transforms to the planes of every HEIC and AVIF picture
    /// (the base and the gain map), through the `isobmff` transform helper. For an `iden` item
    /// the input items' transforms run first, innermost first, then the item's own. The planes'
    /// `width` and `height` become the result's, a 4:2:2 image rotated by 90 or 270 degrees
    /// becomes [`Chroma::Yuv444`](crate::Chroma::Yuv444), and [`Planes::transforms`](crate::Planes::transforms) still lists the transforms (they
    /// are never dropped); [`Planes::transforms_applied`](crate::Planes::transforms_applied) says they are applied. The result can
    /// differ from libheif where it converts the chroma format for odd geometry
    /// (isobmff-rs docs/transforms.md). JPEG pictures have none of these properties and are
    /// unaffected. A grid whose tiles have transforms of their own is
    /// [`Error::Unsupported`]`("grid-tile-transforms")`: the codec assembles the tiles raw.
    /// Default `false`: the planes are raw.
    pub apply_transforms: bool,
}

/// Decodes the base image and the gain map. A JPEG gives [`Picture::Pixels`]: the base as RGBA,
/// the gain map as gray (1 component) or RGBA (3 components). An HEIC or AVIF image gives
/// [`Picture::Planes`]: the coded Y, Cb and Cr planes at their bit depth and chroma format,
/// with the transforms, the `colr` properties and the codec metadata attached and nothing
/// converted or applied.
///
/// The base stays alive while the gain map decodes, so the gain map gets the budget that is
/// left: each codec runs under `max_alloc` minus what the first picture holds, and the planes
/// it returns are charged to it. The two decodes run one after the other; each uses up to
/// `limits.max_threads` threads inside `jpeg` or `av1` (`hevc` decodes on the calling thread).
/// `max_pixels` is the smaller of ours and 2^28, the cap of `hevc` and `av1`; their width and
/// height defaults apply. `max_images` bounds the `tmap` items and `altr` groups of the item and
/// of every tile of a grid.
///
/// A JPEG that decodes with an error beside its pixels (`jpeg::Decoded::error`, a truncated scan)
/// is an error here: [`Error::Jpeg`].
///
/// # Errors
///
/// [`Error::Unsupported`] (`"hevc decode"` or `"av1 decode"`) when the Cargo feature `heif` or
/// `avif` is off; [`Error::Malformed`] when `image` has no byte range or item id;
/// [`Error::Truncated`] when the range is not inside `data`; [`Error::LimitExceeded`] for
/// `max_alloc`, `max_pixels` and `max_images` (`Limit::Images`; the `hevc` and `av1` limit errors
/// map to the first two); [`Error::Unsupported`] (`"heif-identity-depth"`) for an `iden` chain
/// deeper than 8; [`Error::Malformed`] (`"planes"`, `"av1-chroma"`) when a codec returns planes
/// that do not fit their chroma format; [`Error::OutOfMemory`]; [`Error::Cancelled`] (checked between the two decodes and by the
/// codec inside each); [`Error::Heif`], [`Error::Jpeg`], [`Error::Hevc`] and [`Error::Av1`] for
/// the other errors. `data` must be the input that [`read`](crate::read) got.
pub fn decode(
    data: &[u8],
    image: &GainMapImage,
    limits: &Limits,
    cancel: &AtomicBool,
) -> Result<Decoded, Error> {
    decode_with(data, image, &DecodeOptions::default(), limits, cancel)
}

/// [`decode`] with [`DecodeOptions`]. With the default options it is `decode`. With
/// [`DecodeOptions::apply_transforms`] each HEIC or AVIF picture's planes go through the
/// transforms before they are returned ([`Planes::transforms_applied`](crate::Planes::transforms_applied) is then `true`); the
/// planes are charged to `max_alloc` as before, and the transform buffers are charged on top of
/// them while they exist (the helper runs under what is left). The extra errors are
/// [`Error::Unsupported`] (`"grid-tile-transforms"`) and [`Error::Malformed`] (`"clap"`,
/// `"irot"`) for a transform the helper rejects.
///
/// # Errors
///
/// As [`decode`].
pub fn decode_with(
    data: &[u8],
    image: &GainMapImage,
    options: &DecodeOptions,
    limits: &Limits,
    cancel: &AtomicBool,
) -> Result<Decoded, Error> {
    let budget = Budget::new(limits, cancel);
    budget.check_cancel()?;
    let gain = image.gain_map.as_ref().map(|g| &g.image);
    for img in std::iter::once(&image.base).chain(gain) {
        match img.codec {
            Codec::Jpeg => {}
            #[cfg(not(feature = "heif"))]
            Codec::Hevc => return Err(Error::Unsupported("hevc decode")),
            #[cfg(not(feature = "avif"))]
            Codec::Av1 => return Err(Error::Unsupported("av1 decode")),
            #[cfg(feature = "heif")]
            Codec::Hevc => {}
            #[cfg(feature = "avif")]
            Codec::Av1 => {}
        }
    }
    let base = decode_one(data, image, &image.base, false, options, limits, &budget)?;
    budget.check_cancel()?;
    let gain_map = match gain {
        Some(g) => Some(decode_one(data, image, g, true, options, limits, &budget)?),
        None => None,
    };
    Ok(Decoded { base, gain_map })
}

/// Decodes one coded image by its codec.
fn decode_one(
    data: &[u8],
    whole: &GainMapImage,
    img: &CodedImage,
    is_gain_map: bool,
    options: &DecodeOptions,
    limits: &Limits,
    budget: &Budget<'_>,
) -> Result<Picture, Error> {
    match img.codec {
        Codec::Jpeg => {
            let format = if is_gain_map && img.components == 1 {
                Format::Gray
            } else {
                Format::Rgba
            };
            decode_jpeg(data, img, format, limits, budget).map(Picture::Pixels)
        }
        #[cfg(feature = "heif")]
        Codec::Hevc => decode_heif_item(whole, img, options, limits, budget, |heif, id, item| {
            hevc_planes(data, heif, id, item, limits, budget)
        }),
        #[cfg(not(feature = "heif"))]
        Codec::Hevc => {
            let _ = (whole, options);
            Err(Error::Unsupported("hevc decode"))
        }
        #[cfg(feature = "avif")]
        Codec::Av1 => decode_heif_item(whole, img, options, limits, budget, |heif, id, item| {
            av1_planes(data, heif, id, item, limits, budget)
        }),
        #[cfg(not(feature = "avif"))]
        Codec::Av1 => {
            let _ = (whole, options);
            Err(Error::Unsupported("av1 decode"))
        }
    }
}

/// Decodes a HEIF or AVIF item with `codec` and, when `options` ask for it, applies the
/// transforms to the planes. The `iden` inputs' transforms and the grid-tile check come first, so
/// an unsupported grid fails before the codec runs.
#[cfg(any(feature = "heif", feature = "avif"))]
fn decode_heif_item(
    whole: &GainMapImage,
    img: &CodedImage,
    options: &DecodeOptions,
    limits: &Limits,
    budget: &Budget<'_>,
    codec: impl FnOnce(&isobmff::Heif, u32, isobmff::Image) -> Result<Planes, Error>,
) -> Result<Picture, Error> {
    let (heif, id, item, cost) = heif_item(whole, img, limits, budget)?;
    let inner = match (options.apply_transforms, &img.location) {
        (true, Location::Item(outer)) => {
            crate::transforms::inner_chain(heif, *outer, limits, budget).map(Some)
        }
        (true, _) => Err(Error::Malformed("location")),
        (false, _) => Ok(None),
    };
    let inner = match inner {
        Ok(inner) => inner,
        Err(e) => {
            budget.release(cost);
            return Err(e);
        }
    };
    let planes = codec(heif, id, item);
    budget.release(cost);
    let planes = planes.and_then(|p| match inner {
        Some(chain) => crate::transforms::apply(p, chain, limits, budget),
        None => Ok(p),
    });
    planes.map(Picture::Planes)
}

/// The graph, the id of the coded or grid item to decode, the parsed item (for its transforms and
/// colours) and the bytes charged for the `isobmff::Image` values (live at once: this one, the one
/// the codec builds and, for a grid, the tile images: one for `hevc`, up to `max_threads` for
/// `av1`, bounded here by 64); the caller releases them when the codec has returned.
#[cfg(any(feature = "heif", feature = "avif"))]
fn heif_item<'a>(
    whole: &'a GainMapImage,
    img: &CodedImage,
    limits: &Limits,
    budget: &Budget<'_>,
) -> Result<(&'a isobmff::Heif, u32, isobmff::Image, usize), Error> {
    let heif = whole.heif.as_ref().ok_or(Error::Malformed("location"))?;
    let Location::Item(id) = img.location else {
        return Err(Error::Malformed("location"));
    };
    let cost = crate::heif::image_cost(heif, 2 + limits.max_threads.clamp(1, 64));
    budget.charge(cost)?;
    match resolve_item(heif, id, limits, budget) {
        Ok((coded, item)) => Ok((heif, coded, item, cost)),
        Err(e) => {
            budget.release(cost);
            Err(e)
        }
    }
}

/// The item `id` itself (for its transforms and colours) and the id the codec decodes: an `iden`
/// chain is followed to its input, because `hevc` takes only coded and grid items and `av1` only
/// one `iden` level. Every tile of a grid is counted like `id`, since the codecs call
/// `Heif::image` on each tile and that call is not cancellable.
#[cfg(any(feature = "heif", feature = "avif"))]
fn resolve_item(
    heif: &isobmff::Heif,
    id: u32,
    limits: &Limits,
    budget: &Budget<'_>,
) -> Result<(u32, isobmff::Image), Error> {
    let item = crate::heif::checked_image(heif, id, limits, budget)?;
    let mut coded = id;
    let mut input = match &item.kind {
        isobmff::ImageKind::Identity { input } => Some(*input),
        _ => None,
    };
    // Only `item` and `tail` are live at once, which is what `image_cost` bounds.
    let mut tail: Option<isobmff::Image> = None;
    for _ in 0..=crate::heif::IDEN_DEPTH {
        let Some(i) = input else {
            break;
        };
        coded = i;
        drop(tail.take());
        let next = crate::heif::checked_image(heif, i, limits, budget)?;
        input = match &next.kind {
            isobmff::ImageKind::Identity { input } => Some(*input),
            _ => None,
        };
        tail = Some(next);
    }
    if input.is_some() {
        return Err(Error::Unsupported("heif-identity-depth"));
    }
    if let isobmff::ImageKind::Grid(g) = &tail.as_ref().unwrap_or(&item).kind {
        crate::heif::check_ids(heif, &g.tiles, limits, budget)?;
    }
    Ok((coded, item))
}

/// The `max_pixels` to pass to a codec.
#[cfg(any(feature = "heif", feature = "avif"))]
fn codec_pixels(limits: &Limits) -> u64 {
    limits.max_pixels.min(CODEC_MAX_PIXELS)
}

#[cfg(feature = "heif")]
fn hevc_planes(
    data: &[u8],
    heif: &isobmff::Heif,
    id: u32,
    item: isobmff::Image,
    limits: &Limits,
    budget: &Budget<'_>,
) -> Result<Planes, Error> {
    let nested = hevc::Limits {
        max_alloc: budget.remaining(),
        max_threads: limits.max_threads,
        max_pixels: codec_pixels(limits),
        // The SEI is booked in the codec's ledger, so with this it is kept or the decode fails;
        // the default 1 MiB would drop messages without an error.
        max_metadata: budget.remaining(),
        ..hevc::Limits::default()
    };
    let pic = hevc::decode_heic_item(
        heif,
        data,
        id,
        &hevc::Options::default(),
        &nested,
        budget.cancel(),
    )?;
    // The codec ran under what was left, so the picture fits; charge it so the next decode runs
    // under the rest.
    let mut held = pic.sei.len().saturating_mul(size_of::<CodecMetadata>());
    for m in &pic.sei {
        held = held.saturating_add(m.payload.len());
    }
    for p in &pic.planes {
        held = held.saturating_add(match &p.samples {
            hevc::Samples::U8(v) => v.len(),
            hevc::Samples::U16(v) => v.len().saturating_mul(2),
        });
    }
    held = held.saturating_add(properties_bytes(&item));
    budget.charge(held)?;
    let chroma = match pic.info.chroma_format {
        hevc::ChromaFormat::Mono => Chroma::Mono,
        hevc::ChromaFormat::Yuv420 => Chroma::Yuv420,
    };
    let (depth_luma, depth_chroma) = (pic.info.bit_depth_luma, pic.info.bit_depth_chroma);
    let cicp = pic.info.vui.map(|v| crate::Cicp {
        primaries: v.colour_primaries,
        transfer: v.transfer_characteristics,
        matrix: v.matrix_coeffs,
        full_range: v.video_full_range,
    });
    let metadata = pic
        .sei
        .into_iter()
        .map(|m| CodecMetadata {
            kind: u64::from(m.payload_type),
            payload: m.payload,
        })
        .collect();
    let planes: Vec<Plane> = pic
        .planes
        .into_iter()
        .enumerate()
        .map(|(i, p)| Plane {
            width: p.width,
            height: p.height,
            bit_depth: if i == 0 { depth_luma } else { depth_chroma },
            samples: match p.samples {
                hevc::Samples::U8(v) => Samples::U8(v),
                hevc::Samples::U16(v) => Samples::U16(v),
            },
        })
        .collect();
    finish(planes, chroma, item, cicp, metadata)
}

#[cfg(feature = "avif")]
fn av1_planes(
    data: &[u8],
    heif: &isobmff::Heif,
    id: u32,
    item: isobmff::Image,
    limits: &Limits,
    budget: &Budget<'_>,
) -> Result<Planes, Error> {
    // `av1` books the metadata it keeps in its ledger (all decoders of the call), so `max_alloc`
    // is the whole peak.
    let nested = av1::Limits {
        max_alloc: budget.remaining(),
        max_threads: limits.max_threads,
        max_pixels: codec_pixels(limits),
        ..av1::Limits::default()
    };
    let frame = av1::decode_avif_item(
        heif,
        data,
        id,
        &av1::Options::default(),
        &nested,
        budget.cancel(),
    )?;
    let mut held = frame
        .metadata
        .len()
        .saturating_mul(size_of::<CodecMetadata>());
    for m in &frame.metadata {
        held = held.saturating_add(m.payload.len());
    }
    for p in &frame.planes {
        held = held.saturating_add(match &p.data {
            av1::PlaneData::U8(v) => v.len(),
            av1::PlaneData::U16(v) => v.len().saturating_mul(2),
        });
    }
    held = held.saturating_add(properties_bytes(&item));
    budget.charge(held)?;
    let chroma = match (frame.mono_chrome, frame.subsampling_x, frame.subsampling_y) {
        (true, _, _) => Chroma::Mono,
        (false, true, true) => Chroma::Yuv420,
        (false, true, false) => Chroma::Yuv422,
        (false, false, false) => Chroma::Yuv444,
        // 4:4:0 is not a legal AV1 format.
        (false, false, true) => return Err(Error::Malformed("av1-chroma")),
    };
    let cicp = Some(crate::Cicp {
        primaries: frame.color.color_primaries,
        transfer: frame.color.transfer_characteristics,
        matrix: frame.color.matrix_coefficients,
        full_range: frame.color.color_range,
    });
    let depth = frame.bit_depth;
    let metadata = frame
        .metadata
        .into_iter()
        .map(|m| CodecMetadata {
            kind: m.metadata_type,
            payload: m.payload,
        })
        .collect();
    let planes: Vec<Plane> = frame
        .planes
        .into_iter()
        .map(|p| Plane {
            width: p.width,
            height: p.height,
            bit_depth: depth,
            samples: match p.data {
                av1::PlaneData::U8(v) => Samples::U8(v),
                av1::PlaneData::U16(v) => Samples::U16(v),
            },
        })
        .collect();
    finish(planes, chroma, item, cicp, metadata)
}

/// The bytes of the transforms and colours that `finish` moves into the planes.
#[cfg(any(feature = "heif", feature = "avif"))]
fn properties_bytes(item: &isobmff::Image) -> usize {
    item.transforms
        .capacity()
        .saturating_mul(size_of::<isobmff::Transform>())
        .saturating_add(
            item.colour
                .capacity()
                .saturating_mul(size_of::<isobmff::Colour>()),
        )
}

/// Puts the decoded planes and the properties of the item together.
#[cfg(any(feature = "heif", feature = "avif"))]
fn finish(
    planes: Vec<Plane>,
    chroma: Chroma,
    item: isobmff::Image,
    cicp: Option<crate::Cicp>,
    metadata: Vec<CodecMetadata>,
) -> Result<Planes, Error> {
    let want = if chroma == Chroma::Mono { 1 } else { 3 };
    let luma = planes.first().ok_or(Error::Malformed("planes"))?;
    if planes.len() != want {
        return Err(Error::Malformed("planes"));
    }
    Ok(Planes {
        width: luma.width,
        height: luma.height,
        chroma,
        planes,
        transforms: item.transforms,
        colour: item.colour,
        cicp,
        metadata,
        transforms_applied: false,
    })
}

/// Decodes one JPEG range into tightly packed pixels. The pixels stay charged to `budget`.
fn decode_jpeg(
    data: &[u8],
    img: &CodedImage,
    format: Format,
    limits: &Limits,
    budget: &Budget<'_>,
) -> Result<Pixels, Error> {
    let Location::Range(range) = &img.location else {
        return Err(Error::Malformed("location"));
    };
    let bytes = data.get(range.clone()).ok_or(Error::Truncated)?;
    // jpeg-rs reads a `max_alloc` of exactly 1 GiB as its unset default and may then exceed it by
    // the page rounding of its ledger. One byte less is a hard cap, so the nested cap never
    // equals that value.
    const JPEG_UNSET_MAX_ALLOC: u64 = 1 << 30;
    let remaining = u64::try_from(budget.remaining()).unwrap_or(u64::MAX);
    let nested = jpeg::Limits {
        max_pixels: limits.max_pixels,
        max_alloc: if remaining == JPEG_UNSET_MAX_ALLOC {
            remaining - 1
        } else {
            remaining
        },
        max_threads: limits.max_threads,
    };
    let options = JpegOptions {
        format,
        ..JpegOptions::default()
    };
    let decoded = jpeg::decode(bytes, &options, &nested, budget.cancel())?;
    if let Some(e) = decoded.error {
        return Err(Error::from(e));
    }
    let (rect, channels, pix, stride) = match decoded.image {
        Image::Gray(g) => (g.rect, 1usize, g.pix, g.stride),
        Image::Rgba(r) => (r.rect, 4usize, r.pix, r.stride),
        Image::YCbCr(_) => return Err(Error::Malformed("jpeg-format")),
    };
    let width = u32::try_from(rect.width()).map_err(|_| Error::Malformed("jpeg-size"))?;
    let height = u32::try_from(rect.height()).map_err(|_| Error::Malformed("jpeg-size"))?;
    let row = usize::try_from(width)
        .ok()
        .and_then(|w| w.checked_mul(channels))
        .ok_or(Error::Malformed("jpeg-size"))?;
    let rows = usize::try_from(height).map_err(|_| Error::Malformed("jpeg-size"))?;
    let total = row
        .checked_mul(rows)
        .ok_or(Error::LimitExceeded(crate::Limit::Alloc))?;
    let held = pix.capacity();
    budget.charge(held)?;
    let data = if stride == row && pix.len() == total {
        pix
    } else {
        // Rows with padding: copy them tight, then give the padded buffer back.
        if stride < row || row == 0 || pix.len() < total {
            return Err(Error::Malformed("jpeg-stride"));
        }
        let mut tight = budget.vec::<u8>(total)?;
        for line in pix.chunks(stride).take(rows) {
            tight.extend_from_slice(line.get(..row).ok_or(Error::Malformed("jpeg-stride"))?);
        }
        drop(pix);
        budget.release(held);
        tight
    };
    Ok(Pixels {
        width,
        height,
        channels: if channels == 1 { 1 } else { 4 },
        data,
    })
}
