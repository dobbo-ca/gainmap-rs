//! Synthetic inputs for the integration tests: valid baseline JPEGs from the `jpeg` encoder with
//! XMP, ISO 21496-1 and MPF segments added, ISO payloads, and small HEIF and AVIF files. No
//! binary fixtures. Not every test file uses every item.
#![allow(dead_code)]

use std::sync::atomic::AtomicBool;

use gainmap::Limits;

pub const XMP_NS: &[u8] = b"http://ns.adobe.com/xap/1.0/\0";
pub const ISO_NS: &[u8] = b"urn:iso:std:iso:ts:21496:-1\0";
const RDF: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#";
pub const HDRGM_DECL: &str = "xmlns:hdrgm=\"http://ns.adobe.com/hdr-gain-map/1.0/\"";
pub const APPLE_DECL: &str = "xmlns:HDRGainMap=\"http://ns.apple.com/HDRGainMap/1.0/\"";
pub const CONTAINER_DECL: &str = "xmlns:Container=\"http://ns.google.com/photos/1.0/container/\" \
     xmlns:Item=\"http://ns.google.com/photos/1.0/container/item/\"";

pub fn limits() -> Limits {
    Limits {
        max_threads: 2,
        ..Limits::default()
    }
}

pub fn no_cancel() -> AtomicBool {
    AtomicBool::new(false)
}

// ---- JPEG ----

pub fn seg(marker: u8, body: &[u8]) -> Vec<u8> {
    let mut v = vec![0xFF, marker];
    v.extend_from_slice(&(body.len() as u16 + 2).to_be_bytes());
    v.extend_from_slice(body);
    v
}

pub fn app1_xmp(xml: &str) -> Vec<u8> {
    seg(0xE1, &[XMP_NS, xml.as_bytes()].concat())
}

pub fn app2_iso(payload: &[u8]) -> Vec<u8> {
    seg(0xE2, &[ISO_NS, payload].concat())
}

/// A valid baseline JPEG, 4:2:0, `w` x `h`, three components.
pub fn jpeg_rgb(w: usize, h: usize) -> Vec<u8> {
    let mut pix = Vec::with_capacity(w * h * 4);
    for y in 0..h {
        for x in 0..w {
            pix.extend_from_slice(&[
                (x * 255 / w) as u8,
                (y * 255 / h) as u8,
                ((x + y) * 7) as u8,
                255,
            ]);
        }
    }
    let img = jpeg::Rgba {
        pix,
        stride: w * 4,
        rect: jpeg::Rect::new(0, 0, w as i32, h as i32),
    };
    let o = jpeg::EncodeOptions {
        quality: 75,
        chroma444: false,
    };
    jpeg::encode(
        jpeg::EncodeImage::Rgba(&img),
        &o,
        &jpeg::Limits::default(),
        &no_cancel(),
    )
    .unwrap()
}

/// A valid baseline JPEG, `w` x `h`, one component.
pub fn jpeg_gray(w: usize, h: usize) -> Vec<u8> {
    let mut pix = Vec::with_capacity(w * h);
    for y in 0..h {
        for x in 0..w {
            pix.push(((x * 255 / w + y * 3) % 256) as u8);
        }
    }
    let img = jpeg::Gray {
        pix,
        stride: w,
        rect: jpeg::Rect::new(0, 0, w as i32, h as i32),
    };
    let o = jpeg::EncodeOptions {
        quality: 75,
        chroma444: false,
    };
    jpeg::encode(
        jpeg::EncodeImage::Gray(&img),
        &o,
        &jpeg::Limits::default(),
        &no_cancel(),
    )
    .unwrap()
}

/// `jpeg` with `segs` inserted right after the SOI marker.
pub fn with_segments(jpeg: &[u8], segs: &[Vec<u8>]) -> Vec<u8> {
    assert_eq!(&jpeg[..2], &[0xFF, 0xD8]);
    let mut v = jpeg[..2].to_vec();
    for s in segs {
        v.extend_from_slice(s);
    }
    v.extend_from_slice(&jpeg[2..]);
    v
}

// ---- MPF ----

/// An MPF payload from the endian field on. `entries` are (attribute, size, offset).
pub fn mpf_payload(be: bool, entries: &[(u32, u32, u32)]) -> Vec<u8> {
    let w16 = |v: u16| if be { v.to_be_bytes() } else { v.to_le_bytes() };
    let w32 = |v: u32| if be { v.to_be_bytes() } else { v.to_le_bytes() };
    let mut p = if be {
        b"MM\0*".to_vec()
    } else {
        b"II*\0".to_vec()
    };
    p.extend_from_slice(&w32(8));
    p.extend_from_slice(&w16(3));
    // version "0100"
    p.extend_from_slice(&w16(0xB000));
    p.extend_from_slice(&w16(7));
    p.extend_from_slice(&w32(4));
    p.extend_from_slice(b"0100");
    // number of images
    p.extend_from_slice(&w16(0xB001));
    p.extend_from_slice(&w16(4));
    p.extend_from_slice(&w32(1));
    p.extend_from_slice(&w32(entries.len() as u32));
    // entries, right after the IFD: 8 + 2 + 3 * 12 + 4
    p.extend_from_slice(&w16(0xB002));
    p.extend_from_slice(&w16(7));
    p.extend_from_slice(&w32(entries.len() as u32 * 16));
    p.extend_from_slice(&w32(50));
    p.extend_from_slice(&w32(0));
    for &(attr, size, offset) in entries {
        p.extend_from_slice(&w32(attr));
        p.extend_from_slice(&w32(size));
        p.extend_from_slice(&w32(offset));
        p.extend_from_slice(&w16(0));
        p.extend_from_slice(&w16(0));
    }
    p
}

pub fn mpf_seg(be: bool, entries: &[(u32, u32, u32)]) -> Vec<u8> {
    seg(0xE2, &[&b"MPF\0"[..], &mpf_payload(be, entries)].concat())
}

pub struct Built {
    pub file: Vec<u8>,
    pub primary_len: usize,
    pub gm_len: usize,
    /// The start of each image after the primary one.
    pub starts: Vec<usize>,
}

/// The primary image `base` with `primary_segs` and an MPF index, then `gm`.
pub fn with_mpf(primary_segs: &[Vec<u8>], base: &[u8], gm: &[u8], be: bool) -> Built {
    with_mpf_many(primary_segs, base, &[gm], be)
}

/// The primary image `base` with `primary_segs` and an MPF index of `1 + others.len()` entries,
/// then the `others` one after the other.
pub fn with_mpf_many(primary_segs: &[Vec<u8>], base: &[u8], others: &[&[u8]], be: bool) -> Built {
    let mut entries = vec![(0x30000, 0, 0)];
    entries.extend(others.iter().map(|_| (0, 0, 0)));
    let mut segs = primary_segs.to_vec();
    segs.push(mpf_seg(be, &entries));
    let primary_len = with_segments(base, &segs).len();
    // SOI, the earlier segments, the MPF marker and length (4), then "MPF\0" (4).
    let endian_pos = 2 + primary_segs.iter().map(Vec::len).sum::<usize>() + 8;
    let mut entries = vec![(0x30000, primary_len as u32, 0)];
    let mut starts = Vec::new();
    let mut at = primary_len;
    for o in others {
        entries.push((0, o.len() as u32, (at - endian_pos) as u32));
        starts.push(at);
        at += o.len();
    }
    let mut segs = primary_segs.to_vec();
    segs.push(mpf_seg(be, &entries));
    let mut file = with_segments(base, &segs);
    assert_eq!(file.len(), primary_len);
    for o in others {
        file.extend_from_slice(o);
    }
    Built {
        file,
        primary_len,
        gm_len: others.last().map_or(0, |o| o.len()),
        starts,
    }
}

// ---- XMP ----

pub fn xmp_doc(decls: &str, attrs: &str, body: &str) -> String {
    format!(
        "<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><rdf:RDF xmlns:rdf=\"{RDF}\">\
         <rdf:Description rdf:about=\"\" {decls} {attrs}>{body}</rdf:Description>\
         </rdf:RDF></x:xmpmeta>"
    )
}

/// The primary image XMP of an Ultra HDR file.
pub fn primary_xmp() -> String {
    xmp_doc(HDRGM_DECL, "hdrgm:Version=\"1.0\"", "")
}

/// A gain map XMP in attribute form.
pub fn gm_xmp(gain_max: &str, capacity_max: &str) -> String {
    xmp_doc(
        HDRGM_DECL,
        &format!(
            "hdrgm:Version=\"1.0\" hdrgm:GainMapMin=\"0\" hdrgm:GainMapMax=\"{gain_max}\" \
             hdrgm:Gamma=\"1\" hdrgm:OffsetSDR=\"0.015625\" hdrgm:OffsetHDR=\"0.015625\" \
             hdrgm:HDRCapacityMin=\"0\" hdrgm:HDRCapacityMax=\"{capacity_max}\" \
             hdrgm:BaseRenditionIsHDR=\"False\""
        ),
        "",
    )
}

/// A `rdf:Seq` element `hdrgm:<name>` with these values.
pub fn seq(name: &str, values: &[&str]) -> String {
    let li: String = values
        .iter()
        .map(|v| format!("<rdf:li>{v}</rdf:li>"))
        .collect();
    format!("<hdrgm:{name}><rdf:Seq>{li}</rdf:Seq></hdrgm:{name}>")
}

/// An Apple gain map XMP.
pub fn apple_xmp() -> String {
    xmp_doc(
        APPLE_DECL,
        "",
        "<HDRGainMap:HDRGainMapVersion>65536</HDRGainMap:HDRGainMapVersion>\
         <HDRGainMap:HDRGainMapHeadroom>4.5</HDRGainMap:HDRGainMapHeadroom>",
    )
}

/// A primary XMP with a container directory: Primary, then the gain map of `len` bytes after
/// `padding` bytes of padding of the primary image.
pub fn container_xmp(len: usize, padding: usize) -> String {
    xmp_doc(
        &format!("{HDRGM_DECL} {CONTAINER_DECL}"),
        "hdrgm:Version=\"1.0\"",
        &format!(
            "<Container:Directory><rdf:Seq>\
             <rdf:li rdf:parseType=\"Resource\"><Container:Item Item:Semantic=\"Primary\" \
             Item:Mime=\"image/jpeg\" Item:Padding=\"{padding}\"/></rdf:li>\
             <rdf:li rdf:parseType=\"Resource\"><Container:Item Item:Semantic=\"GainMap\" \
             Item:Mime=\"image/jpeg\" Item:Length=\"{len}\"/></rdf:li>\
             </rdf:Seq></Container:Directory>"
        ),
    )
}

// ---- ISO 21496-1 ----

/// A payload with the common-denominator flag (denominator 1000). One channel: min 0, max 2,
/// gamma 1, offsets 0.015 and 0.025. Three channels: min 0, 0.1, 0.2; max 2, 2.1, 2.2; the rest
/// as for one. Base headroom 0, alternate headroom 3. `use_base_colour_space` is set.
pub fn iso_common(multichannel: bool, minimum_version: u16) -> Vec<u8> {
    let flags = 0x08 | 0x40 | if multichannel { 0x80 } else { 0 };
    let mut b = minimum_version.to_be_bytes().to_vec();
    b.extend_from_slice(&0u16.to_be_bytes());
    b.push(flags);
    for v in [1000u32, 0, 3000] {
        b.extend_from_slice(&v.to_be_bytes());
    }
    let channels: i32 = if multichannel { 3 } else { 1 };
    for c in 0..channels {
        for v in [c * 100, 2000 + c * 100, 1000, 15, 25] {
            b.extend_from_slice(&v.to_be_bytes());
        }
    }
    b
}

/// A payload without the common-denominator flag, one denominator per value (headrooms 1/2 and
/// 6/2; channel c: min -c/4, max (8 + c)/4, gamma 3/3, offsets 1/64 and 2/64).
pub fn iso_separate(multichannel: bool) -> Vec<u8> {
    let flags = if multichannel { 0x80u8 } else { 0 };
    let mut b = vec![0, 0, 0, 0, flags];
    for v in [1u32, 2, 6, 2] {
        b.extend_from_slice(&v.to_be_bytes());
    }
    let channels: i32 = if multichannel { 3 } else { 1 };
    for c in 0..channels {
        let pairs: [(i32, u32); 5] = [(-c, 4), (8 + c, 4), (3, 3), (1, 64), (2, 64)];
        for (n, d) in pairs {
            b.extend_from_slice(&n.to_be_bytes());
            b.extend_from_slice(&d.to_be_bytes());
        }
    }
    b
}

/// A `tmap` item payload: the version byte, then [`iso_common`] for one channel.
pub fn tmap_payload(version: u8) -> Vec<u8> {
    let mut b = vec![version];
    b.extend_from_slice(&iso_common(false, 0));
    b
}

// ---- HEIF ----

pub fn cat(parts: &[&[u8]]) -> Vec<u8> {
    parts.concat()
}

pub fn bx(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let mut v = ((body.len() + 8) as u32).to_be_bytes().to_vec();
    v.extend_from_slice(kind);
    v.extend_from_slice(body);
    v
}

pub fn fbx(kind: &[u8; 4], version: u8, body: &[u8]) -> Vec<u8> {
    let mut b = vec![version, 0, 0, 0];
    b.extend_from_slice(body);
    bx(kind, &b)
}

pub fn ispe(w: u32, h: u32) -> Vec<u8> {
    fbx(b"ispe", 0, &cat(&[&w.to_be_bytes(), &h.to_be_bytes()]))
}

/// `hvcC`: 4:2:0, 8 bit.
pub fn hvcc() -> Vec<u8> {
    let mut b = vec![
        1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 93, 0xF0, 0, 0xFC, 0xFD, 0xF8, 0xF8, 0, 0, 3, 1, 0xA0,
        0, 1, 0, 3,
    ];
    b.extend_from_slice(&[0x40, 0x01, 0x02]);
    bx(b"hvcC", &b)
}

/// `av1C`: 10 bit 4:2:0, not monochrome.
pub fn av1c() -> Vec<u8> {
    bx(b"av1C", &[0x81, 0x0c, 0x4c, 0x00, 0x0a, 0x00])
}

pub fn auxc(aux_type: &str) -> Vec<u8> {
    fbx(b"auxC", 0, &cat(&[aux_type.as_bytes(), &[0]]))
}

pub const APPLE_AUX: &str = "urn:com:apple:photo:2020:aux:hdrgainmap";

struct ItemSpec {
    id: u32,
    kind: [u8; 4],
    mime: bool,
    data: Vec<u8>,
    /// 1-based property indices, the last one essential when `essential` is set.
    assoc: Vec<u16>,
    /// How many `iloc` extents the data is split into (see [`Heif::split`]).
    extents: usize,
}

/// A HEIF file builder: `iloc` version 0, one extent per item (see [`Heif::split`]), 16-bit ids.
pub struct Heif {
    major: [u8; 4],
    pub primary: u32,
    items: Vec<ItemSpec>,
    props: Vec<Vec<u8>>,
    refs: Vec<([u8; 4], u32, Vec<u32>)>,
    groups: Vec<([u8; 4], u32, Vec<u32>)>,
}

impl Heif {
    pub fn new(major: &[u8; 4]) -> Self {
        Self {
            major: *major,
            primary: 1,
            items: Vec::new(),
            props: Vec::new(),
            refs: Vec::new(),
            groups: Vec::new(),
        }
    }

    /// Adds a property box and returns its 1-based index.
    pub fn prop(&mut self, p: Vec<u8>) -> u16 {
        self.props.push(p);
        self.props.len() as u16
    }

    pub fn item(&mut self, id: u32, kind: &[u8; 4], data: &[u8], assoc: &[u16]) -> &mut Self {
        self.items.push(ItemSpec {
            id,
            kind: *kind,
            mime: false,
            data: data.to_vec(),
            assoc: assoc.to_vec(),
            extents: 1,
        });
        self
    }

    /// Stores the data of item `id` in `n` back to back extents.
    pub fn split(&mut self, id: u32, n: usize) -> &mut Self {
        if let Some(it) = self.items.iter_mut().find(|i| i.id == id) {
            it.extents = n.clamp(1, it.data.len().max(1));
        }
        self
    }

    /// An XMP item (`mime`, `application/rdf+xml`).
    pub fn xmp_item(&mut self, id: u32, xml: &str) -> &mut Self {
        self.item(id, b"mime", xml.as_bytes(), &[]);
        if let Some(it) = self.items.last_mut() {
            it.mime = true;
        }
        self
    }

    pub fn link(&mut self, kind: &[u8; 4], from: u32, to: &[u32]) -> &mut Self {
        self.refs.push((*kind, from, to.to_vec()));
        self
    }

    pub fn group(&mut self, kind: &[u8; 4], id: u32, entities: &[u32]) -> &mut Self {
        self.groups.push((*kind, id, entities.to_vec()));
        self
    }

    fn meta(&self, mdat_at: u32) -> Vec<u8> {
        let mut kids = fbx(b"hdlr", 0, &cat(&[&[0; 4], b"pict", &[0; 13]]));
        kids.extend(fbx(b"pitm", 0, &(self.primary as u16).to_be_bytes()));
        let mut infes = Vec::new();
        for it in &self.items {
            let mut b = (it.id as u16).to_be_bytes().to_vec();
            b.extend_from_slice(&[0, 0]);
            b.extend_from_slice(&it.kind);
            b.push(0);
            if it.mime {
                b.extend_from_slice(b"application/rdf+xml\0\0");
            }
            infes.extend(fbx(b"infe", 2, &b));
        }
        kids.extend(fbx(
            b"iinf",
            0,
            &cat(&[&(self.items.len() as u16).to_be_bytes(), &infes]),
        ));
        // iloc v0: 4-byte offsets and lengths, no base, `extents` extents each.
        let mut iloc = vec![0x44, 0];
        iloc.extend_from_slice(&(self.items.len() as u16).to_be_bytes());
        let mut at = mdat_at;
        for it in &self.items {
            iloc.extend_from_slice(&(it.id as u16).to_be_bytes());
            iloc.extend_from_slice(&[0, 0]);
            iloc.extend_from_slice(&(it.extents as u16).to_be_bytes());
            // All extents have the same length, the last one takes the rest.
            let each = it.data.len() / it.extents;
            for k in 0..it.extents {
                let len = if k + 1 == it.extents {
                    it.data.len() - each * k
                } else {
                    each
                };
                iloc.extend_from_slice(&at.to_be_bytes());
                iloc.extend_from_slice(&(len as u32).to_be_bytes());
                at += len as u32;
            }
        }
        kids.extend(fbx(b"iloc", 0, &iloc));
        if !self.refs.is_empty() {
            let mut refs = Vec::new();
            for (kind, from, to) in &self.refs {
                let mut b = (*from as u16).to_be_bytes().to_vec();
                b.extend_from_slice(&(to.len() as u16).to_be_bytes());
                for t in to {
                    b.extend_from_slice(&(*t as u16).to_be_bytes());
                }
                refs.extend(bx(kind, &b));
            }
            kids.extend(fbx(b"iref", 0, &refs));
        }
        let with: Vec<&ItemSpec> = self.items.iter().filter(|i| !i.assoc.is_empty()).collect();
        let mut ipma = (with.len() as u32).to_be_bytes().to_vec();
        for it in with {
            ipma.extend_from_slice(&(it.id as u16).to_be_bytes());
            ipma.push(it.assoc.len() as u8);
            // Every association is essential: simple and enough for these files.
            ipma.extend(it.assoc.iter().map(|&i| 0x80 | i as u8));
        }
        let ipco = bx(b"ipco", &self.props.concat());
        kids.extend(bx(b"iprp", &cat(&[&ipco, &fbx(b"ipma", 0, &ipma)])));
        if !self.groups.is_empty() {
            let mut groups = Vec::new();
            for (kind, id, entities) in &self.groups {
                let mut b = id.to_be_bytes().to_vec();
                b.extend_from_slice(&(entities.len() as u32).to_be_bytes());
                for e in entities {
                    b.extend_from_slice(&e.to_be_bytes());
                }
                groups.extend(fbx(kind, 0, &b));
            }
            kids.extend(bx(b"grpl", &groups));
        }
        fbx(b"meta", 0, &kids)
    }

    /// `ftyp`, `meta`, `mdat`.
    pub fn build(&self) -> Vec<u8> {
        let ftyp = bx(b"ftyp", &cat(&[&self.major, &[0; 4], b"mif1", &self.major]));
        let mdat_at = (ftyp.len() + self.meta(0).len() + 8) as u32;
        let mut out = ftyp;
        out.extend(self.meta(mdat_at));
        let data: Vec<u8> = self.items.iter().flat_map(|i| i.data.clone()).collect();
        out.extend(bx(b"mdat", &data));
        out
    }
}

/// An AVIF gain map file: base 1 (data `[1; 4]`), gain map 2 (`[2; 4]`, 32 x 24), a `tmap` item 3
/// (the primary item) with `dimg` [1, 2] and an `altr` group [3, 1].
pub fn avif_tmap(tmap_version: u8) -> Vec<u8> {
    let mut s = Heif::new(b"avif");
    s.primary = 3;
    let size = s.prop(ispe(64, 48));
    let small = s.prop(ispe(32, 24));
    let cfg = s.prop(av1c());
    s.item(1, b"av01", &[1; 4], &[size, cfg]);
    s.item(2, b"av01", &[2; 4], &[small, cfg]);
    s.item(3, b"tmap", &tmap_payload(tmap_version), &[]);
    s.link(b"dimg", 3, &[1, 2]);
    s.group(b"altr", 100, &[3, 1]);
    s.build()
}

/// A HEIC file with an Apple gain map: base 1, gain map 2 (`auxl`), XMP item 3 on the gain map.
pub fn heic_apple() -> Vec<u8> {
    let mut s = Heif::new(b"heic");
    let size = s.prop(ispe(64, 48));
    let small = s.prop(ispe(32, 24));
    let cfg = s.prop(hvcc());
    let aux = s.prop(auxc(APPLE_AUX));
    s.item(1, b"hvc1", &[1; 4], &[size, cfg]);
    s.item(2, b"hvc1", &[2; 4], &[small, cfg, aux]);
    s.xmp_item(3, &apple_xmp());
    s.link(b"auxl", 2, &[1]);
    s.link(b"cdsc", 3, &[2]);
    s.build()
}

/// Like [`avif_tmap`], with the given `tmap` payload (the version byte included) and `altr` group.
pub fn avif_tmap_custom(payload: &[u8], altr: &[u32]) -> Vec<u8> {
    let mut s = Heif::new(b"avif");
    s.primary = 3;
    let size = s.prop(ispe(64, 48));
    let small = s.prop(ispe(32, 24));
    let cfg = s.prop(av1c());
    s.item(1, b"av01", &[1; 4], &[size, cfg]);
    s.item(2, b"av01", &[2; 4], &[small, cfg]);
    s.item(3, b"tmap", payload, &[]);
    s.link(b"dimg", 3, &[1, 2]);
    s.group(b"altr", 100, altr);
    s.build()
}
