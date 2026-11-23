//! Pixel decode of the base image and the gain map (JPEG only for now).

use std::sync::atomic::AtomicBool;

use jpeg::{DecodeOptions, Format, Image};

use crate::budget::Budget;
use crate::{Codec, CodedImage, Decoded, Error, GainMapImage, Limits, Location, Pixels};

/// Decodes the base image to RGBA and the gain map to gray (1 component) or RGBA (3 components).
/// The base pixels stay alive while the gain map decodes, so the gain map gets the budget that is
/// left. Both decodes run one after the other; each uses up to `limits.max_threads` threads
/// inside `jpeg`.
///
/// A JPEG that decodes with an error beside its pixels (`jpeg::Decoded::error`, a truncated scan)
/// is an error here: [`Error::Jpeg`].
///
/// # Errors
///
/// [`Error::Unsupported`] (`"hevc decode"` or `"av1 decode"`) for HEIF and AVIF until the HEVC
/// and AV1 decoders exist; [`Error::Malformed`] when `image` has no byte range;
/// [`Error::Truncated`] when the range is not inside `data`; [`Error::LimitExceeded`] for
/// `max_alloc` and `max_pixels`; [`Error::OutOfMemory`]; [`Error::Cancelled`] (checked between
/// the two decodes and by `jpeg` per MCU row); [`Error::Jpeg`] for the other decoder errors.
pub fn decode(
    data: &[u8],
    image: &GainMapImage,
    limits: &Limits,
    cancel: &AtomicBool,
) -> Result<Decoded, Error> {
    let budget = Budget::new(limits, cancel);
    budget.check_cancel()?;
    let gain = image.gain_map.as_ref().map(|g| &g.image);
    for img in std::iter::once(&image.base).chain(gain) {
        match img.codec {
            Codec::Jpeg => {}
            Codec::Hevc => return Err(Error::Unsupported("hevc decode")),
            Codec::Av1 => return Err(Error::Unsupported("av1 decode")),
        }
    }
    let base = decode_jpeg(data, &image.base, Format::Rgba, limits, &budget)?;
    budget.check_cancel()?;
    let gain_map = match gain {
        Some(g) => {
            let format = if g.components == 1 {
                Format::Gray
            } else {
                Format::Rgba
            };
            Some(decode_jpeg(data, g, format, limits, &budget)?)
        }
        None => None,
    };
    Ok(Decoded { base, gain_map })
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
    let options = DecodeOptions {
        format,
        ..DecodeOptions::default()
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
