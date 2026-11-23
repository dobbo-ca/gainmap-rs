//! HEIF and AVIF: the base image and the gain map from the item graph of `isobmff`. The `tmap`
//! payload is read by [`IsoMetadata::parse`], the same parser as for JPEG.

use std::borrow::Cow;

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

/// Slack for the parts of `Heif` that have no public field: the struct itself and the lengths of
/// its private index vectors, which are counted below from the public fields.
const GRAPH_MARGIN: usize = 1024;

/// A size of the parsed graph that the result keeps, from the public fields: every Vec and
/// String of the items, properties, references and groups, plus `isobmff`'s private lookup
/// vectors (`index` 8 bytes per item, `fwd` 8 per reference, `rev` 8 per `to` id and `altr` 12
/// per group entity), which are bounded by the same counts.
fn graph_cost(h: &Heif) -> usize {
    let mut sum = GRAPH_MARGIN;
    let mut add = |n: usize| sum = sum.saturating_add(n);
    let to_total = h
        .references
        .iter()
        .fold(0usize, |n, r| n.saturating_add(r.to.len()));
    let entities = h
        .groups
        .iter()
        .fold(0usize, |n, g| n.saturating_add(g.entities.len()));
    add(h.items.len().saturating_mul(size_of::<isobmff::Item>() + 8));
    for it in &h.items {
        add(it.name.len());
        add(it.content_type.len());
        add(it.content_encoding.len());
        add(it.uri_type.len());
        add(it
            .associations
            .len()
            .saturating_mul(size_of::<isobmff::Association>()));
        if let Ok(v) = &it.data {
            add(v.len().saturating_mul(size_of::<isobmff::Extent>()));
        }
    }
    add(h
        .properties
        .len()
        .saturating_mul(size_of::<isobmff::Property>()));
    for p in &h.properties {
        match &p.value {
            isobmff::PropValue::Pixi { bits } => add(bits.len()),
            isobmff::PropValue::AuxC { aux_type, .. } => add(aux_type.len()),
            isobmff::PropValue::Rref { types } => {
                add(types.len().saturating_mul(size_of::<isobmff::FourCc>()))
            }
            isobmff::PropValue::Hvcc(c) => {
                add(size_of::<isobmff::Hvcc>());
                for a in &c.arrays {
                    add(size_of::<isobmff::NalArray>());
                    add(a.nals.len().saturating_mul(size_of::<isobmff::Extent>()));
                }
            }
            _ => {}
        }
    }
    add(h
        .references
        .len()
        .saturating_mul(size_of::<isobmff::Reference>() + 8));
    add(to_total.saturating_mul(4 + 8));
    add(h
        .groups
        .len()
        .saturating_mul(size_of::<isobmff::EntityGroup>()));
    add(entities.saturating_mul(4 + 12));
    add(h
        .ftyp
        .compatible
        .len()
        .saturating_mul(size_of::<isobmff::FourCc>()));
    add(h
        .other_boxes
        .len()
        .saturating_mul(size_of::<(isobmff::FourCc, isobmff::Extent)>()));
    sum
}

/// A bound for the Vecs that two live [`Heif::image`] results hold (the base image and one
/// transient image): aux ids, thumbnails and grid tiles are each at most the `to` ids in the file.
fn image_cost(h: &Heif) -> usize {
    let to_total = h
        .references
        .iter()
        .fold(0usize, |n, r| n.saturating_add(r.to.len()));
    to_total.saturating_mul(2 * (size_of::<isobmff::Aux>() + 3 * 4))
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
    let images = image_cost(&heif);
    budget.charge(images)?;

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
    // The `tmap` link is found here, not taken from `img.gain_map`, so that a gain map whose
    // `tmap` is out of `altr` order or unreadable is kept. The Apple auxiliary image is isobmff's.
    let gain_map = match find_tmap(&heif, base_id, budget)? {
        Some((gm, preferred)) => {
            let mut g = read_gain_map(data, &heif, &gm, &img.xmp, base.codec, limits, budget)?;
            if !preferred {
                g.issues.push("tmap-not-preferred");
            }
            Some(g)
        }
        None => match &img.gain_map {
            Some(gm) => Some(read_gain_map(
                data, &heif, gm, &img.xmp, base.codec, limits, budget,
            )?),
            None => None,
        },
    };
    budget.release(images);
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

/// The `tmap` item that links `base_id` to a gain map (`dimg` inputs [base, gain map]). Gives the
/// first one that precedes the base in an `altr` group, else the first one in reference order;
/// the flag says whether it was the preferred kind. The lookup tables are charged while they live.
fn find_tmap(
    heif: &Heif,
    base_id: u32,
    budget: &Budget<'_>,
) -> Result<Option<(isobmff::GainMap, bool)>, Error> {
    let is_link = |r: &&isobmff::Reference| {
        r.kind.0 == *b"dimg"
            && matches!(r.to.as_slice(), [b, _] if *b == base_id)
            && heif
                .item(r.from)
                .is_some_and(|it| it.item_type.0 == *b"tmap")
    };
    let n = heif.references.iter().filter(is_link).count();
    if n == 0 {
        return Ok(None);
    }
    // Per candidate: (tmap, gain map) 8, (tmap, position) 16 and a state byte.
    let cost = n.saturating_mul(25);
    budget.charge(cost)?;
    let found = pick_tmap(heif, base_id, n, is_link);
    budget.release(cost);
    found
}

fn pick_tmap(
    heif: &Heif,
    base_id: u32,
    n: usize,
    is_link: impl Fn(&&isobmff::Reference) -> bool,
) -> Result<Option<(isobmff::GainMap, bool)>, Error> {
    let mut cands: Vec<(u32, u32)> = Vec::new();
    let mut by_id: Vec<(u32, usize)> = Vec::new();
    // 0: repeat of an earlier tmap, 1: a candidate, 2: a candidate before the base in `altr`.
    let mut state: Vec<u8> = Vec::new();
    cands.try_reserve_exact(n).map_err(|_| Error::OutOfMemory)?;
    by_id.try_reserve_exact(n).map_err(|_| Error::OutOfMemory)?;
    state.try_reserve_exact(n).map_err(|_| Error::OutOfMemory)?;
    for r in heif.references.iter().filter(is_link) {
        if let [_, gm] = r.to.as_slice() {
            by_id.push((r.from, cands.len()));
            cands.push((r.from, *gm));
            state.push(1);
        }
    }
    by_id.sort_unstable();
    by_id.dedup_by(|b, a| {
        let repeat = a.0 == b.0;
        if repeat && let Some(s) = state.get_mut(b.1) {
            *s = 0;
        }
        repeat
    });
    for g in heif.groups.iter().filter(|g| g.kind.0 == *b"altr") {
        let Some(at) = g.entities.iter().position(|&e| e == base_id) else {
            continue;
        };
        for e in g.entities.iter().take(at) {
            if let Ok(k) = by_id.binary_search_by_key(e, |x| x.0)
                && let Some(s) = by_id.get(k).and_then(|x| state.get_mut(x.1))
            {
                *s = 2;
            }
        }
    }
    let best = match state.iter().position(|&s| s == 2) {
        Some(i) => Some((i, true)),
        None => state.iter().position(|&s| s == 1).map(|i| (i, false)),
    };
    let Some((i, preferred)) = best else {
        return Ok(None);
    };
    let Some(&(tmap, image)) = cands.get(i) else {
        return Ok(None);
    };
    Ok(Some((
        isobmff::GainMap {
            image,
            tmap: Some(tmap),
            metadata: None,
            source: GainMapSource::Tmap,
            xmp: Vec::new(),
        },
        preferred,
    )))
}

/// The bytes of a copy that `item_bytes` made (0 for bytes borrowed from the file).
fn owned_len(bytes: &Cow<'_, [u8]>) -> usize {
    match bytes {
        Cow::Owned(v) => v.len(),
        Cow::Borrowed(_) => 0,
    }
}

/// The most bytes of a `tmap` payload that are read: a valid one is 1 + 141 bytes, plus a few
/// trailing bytes that writers add.
const TMAP_MAX: u64 = 4096;

/// The summed extent lengths of item `id`, 0 when it has none or cannot be located.
fn item_len(heif: &Heif, id: u32) -> u64 {
    heif.item(id)
        .and_then(|it| it.data.as_ref().ok())
        .map_or(0u64, |v| {
            v.iter().fold(0u64, |n, e| n.saturating_add(e.length))
        })
}

/// Reads and parses the XMP item `id` under the limits. Unreadable or malformed XMP is an issue;
/// the limits, memory and cancel are errors. The caller releases the result.
fn xmp_item(
    data: &[u8],
    heif: &Heif,
    id: Option<&u32>,
    limits: &Limits,
    budget: &Budget<'_>,
    issues: &mut Vec<&'static str>,
) -> Result<Option<xmp::Parsed>, Error> {
    let Some(&id) = id else { return Ok(None) };
    if item_len(heif, id) > u64::try_from(limits.max_xmp_bytes).unwrap_or(u64::MAX) {
        return Err(Error::LimitExceeded(Limit::XmpBytes));
    }
    let il = nested_limits(limits, budget);
    match heif
        .item_bytes(data, id, &il, budget.cancel())
        .map_err(Error::from)
    {
        Ok(bytes) => {
            // An item in several extents is copied; the copy lives while the parse runs.
            let held = owned_len(&bytes);
            budget.charge(held)?;
            let parsed = xmp::parse(&bytes, limits, budget, issues);
            budget.release(held);
            parsed
        }
        Err(e) if fatal(&e) => Err(e),
        Err(_) => {
            issues.push("xmp-unreadable");
            Ok(None)
        }
    }
}

fn read_gain_map(
    data: &[u8],
    heif: &Heif,
    gm: &isobmff::GainMap,
    base_xmp: &[u32],
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
        if item_len(heif, id) > TMAP_MAX {
            issues.push("tmap-too-large");
        } else {
            match heif
                .item_bytes(data, id, &il, budget.cancel())
                .map_err(Error::from)
            {
                Ok(bytes) => {
                    let held = owned_len(&bytes);
                    budget.charge(held)?;
                    match bytes.split_first() {
                        Some((0, rest)) => match IsoMetadata::parse(rest, limits) {
                            Ok(parsed) => iso = Some(parsed),
                            Err(_) => issues.push("iso-truncated"),
                        },
                        Some(_) => issues.push("tmap-version"),
                        None => issues.push("iso-truncated"),
                    }
                    budget.release(held);
                }
                Err(e) if fatal(&e) => return Err(e),
                Err(_) => issues.push("tmap-unreadable"),
            }
        }
    }

    let parsed = xmp_item(data, heif, gm.xmp.first(), limits, budget, &mut issues)?;
    let (hdrgm, apple) = match &parsed {
        Some(p) => (
            xmp::hdrgm_from_xmp(&p.xmp, budget)?,
            xmp::apple_from_xmp(&p.xmp, budget)?,
        ),
        None => (None, None),
    };
    xmp::release(budget, parsed);

    // hdrgm:Version of the primary (base) image's XMP, as for JPEG.
    let pxmp = xmp_item(data, heif, base_xmp.first(), limits, budget, &mut issues)?;
    let primary_version = match &pxmp {
        Some(p) => xmp::hdrgm_version(&p.xmp)
            .map(|s| budget.string(s))
            .transpose()?,
        None => None,
    };
    xmp::release(budget, pxmp);

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
        primary_version,
        apple,
        issues,
    })
}
