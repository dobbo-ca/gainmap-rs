//! The Multi-Picture Format index (CIPA DC-007) of an MPF APP2 segment.

use crate::budget::Budget;
use crate::{Error, Limit};

/// One MP entry. `offset` is relative to the MP endian field; 0 is the first image. The size in
/// the entry is not kept: a writer can get it wrong, and the walk of the image finds its EOI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct MpEntry {
    pub(crate) offset: u32,
}

struct Reader<'a> {
    data: &'a [u8],
    big: bool,
}

impl Reader<'_> {
    fn u16(&self, at: usize) -> Option<u16> {
        let b = self.data.get(at..at.checked_add(2)?)?;
        let b = [b[0], b[1]];
        Some(if self.big {
            u16::from_be_bytes(b)
        } else {
            u16::from_le_bytes(b)
        })
    }

    fn u32(&self, at: usize) -> Option<u32> {
        let b = self.data.get(at..at.checked_add(4)?)?;
        let b = [b[0], b[1], b[2], b[3]];
        Some(if self.big {
            u32::from_be_bytes(b)
        } else {
            u32::from_le_bytes(b)
        })
    }
}

/// Parses the MP index. `mpf` starts at the MP endian field (`MM\0*` or `II*\0`), after the
/// `MPF\0` signature. Only tag 0xB002 (the MP entries) is used.
///
/// # Errors
///
/// [`Error::Malformed`] for a bad endian field, a tag table or entry list outside the segment,
/// or no entry tag; [`Error::LimitExceeded`] with `Images` when the entry count is over
/// `max_images`; the budget errors for the entry list.
pub(crate) fn parse(
    mpf: &[u8],
    max_images: usize,
    budget: &Budget<'_>,
) -> Result<Vec<MpEntry>, Error> {
    let big = match mpf.get(..4) {
        Some(b"MM\0*") => true,
        Some(b"II*\0") => false,
        _ => return Err(Error::Malformed("mpf-endian")),
    };
    let r = Reader { data: mpf, big };
    let ifd = r
        .u32(4)
        .and_then(|v| usize::try_from(v).ok())
        .filter(|&v| v >= 8)
        .ok_or(Error::Malformed("mpf-ifd"))?;
    let count = usize::from(r.u16(ifd).ok_or(Error::Malformed("mpf-ifd"))?);
    let tags = ifd + 2;
    let table_end = count
        .checked_mul(12)
        .and_then(|n| n.checked_add(tags))
        .filter(|&e| e <= mpf.len())
        .ok_or(Error::Malformed("mpf-ifd"))?;
    let mut at = tags;
    while at < table_end {
        if r.u16(at) == Some(0xB002) {
            let bytes = r
                .u32(at + 4)
                .and_then(|v| usize::try_from(v).ok())
                .ok_or(Error::Malformed("mpf-entries"))?;
            let offset = r
                .u32(at + 8)
                .and_then(|v| usize::try_from(v).ok())
                .ok_or(Error::Malformed("mpf-entries"))?;
            let n = bytes / 16;
            if n == 0 || !bytes.is_multiple_of(16) {
                return Err(Error::Malformed("mpf-entries"));
            }
            if n > max_images {
                return Err(Error::LimitExceeded(Limit::Images));
            }
            offset
                .checked_add(bytes)
                .filter(|&e| e <= mpf.len())
                .ok_or(Error::Malformed("mpf-entries"))?;
            let mut entries = budget.vec::<MpEntry>(n)?;
            for i in 0..n {
                let e = offset + i * 16;
                match (r.u32(e + 4), r.u32(e + 8)) {
                    (Some(_), Some(offset)) => {
                        entries.push(MpEntry { offset });
                    }
                    _ => return Err(Error::Malformed("mpf-entries")),
                }
            }
            return Ok(entries);
        }
        at += 12;
    }
    Err(Error::Malformed("mpf-entries"))
}
