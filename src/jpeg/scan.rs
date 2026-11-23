//! A bounded walk over the markers of one JPEG image: from SOI to EOI, without decoding.

use std::ops::Range;

use crate::budget::Budget;
use crate::{Error, Limit, Limits};

const XMP_NS: &[u8] = b"http://ns.adobe.com/xap/1.0/\0";
const ISO_NS: &[u8] = b"urn:iso:std:iso:ts:21496:-1\0";
const MPF_SIG: &[u8] = b"MPF\0";

/// What the SOF marker says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Sof {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) components: u8,
}

/// What one walk found. Ranges are absolute offsets in the input.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct Scan {
    /// Just after the EOI marker.
    pub(crate) end: usize,
    /// The first SOF.
    pub(crate) sof: Option<Sof>,
    /// The payload of the first standard XMP APP1 segment (after the namespace).
    pub(crate) xmp: Option<Range<usize>>,
    /// The payload of the first MPF APP2 segment, from the MP endian field (after `MPF\0`).
    pub(crate) mpf: Option<Range<usize>>,
    /// The payload of the first ISO 21496-1 APP2 segment (after the namespace).
    pub(crate) iso: Option<Range<usize>>,
}

fn is_sof(marker: u8) -> bool {
    matches!(marker, 0xC0..=0xC3 | 0xC5..=0xC7 | 0xC9..=0xCB | 0xCD..=0xCF)
}

/// Skips entropy-coded data from `pos` to the next marker that is not a stuffed byte, a restart
/// marker or a fill byte; returns its offset.
fn skip_entropy(data: &[u8], mut pos: usize, budget: &Budget<'_>) -> Result<usize, Error> {
    let mut ticks = 0usize;
    loop {
        let rest = data.get(pos..).ok_or(Error::Truncated)?;
        pos += rest
            .iter()
            .position(|&b| b == 0xFF)
            .ok_or(Error::Truncated)?;
        ticks += 1;
        if ticks.is_multiple_of(1024) {
            budget.check_cancel()?;
        }
        match data.get(pos + 1) {
            None => return Err(Error::Truncated),
            Some(0x00 | 0xD0..=0xD7) => pos += 2,
            Some(0xFF) => pos += 1,
            Some(_) => return Ok(pos),
        }
    }
}

/// Walks the image that starts at `start` (which must be an SOI marker) up to its EOI marker.
/// Allocates nothing. The cancel flag is checked every 1024 markers and every 1024 stuffed
/// bytes of entropy data.
///
/// # Errors
///
/// [`Error::Malformed`] when `start` is not an SOI or a marker is invalid; [`Error::Truncated`]
/// when the data ends before the EOI; [`Error::LimitExceeded`] with `Segments` after
/// `limits.max_segments` markers with a length; [`Error::Cancelled`].
pub(crate) fn scan(
    data: &[u8],
    start: usize,
    limits: &Limits,
    budget: &Budget<'_>,
) -> Result<Scan, Error> {
    if data.get(start..start.saturating_add(2)) != Some(&[0xFF, 0xD8][..]) {
        return Err(Error::Malformed("jpeg-soi"));
    }
    let mut pos = start + 2;
    let mut out = Scan::default();
    let mut segments = 0usize;
    loop {
        if *data.get(pos).ok_or(Error::Truncated)? != 0xFF {
            return Err(Error::Malformed("jpeg-marker"));
        }
        // Fill bytes: any number of 0xFF before the marker code.
        while data.get(pos + 1) == Some(&0xFF) {
            pos += 1;
        }
        let marker = *data.get(pos + 1).ok_or(Error::Truncated)?;
        pos += 2;
        match marker {
            0xD9 => {
                out.end = pos;
                return Ok(out);
            }
            0x01 | 0xD0..=0xD7 => continue,
            0x00 | 0xD8 => return Err(Error::Malformed("jpeg-marker")),
            _ => {}
        }
        segments += 1;
        if segments > limits.max_segments {
            return Err(Error::LimitExceeded(Limit::Segments));
        }
        if segments.is_multiple_of(1024) {
            budget.check_cancel()?;
        }
        let len = data.get(pos..pos + 2).ok_or(Error::Truncated)?;
        let len = usize::from(u16::from_be_bytes([len[0], len[1]]));
        if len < 2 {
            return Err(Error::Malformed("jpeg-length"));
        }
        let body_start = pos + 2;
        let body_end = pos + len;
        let body = data.get(body_start..body_end).ok_or(Error::Truncated)?;
        match marker {
            0xE1 if out.xmp.is_none() && body.starts_with(XMP_NS) => {
                out.xmp = Some(body_start + XMP_NS.len()..body_end);
            }
            0xE2 if out.mpf.is_none() && body.starts_with(MPF_SIG) => {
                out.mpf = Some(body_start + MPF_SIG.len()..body_end);
            }
            0xE2 if out.iso.is_none() && body.starts_with(ISO_NS) => {
                out.iso = Some(body_start + ISO_NS.len()..body_end);
            }
            m if is_sof(m) && out.sof.is_none() && body.len() >= 6 => {
                out.sof = Some(Sof {
                    height: u32::from(u16::from_be_bytes([body[1], body[2]])),
                    width: u32::from(u16::from_be_bytes([body[3], body[4]])),
                    components: body[5],
                });
            }
            _ => {}
        }
        pos = body_end;
        if marker == 0xDA {
            pos = skip_entropy(data, pos, budget)?;
        }
    }
}
