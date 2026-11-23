//! HEIF and AVIF: the base image and the gain map from the item graph of `isobmff`. The `tmap`
//! payload is read by [`IsoMetadata::parse`], the same parser as for JPEG.

use isobmff::{CodecConfig, GainMapSource, Heif, ImageKind};

use crate::budget::Budget;
use crate::types::resolve_metadata;
use crate::xmp;
use crate::{
    Codec, CodedImage, Container, Error, GainMap, GainMapImage, IsoMetadata, Kind, Limit, Limits,
    Location,
};

/// The `isobmff` limits for a nested call: what is left of `max_alloc`, and our pixel cap.
fn nested_limits(limits: &Limits, budget: &Budget<'_>) -> isobmff::Limits {
    isobmff::Limits {
        max_alloc: budget.remaining(),
        max_pixels: limits.max_pixels,
        ..isobmff::Limits::default()
    }
}

/// A rough size of the parsed graph that the result keeps.
fn graph_cost(h: &Heif) -> usize {
    let extents = h.items.iter().fold(0usize, |n, it| {
        n.saturating_add(it.data.as_ref().map_or(0, |v| v.len()))
    });
    [
        h.items.len().saturating_mul(size_of::<isobmff::Item>()),
        extents.saturating_mul(size_of::<isobmff::Extent>()),
        h.properties
            .len()
            .saturating_mul(size_of::<isobmff::Property>()),
        h.references
            .len()
            .saturating_mul(size_of::<isobmff::Reference>()),
        h.groups
            .len()
            .saturating_mul(size_of::<isobmff::EntityGroup>()),
        h.other_boxes
            .len()
            .saturating_mul(size_of::<(isobmff::FourCc, isobmff::Extent)>()),
    ]
    .iter()
    .fold(0usize, |sum, &n| sum.saturating_add(n))
}

/// Errors that end the read instead of becoming an issue.
fn fatal(e: &Error) -> bool {
    matches!(
        e,
        Error::LimitExceeded(_) | Error::OutOfMemory | Error::Cancelled
    )
}

fn coded_of(config: &CodecConfig) -> (Codec, u8) {
    match config {
        CodecConfig::Hevc(c) => (Codec::Hevc, if c.chroma_format_idc == 0 { 1 } else { 3 }),
        CodecConfig::Av1(c) => (Codec::Av1, if c.monochrome { 1 } else { 3 }),
    }
}

/// The coded image of an image item; a grid reports the codec of its first tile.
fn coded(heif: &Heif, img: &isobmff::Image) -> Result<CodedImage, Error> {
    let (codec, components, width, height) = match &img.kind {
        ImageKind::Coded { config, .. } => {
            let (codec, components) = coded_of(config);
            (codec, components, img.width, img.height)
        }
        ImageKind::Grid(g) => {
            let tile = g.tiles.first().ok_or(Error::Malformed("heif-grid"))?;
            let tile = heif.image(*tile)?;
            let ImageKind::Coded { config, .. } = &tile.kind else {
                return Err(Error::Unsupported("heif-grid-tile"));
            };
            let (codec, components) = coded_of(config);
            if img.width == 0 && img.height == 0 {
                (codec, components, g.width, g.height)
            } else {
                (codec, components, img.width, img.height)
            }
        }
        _ => return Err(Error::Unsupported("heif-image-kind")),
    };
    Ok(CodedImage {
        codec,
        width,
        height,
        components,
        location: Location::Item(img.id),
    })
}

/// `data` has an `ftyp` box.
pub(crate) fn read(
    data: &[u8],
    limits: &Limits,
    budget: &Budget<'_>,
) -> Result<GainMapImage, Error> {
    let heif = isobmff::parse(data, &nested_limits(limits, budget), budget.cancel())?;
    budget.charge(graph_cost(&heif))?;

    // A file with a gain map may name the `tmap` item as primary; the base is its first input.
    let mut base_id = heif.primary;
    let primary = heif.item(base_id).ok_or(Error::Malformed("pitm"))?;
    if primary.item_type.0 == *b"tmap" {
        base_id = heif
            .references
            .iter()
            .find(|r| r.kind.0 == *b"dimg" && r.from == base_id)
            .and_then(|r| r.to.first().copied())
            .ok_or(Error::Malformed("tmap-dimg"))?;
    }
    let img = heif.image(base_id)?;
    let base = coded(&heif, &img)?;
    let gain_map = match &img.gain_map {
        Some(gm) => Some(read_gain_map(data, &heif, gm, base.codec, limits, budget)?),
        None => None,
    };
    let container = if base.codec == Codec::Av1
        && (heif.ftyp.has_brand(b"avif") || heif.ftyp.has_brand(b"avis"))
    {
        Container::Avif
    } else {
        Container::Heif
    };
    Ok(GainMapImage {
        container,
        base,
        gain_map,
        heif: Some(heif),
    })
}

fn read_gain_map(
    data: &[u8],
    heif: &Heif,
    gm: &isobmff::GainMap,
    base_codec: Codec,
    limits: &Limits,
    budget: &Budget<'_>,
) -> Result<GainMap, Error> {
    let mut issues: Vec<&'static str> = Vec::new();
    let image = match heif
        .image(gm.image)
        .map_err(Error::from)
        .and_then(|i| coded(heif, &i))
    {
        Ok(c) => c,
        Err(e) if fatal(&e) => return Err(e),
        Err(_) => {
            issues.push("gainmap-image-unreadable");
            CodedImage {
                codec: base_codec,
                width: 0,
                height: 0,
                components: 1,
                location: Location::Item(gm.image),
            }
        }
    };

    let mut iso = None;
    if gm.source == GainMapSource::Tmap
        && let Some(id) = gm.tmap
    {
        let il = nested_limits(limits, budget);
        match heif
            .item_bytes(data, id, &il, budget.cancel())
            .map_err(Error::from)
        {
            Ok(bytes) => match bytes.split_first() {
                Some((0, rest)) => match IsoMetadata::parse(rest) {
                    Ok(parsed) => iso = Some(parsed),
                    Err(_) => issues.push("iso-truncated"),
                },
                Some(_) => issues.push("tmap-version"),
                None => issues.push("iso-truncated"),
            },
            Err(e) if fatal(&e) => return Err(e),
            Err(_) => issues.push("tmap-unreadable"),
        }
    }

    let mut parsed = None;
    if let Some(&id) = gm.xmp.first() {
        let total = heif
            .item(id)
            .and_then(|it| it.data.as_ref().ok())
            .map_or(0u64, |v| {
                v.iter().fold(0u64, |n, e| n.saturating_add(e.length))
            });
        if total > u64::try_from(limits.max_xmp_bytes).unwrap_or(u64::MAX) {
            return Err(Error::LimitExceeded(Limit::XmpBytes));
        }
        let il = nested_limits(limits, budget);
        match heif
            .item_bytes(data, id, &il, budget.cancel())
            .map_err(Error::from)
        {
            Ok(bytes) => parsed = xmp::parse(&bytes, limits, budget, &mut issues)?,
            Err(e) if fatal(&e) => return Err(e),
            Err(_) => issues.push("xmp-unreadable"),
        }
    }
    let (hdrgm, apple) = match &parsed {
        Some(p) => (
            xmp::hdrgm_from_xmp(&p.xmp, budget)?,
            xmp::apple_from_xmp(&p.xmp, budget)?,
        ),
        None => (None, None),
    };
    xmp::release(budget, parsed);

    let metadata = resolve_metadata(iso.as_ref(), hdrgm.as_ref(), &mut issues);
    let kind = match gm.source {
        GainMapSource::Tmap => Kind::Iso,
        GainMapSource::Apple => Kind::Apple,
    };
    Ok(GainMap {
        kind,
        image,
        metadata,
        iso,
        xmp: hdrgm,
        primary_version: None,
        apple,
        issues,
    })
}
