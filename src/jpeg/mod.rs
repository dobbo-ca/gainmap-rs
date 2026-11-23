//! Reads an Ultra HDR, ISO 21496-1 or Apple gain-map JPEG: finds the gain-map image and its
//! metadata. Locators run in this order: the MPF entries, the Google container directory of the
//! primary XMP, and the JPEG that follows the primary image.

use crate::budget::Budget;
use crate::types::resolve_metadata;
use crate::xmp::{self, Parsed};
use crate::{
    Codec, CodedImage, Container, Error, GainMap, GainMapImage, IsoMetadata, Kind, Limits, Location,
};

pub(crate) mod mpf;
pub(crate) mod scan;

/// Where a gain-map image may start.
#[derive(Debug, Clone, Copy)]
struct Cand {
    start: usize,
    /// The input is cut here for the walk of this candidate.
    end: usize,
    /// The container directory names it a gain map.
    semantic: bool,
}

/// A candidate that was walked.
struct Found {
    start: usize,
    scan: scan::Scan,
    sof: scan::Sof,
    xmp: Option<Parsed>,
    issues: Vec<&'static str>,
}

/// `data` is a JPEG (it starts with SOI).
pub(crate) fn read(
    data: &[u8],
    limits: &Limits,
    budget: &Budget<'_>,
) -> Result<GainMapImage, Error> {
    let primary = scan::scan(data, 0, limits, budget)?;
    let sof = primary.sof.ok_or(Error::Malformed("jpeg-no-sof"))?;
    let base = CodedImage {
        codec: Codec::Jpeg,
        width: sof.width,
        height: sof.height,
        components: sof.components,
        location: Location::Range(0..primary.end),
    };
    let mut issues: Vec<&'static str> = Vec::new();

    let pxmp = match primary.xmp.clone().and_then(|r| data.get(r)) {
        Some(packet) => xmp::parse(packet, limits, budget, &mut issues)?,
        None => None,
    };
    let primary_version = match &pxmp {
        Some(p) => xmp::hdrgm_version(&p.xmp)
            .map(|s| budget.string(s))
            .transpose()?,
        None => None,
    };

    let mut entries = Vec::new();
    let mut mpf_base = 0;
    if let Some(r) = primary.mpf.clone() {
        match data
            .get(r.clone())
            .map(|p| mpf::parse(p, limits.max_images, budget))
        {
            Some(Ok(e)) => {
                entries = e;
                mpf_base = r.start;
            }
            Some(Err(Error::Malformed(_) | Error::Truncated)) | None => {
                issues.push("mpf-malformed");
            }
            Some(Err(e)) => return Err(e),
        }
    }
    let items = match &pxmp {
        Some(p) => xmp::container_items(p.xmp.raw(), limits.max_images, budget)?,
        None => Vec::new(),
    };

    let cap = entries
        .len()
        .saturating_add(items.len())
        .saturating_add(1)
        .min(limits.max_images);
    let mut cands = budget.vec::<Cand>(cap)?;
    let mut add = |start: Option<usize>, end: usize, semantic: bool| {
        if let Some(start) = start
            && cands.len() < cap
        {
            cands.push(Cand {
                start,
                end,
                semantic,
            });
        }
    };
    // (a) MPF entries after the first.
    for e in entries.iter().skip(1).filter(|e| e.offset != 0) {
        let start = usize::try_from(e.offset)
            .ok()
            .and_then(|o| mpf_base.checked_add(o));
        let end = start
            .zip(usize::try_from(e.size).ok().filter(|&s| s > 0))
            .and_then(|(s, size)| s.checked_add(size))
            .filter(|&end| end <= data.len())
            .unwrap_or(data.len());
        add(start, end, false);
    }
    // (b) The container directory: items after the primary, in order.
    if let Some((index, first)) = items.iter().enumerate().find(|(_, i)| i.primary) {
        let mut pos = usize::try_from(first.padding)
            .ok()
            .and_then(|pad| primary.end.checked_add(pad));
        for item in items.iter().skip(index + 1) {
            let Some(start) = pos else { break };
            add(Some(start), data.len(), item.gain_map);
            pos = item
                .length
                .and_then(|l| usize::try_from(l).ok())
                .and_then(|l| start.checked_add(l))
                .and_then(|end| {
                    usize::try_from(item.padding)
                        .ok()
                        .and_then(|pad| end.checked_add(pad))
                });
        }
    }
    // (c) The JPEG right after the primary image.
    if primary_version.is_some() || primary.iso.is_some() {
        add(Some(primary.end), data.len(), false);
    }

    let mut chosen: Option<Found> = None;
    let mut by_semantic: Option<Found> = None;
    for c in &cands {
        budget.check_cancel()?;
        if c.start < primary.end {
            continue;
        }
        let Some(slice) = data.get(..c.end) else {
            continue;
        };
        let walked = match scan::scan(slice, c.start, limits, budget) {
            Ok(s) => s,
            Err(Error::Malformed(_) | Error::Truncated) => continue,
            Err(e) => return Err(e),
        };
        let Some(cand_sof) = walked.sof else {
            continue;
        };
        let mut cand_issues = Vec::new();
        let parsed = match walked.xmp.clone().and_then(|r| data.get(r)) {
            Some(packet) => xmp::parse(packet, limits, budget, &mut cand_issues)?,
            None => None,
        };
        let is_gain_map = parsed.as_ref().is_some_and(|p| xmp::is_gain_map(&p.xmp))
            || walked.iso.as_ref().is_some_and(|r| r.len() > 4);
        let found = Found {
            start: c.start,
            scan: walked,
            sof: cand_sof,
            xmp: parsed,
            issues: cand_issues,
        };
        if is_gain_map {
            chosen = Some(found);
            break;
        }
        if c.semantic && by_semantic.is_none() {
            by_semantic = Some(found);
        } else {
            xmp::release(budget, found.xmp);
        }
    }
    let found = match (chosen, by_semantic) {
        (Some(found), other) => {
            xmp::release(budget, other.and_then(|o| o.xmp));
            found
        }
        (None, Some(found)) => found,
        (None, None) => {
            xmp::release(budget, pxmp);
            return Ok(GainMapImage {
                container: Container::Jpeg,
                base,
                gain_map: None,
                heif: None,
            });
        }
    };
    issues.extend(found.issues.iter().copied());

    let iso = match found
        .scan
        .iso
        .clone()
        .and_then(|r| data.get(r))
        .filter(|p| p.len() > 4)
    {
        Some(payload) => match IsoMetadata::parse(payload) {
            Ok(iso) => Some(iso),
            Err(_) => {
                issues.push("iso-truncated");
                None
            }
        },
        None => None,
    };
    let (hdrgm, apple) = match &found.xmp {
        Some(p) => (
            xmp::hdrgm_from_xmp(&p.xmp, budget)?,
            xmp::apple_from_xmp(&p.xmp, budget)?,
        ),
        None => (None, None),
    };
    xmp::release(budget, pxmp);
    xmp::release(budget, found.xmp);

    let metadata = resolve_metadata(iso.as_ref(), hdrgm.as_ref(), &mut issues);
    let kind = if primary_version.is_some() || hdrgm.is_some() {
        Kind::UltraHdr
    } else if found.scan.iso.is_some() {
        Kind::Iso
    } else {
        Kind::Apple
    };
    Ok(GainMapImage {
        container: Container::Jpeg,
        base,
        gain_map: Some(GainMap {
            kind,
            image: CodedImage {
                codec: Codec::Jpeg,
                width: found.sof.width,
                height: found.sof.height,
                components: found.sof.components,
                location: Location::Range(found.start..found.scan.end),
            },
            metadata,
            iso,
            xmp: hdrgm,
            primary_version,
            apple,
            issues,
        }),
        heif: None,
    })
}
