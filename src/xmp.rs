//! XMP values of a gain map: `hdrgm`, Apple `HDRGainMap` and the Google container directory.
//!
//! The XML is read by `exif::Xmp` (every value is text); this module picks the properties and
//! converts them.

use crate::budget::Budget;
use crate::{AppleGainMap, Error, HdrgmXmp, Limit, Limits, Metadata};

const NS_HDRGM: &str = "http://ns.adobe.com/hdr-gain-map/1.0/";
const NS_APPLE: &str = "http://ns.apple.com/HDRGainMap/1.0/";

/// A parsed packet and the bytes charged for it, to give back when it is dropped.
pub(crate) struct Parsed {
    pub(crate) xmp: exif::Xmp,
    pub(crate) cost: usize,
}

/// Parses an XMP packet under the remaining budget. A packet that is not valid XMP gives `None`
/// and the issue `xmp-malformed` (a bad packet never fails a read); the limits, an allocation
/// failure and the cancel flag are errors.
pub(crate) fn parse(
    packet: &[u8],
    limits: &Limits,
    budget: &Budget<'_>,
    issues: &mut Vec<&'static str>,
) -> Result<Option<Parsed>, Error> {
    let l = exif::Limits {
        max_alloc: budget.remaining(),
        max_xmp_bytes: limits.max_xmp_bytes,
        ..exif::Limits::default()
    };
    match exif::Xmp::parse(packet, None, &l, budget.cancel()) {
        Ok(xmp) => {
            // Counts the capacity exif holds (a value keeps the room of its raw span even when
            // entities shrink it), not the length. exif also reserves a slot for a property it
            // then leaves out as not UTF-8, and its Vec is not visible from here. So the slots
            // are charged at the length rounded up to the next power of two: an upper bound for
            // a Vec that grew by doubling, and a guess when exif reserved the exact count.
            let slots = xmp.properties().len().next_power_of_two();
            let props = xmp.properties().iter().fold(
                slots.saturating_mul(size_of::<exif::Property>()),
                |sum, p| {
                    sum.saturating_add(p.ns.capacity())
                        .saturating_add(p.name.capacity())
                        .saturating_add(p.value.capacity())
                },
            );
            let cost = xmp.raw().len().saturating_add(props);
            budget.charge(cost)?;
            Ok(Some(Parsed { xmp, cost }))
        }
        Err(exif::Error::LimitExceeded(exif::Limit::Alloc)) => {
            Err(Error::LimitExceeded(Limit::Alloc))
        }
        // The node count of a packet is bounded by its size, so exif's node limit is reported as
        // the packet-size limit.
        Err(exif::Error::LimitExceeded(exif::Limit::XmpBytes | exif::Limit::XmpNodes)) => {
            Err(Error::LimitExceeded(Limit::XmpBytes))
        }
        Err(exif::Error::OutOfMemory) => Err(Error::OutOfMemory),
        Err(exif::Error::Cancelled) => Err(Error::Cancelled),
        Err(_) => {
            issues.push("xmp-malformed");
            Ok(None)
        }
    }
}

/// Gives back what [`parse`] charged.
pub(crate) fn release(budget: &Budget<'_>, parsed: Option<Parsed>) {
    if let Some(p) = parsed {
        budget.release(p.cost);
    }
}

/// The packet describes a gain map image: any `hdrgm` property other than `Version` (a packet
/// without `GainMapMax` still counts, so the read keeps the image and reports
/// `xmp-missing-gainmapmax`) or any Apple `HDRGainMap` property. `hdrgm:Version` alone marks a
/// primary image.
pub(crate) fn is_gain_map(xmp: &exif::Xmp) -> bool {
    xmp.hdrgm().any(|p| p.name != "Version") || has_apple(xmp)
}

/// The text of `hdrgm:Version`.
pub(crate) fn hdrgm_version(xmp: &exif::Xmp) -> Option<&str> {
    xmp.get(NS_HDRGM, "Version")
}

fn has_apple(xmp: &exif::Xmp) -> bool {
    xmp.properties()
        .iter()
        .any(|p| p.top_level && p.ns == NS_APPLE)
}

fn all(xmp: &exif::Xmp, name: &str, budget: &Budget<'_>) -> Result<Vec<String>, Error> {
    let n = xmp.hdrgm().filter(|p| p.name == name).count();
    let mut v = budget.vec::<String>(n)?;
    for p in xmp.hdrgm().filter(|p| p.name == name) {
        v.push(budget.string(&p.value)?);
    }
    Ok(v)
}

fn first(xmp: &exif::Xmp, name: &str, budget: &Budget<'_>) -> Result<Option<String>, Error> {
    xmp.hdrgm()
        .find(|p| p.name == name)
        .map(|p| budget.string(&p.value))
        .transpose()
}

/// The `hdrgm` properties of a packet as raw text; `None` when it has none.
pub(crate) fn hdrgm_from_xmp(
    xmp: &exif::Xmp,
    budget: &Budget<'_>,
) -> Result<Option<HdrgmXmp>, Error> {
    if xmp.hdrgm().next().is_none() {
        return Ok(None);
    }
    Ok(Some(HdrgmXmp {
        version: first(xmp, "Version", budget)?,
        gain_map_min: all(xmp, "GainMapMin", budget)?,
        gain_map_max: all(xmp, "GainMapMax", budget)?,
        gamma: all(xmp, "Gamma", budget)?,
        offset_sdr: all(xmp, "OffsetSDR", budget)?,
        offset_hdr: all(xmp, "OffsetHDR", budget)?,
        hdr_capacity_min: first(xmp, "HDRCapacityMin", budget)?,
        hdr_capacity_max: first(xmp, "HDRCapacityMax", budget)?,
        base_rendition_is_hdr: first(xmp, "BaseRenditionIsHDR", budget)?,
    }))
}

/// The Apple `HDRGainMap` properties of a packet; `None` when it has none.
pub(crate) fn apple_from_xmp(
    xmp: &exif::Xmp,
    budget: &Budget<'_>,
) -> Result<Option<AppleGainMap>, Error> {
    if !has_apple(xmp) {
        return Ok(None);
    }
    let get = |name: &str| {
        xmp.get(NS_APPLE, name)
            .map(|s| budget.string(s))
            .transpose()
    };
    Ok(Some(AppleGainMap {
        version: get("HDRGainMapVersion")?,
        headroom: get("HDRGainMapHeadroom")?,
    }))
}

/// A number as `str::parse::<f32>` reads it after trimming ASCII whitespace; NaN and infinity
/// are refused.
fn num(s: &str) -> Result<f32, Error> {
    match s.trim_ascii().parse::<f32>() {
        Ok(v) if v.is_finite() => Ok(v),
        _ => Err(Error::Malformed("xmp-bad-number")),
    }
}

/// One value or three (R, G, B); `true` when there were three.
fn channels(values: &[String], default: f32) -> Result<([f32; 3], bool), Error> {
    match values {
        [] => Ok(([default; 3], false)),
        [v] => Ok(([num(v)?; 3], false)),
        [r, g, b] => Ok(([num(r)?, num(g)?, num(b)?], true)),
        _ => Err(Error::Malformed("xmp-bad-count")),
    }
}

impl Metadata {
    /// Converts `hdrgm` XMP text to metadata, with the defaults of the Ultra HDR format:
    /// `GainMapMin` 0, `Gamma` 1, `OffsetSDR` and `OffsetHDR` 1/64, `HDRCapacityMin` 0,
    /// `BaseRenditionIsHDR` False. `GainMapMax` and `HDRCapacityMax` are required. A sequence of
    /// one value applies to all channels; three values are R, G, B. XMP has no colour-space
    /// field, so `use_base_colour_space` is true, as in libultrahdr.
    ///
    /// `OffsetSDR` and `HDRCapacityMin` belong to the SDR rendition, `OffsetHDR` and
    /// `HDRCapacityMax` to the HDR one. Normally the base is the SDR rendition, so they become
    /// `base_offset`, `base_hdr_headroom`, `alternate_offset` and `alternate_hdr_headroom`. When
    /// `BaseRenditionIsHDR` is True the base is the HDR rendition: `backward_direction` is set
    /// and the base fields take the HDR values (`OffsetHDR`, `HDRCapacityMax`) while the
    /// alternate fields take the SDR ones.
    ///
    /// # Errors
    ///
    /// [`Error::Malformed`] with the token `xmp-missing-gainmapmax`,
    /// `xmp-missing-hdrcapacitymax`, `xmp-bad-count` (a sequence of 2 or more than 3 values) or
    /// `xmp-bad-number`.
    pub fn from_xmp(xmp: &HdrgmXmp, _limits: &Limits) -> Result<Metadata, Error> {
        if xmp.gain_map_max.is_empty() {
            return Err(Error::Malformed("xmp-missing-gainmapmax"));
        }
        let capacity_max = xmp
            .hdr_capacity_max
            .as_deref()
            .ok_or(Error::Malformed("xmp-missing-hdrcapacitymax"))?;
        let (gain_map_max, a) = channels(&xmp.gain_map_max, 0.0)?;
        let (gain_map_min, b) = channels(&xmp.gain_map_min, 0.0)?;
        let (gamma, c) = channels(&xmp.gamma, 1.0)?;
        let (base_offset, d) = channels(&xmp.offset_sdr, 1.0 / 64.0)?;
        let (alternate_offset, e) = channels(&xmp.offset_hdr, 1.0 / 64.0)?;
        let capacity_min = match xmp.hdr_capacity_min.as_deref() {
            Some(s) => num(s)?,
            None => 0.0,
        };
        let capacity_max = num(capacity_max)?;
        let backward_direction = xmp
            .base_rendition_is_hdr
            .as_deref()
            .is_some_and(|s| s.trim_ascii().eq_ignore_ascii_case("true"));
        // `OffsetSDR` and `HDRCapacityMin` describe the SDR rendition, `OffsetHDR` and
        // `HDRCapacityMax` the HDR one. When the base is the HDR rendition the two swap roles.
        let (base_offset, alternate_offset, base_hdr_headroom, alternate_hdr_headroom) =
            if backward_direction {
                (alternate_offset, base_offset, capacity_max, capacity_min)
            } else {
                (base_offset, alternate_offset, capacity_min, capacity_max)
            };
        Ok(Metadata {
            gain_map_min,
            gain_map_max,
            gamma,
            base_offset,
            alternate_offset,
            base_hdr_headroom,
            alternate_hdr_headroom,
            use_base_colour_space: true,
            multichannel: a || b || c || d || e,
            backward_direction,
        })
    }
}

/// One item of the Google container directory (`Container:Item`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Item {
    /// `Item:Semantic` is `Primary`.
    pub(crate) primary: bool,
    /// `Item:Semantic` is `GainMap`.
    pub(crate) gain_map: bool,
    /// `Item:Length`.
    pub(crate) length: Option<u64>,
    /// `Item:Padding` (0 when absent).
    pub(crate) padding: u64,
}

/// The `Container:Item` start tags of a packet that have an `Item:Semantic` attribute, in order.
///
/// `exif::Xmp` gives the attributes of `rdf:Description` only, and the items carry their values
/// as attributes of the `Item` element, so this reads the packet text directly. The prefix of
/// the attributes is not checked, only their local names.
struct ItemTags<'a> {
    data: &'a [u8],
    pos: usize,
}

/// The index of the first `>` that is not inside a quoted value.
fn tag_end(rest: &[u8]) -> Option<usize> {
    let mut quote = 0u8;
    for (i, &b) in rest.iter().enumerate() {
        if quote != 0 {
            if b == quote {
                quote = 0;
            }
        } else if b == b'"' || b == b'\'' {
            quote = b;
        } else if b == b'>' {
            return Some(i);
        }
    }
    None
}

/// The part of a qualified name after the last `:`.
fn local(name: &[u8]) -> &[u8] {
    match name.iter().rposition(|&b| b == b':') {
        Some(i) => name.get(i + 1..).unwrap_or_default(),
        None => name,
    }
}

/// Reads `name="value"` pairs of a start tag; `tag` starts after the element name.
fn item_of(tag: &[u8]) -> Option<Item> {
    let mut item = Item {
        primary: false,
        gain_map: false,
        length: None,
        padding: 0,
    };
    let mut semantic = false;
    let mut i = 0;
    loop {
        while tag.get(i).is_some_and(u8::is_ascii_whitespace) {
            i += 1;
        }
        let start = i;
        while tag
            .get(i)
            .is_some_and(|&b| b != b'=' && b != b'/' && !b.is_ascii_whitespace())
        {
            i += 1;
        }
        let name = tag.get(start..i)?;
        if name.is_empty() {
            break;
        }
        while tag.get(i).is_some_and(u8::is_ascii_whitespace) {
            i += 1;
        }
        if tag.get(i) != Some(&b'=') {
            break;
        }
        i += 1;
        while tag.get(i).is_some_and(u8::is_ascii_whitespace) {
            i += 1;
        }
        let quote = *tag.get(i)?;
        if quote != b'"' && quote != b'\'' {
            break;
        }
        i += 1;
        let len = tag.get(i..)?.iter().position(|&b| b == quote)?;
        let value = tag.get(i..i + len)?;
        i += len + 1;
        let number = || {
            std::str::from_utf8(value)
                .ok()?
                .trim_ascii()
                .parse::<u64>()
                .ok()
        };
        match local(name) {
            b"Semantic" => {
                semantic = true;
                item.primary = value == b"Primary";
                item.gain_map = value == b"GainMap";
            }
            b"Length" => item.length = number(),
            b"Padding" => item.padding = number().unwrap_or(0),
            _ => {}
        }
    }
    semantic.then_some(item)
}

impl Iterator for ItemTags<'_> {
    type Item = Item;

    fn next(&mut self) -> Option<Item> {
        loop {
            let from = self.data.get(self.pos..)?;
            let lt = from.iter().position(|&b| b == b'<')?;
            let rest = from.get(lt + 1..)?;
            if rest.starts_with(b"!--") {
                let end = rest.windows(3).position(|w| w == b"-->")?;
                self.pos += lt + 1 + end + 3;
                continue;
            }
            let end = tag_end(rest)?;
            self.pos += lt + 1 + end + 1;
            let tag = rest.get(..end)?;
            if matches!(tag.first(), Some(b'/' | b'?' | b'!')) {
                continue;
            }
            let name_len = tag
                .iter()
                .position(|&b| b == b'/' || b.is_ascii_whitespace())
                .unwrap_or(tag.len());
            if local(tag.get(..name_len).unwrap_or_default()) != b"Item" {
                continue;
            }
            if let Some(item) = item_of(tag.get(name_len..).unwrap_or_default()) {
                return Some(item);
            }
        }
    }
}

/// The container directory items of a primary-image XMP packet, at most `max` of them.
pub(crate) fn container_items(
    packet: &[u8],
    max: usize,
    budget: &Budget<'_>,
) -> Result<Vec<Item>, Error> {
    let n = ItemTags {
        data: packet,
        pos: 0,
    }
    .take(max)
    .count();
    let mut items = budget.vec::<Item>(n)?;
    items.extend(
        ItemTags {
            data: packet,
            pos: 0,
        }
        .take(n),
    );
    Ok(items)
}
