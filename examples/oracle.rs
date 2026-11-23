//! The parity oracle of gainmap (docs/parity.md).
//!
//! `oracle uhdr <in-dir> <out-dir> [--no-raw]`: for every regular file `<f>` of `<in-dir>` (sorted) writes
//! `<out-dir>/<f>.txt` and, as far as they exist, `<f>.base.jpg`, `<f>.gm.jpg` (the coded bytes
//! of the base image and of the gain map), `<f>.base.rgba` and `<f>.gm.raw` (the decoded
//! pixels, tightly packed: RGBA8888, or 8-bit gray for a gain map with one component).
//! `--no-raw` writes the `.txt` files only (the no-crash sweep and the bench use it).
//! tests/parity/uhdr_oracle.cpp writes the same files with libultrahdr v2.0.2 and libjpeg-turbo.
//!
//! `oracle avif <in-dir> <out-dir>`: `<f>.txt` with the gain-map fractions, as
//! tests/parity/avif_oracle.c prints them with libavif v1.4.2.
//!
//! `oracle xmp <in-dir> <out.jsonl>`: one JSON object per file with the `hdrgm` and
//! `HDRGainMap` XMP values of the primary and the gain-map image, as strings.
//! tests/parity/compare.py compares them with ExifTool 13.64.
//!
//! `oracle gen <in-dir> <out-dir>`: deterministic mutations of every JPEG of `<in-dir>` (at
//! most 8 MB): truncations, bit flips in the MPF and ISO payloads, MPF offset edits. Used by
//! the no-crash sweep of tests/parity/run.sh; not compared with a C tool.
//!
//! The text format is in docs/parity.md. Output is deterministic and, for the same libm, the
//! same on every platform.

use std::fmt::Write as _;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::{env, fs, io, process};

use gainmap::{Container, Fraction, GainMapImage, Limits, Metadata};

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let result = match args.as_slice() {
        ["uhdr", dir, out] => uhdr_cmd(Path::new(dir), Path::new(out), true),
        ["uhdr", dir, out, "--no-raw"] => uhdr_cmd(Path::new(dir), Path::new(out), false),
        ["avif", dir, out] => avif_cmd(Path::new(dir), Path::new(out)),
        ["xmp", dir, out] => xmp_cmd(Path::new(dir), Path::new(out)),
        ["gen", dir, out] => gen_cmd(Path::new(dir), Path::new(out)),
        _ => {
            eprintln!(
                "usage: oracle uhdr <in-dir> <out-dir>\n       oracle avif <in-dir> <out-dir>\n       \
                 oracle xmp <in-dir> <out.jsonl>\n       oracle gen <in-dir> <out-dir>"
            );
            process::exit(2);
        }
    };
    if let Err(e) = result {
        eprintln!("oracle: {e}");
        process::exit(1);
    }
}

/// Default limits, so the threaded decode path is part of the parity run too (the output does
/// not depend on the thread count).
fn limits() -> Limits {
    Limits::default()
}

/// The regular files of `dir` (symlinks followed), sorted by name.
fn files(dir: &Path) -> io::Result<Vec<(String, PathBuf)>> {
    let mut out = Vec::new();
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        if fs::metadata(&path).is_ok_and(|m| m.is_file()) {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            out.push((name, path));
        }
    }
    out.sort();
    Ok(out)
}

// ---- uhdr ----

/// The 8 hex digits of the bits of `f`.
fn hex(f: f32) -> String {
    format!("{:08x}", f.to_bits())
}

fn hex3(v: [f32; 3]) -> String {
    format!("{} {} {}", hex(v[0]), hex(v[1]), hex(v[2]))
}

/// `exp2` as `gainmapMetadataFractionToFloat` (lib/src/gainmapmetadata.cpp, libultrahdr v2.0.2)
/// calls it: that file includes `<cmath>` but has no `using namespace std`, so
/// `exp2((float)n / d)` is the C `exp2(double)`; the double is then stored in a float.
fn exp2_iso(x: f32) -> f32 {
    f64::from(x).exp2() as f32
}

/// `exp2` as `XMPXmlHandler` (lib/src/jpegrutils.cpp) calls it: that file has `using namespace
/// std`, and the argument is a `float`, so `std::exp2(float)` (`exp2f`) is chosen.
fn exp2_xmp(x: f32) -> f32 {
    x.exp2()
}

/// What `oracle uhdr` writes for one file.
struct Uhdr {
    text: String,
    files: Vec<(&'static str, Vec<u8>)>,
}

fn status_only(class: &str) -> String {
    format!("status {class}\n")
}

/// The metadata lines of `<f>.txt`: libultrahdr's linear `uhdr_gainmap_metadata_t`.
fn metadata_lines(md: &Metadata, from_iso: bool) -> String {
    let exp2: fn(f32) -> f32 = if from_iso { exp2_iso } else { exp2_xmp };
    let map = |v: [f32; 3]| [exp2(v[0]), exp2(v[1]), exp2(v[2])];
    let mut t = String::new();
    let _ = writeln!(t, "max_content_boost {}", hex3(map(md.gain_map_max)));
    let _ = writeln!(t, "min_content_boost {}", hex3(map(md.gain_map_min)));
    let _ = writeln!(t, "gamma {}", hex3(md.gamma));
    let _ = writeln!(t, "offset_sdr {}", hex3(md.base_offset));
    let _ = writeln!(t, "offset_hdr {}", hex3(md.alternate_offset));
    let _ = writeln!(t, "hdr_capacity_min {}", hex(exp2(md.base_hdr_headroom)));
    let _ = writeln!(
        t,
        "hdr_capacity_max {}",
        hex(exp2(md.alternate_hdr_headroom))
    );
    // XMP has no colour-space field: libultrahdr sets use_base_cg to true for it.
    let base_cg = if from_iso {
        md.use_base_colour_space
    } else {
        true
    };
    let _ = writeln!(t, "use_base_cg {}", u8::from(base_cg));
    t
}

fn uhdr_one(data: &[u8], limits: &Limits, cancel: &AtomicBool) -> Uhdr {
    let mut out = Uhdr {
        text: String::new(),
        files: Vec::new(),
    };
    let image: GainMapImage = match gainmap::read(data, limits, cancel) {
        Ok(i) => i,
        Err(e) => {
            out.text = status_only(e.class());
            return out;
        }
    };
    let Some(gm) = &image.gain_map else {
        out.text = status_only("no-gain-map");
        return out;
    };
    if !matches!(image.container, Container::Jpeg) {
        out.text = status_only("not-jpeg");
        return out;
    }
    // The coded bytes and the pixels are written whenever they exist, whatever the status, so a
    // file that only one side rejects still gets its other channels compared.
    for (ext, img) in [("base.jpg", &image.base), ("gm.jpg", &gm.image)] {
        match image.coded_bytes(data, img, limits, cancel) {
            Ok(b) => out.files.push((ext, b.into_owned())),
            Err(e) => {
                out.text = status_only(e.class());
                return out;
            }
        }
    }
    let decoded = match gainmap::decode(data, &image, limits, cancel) {
        Ok(d) => d,
        Err(e) => {
            out.text = status_only(e.class());
            return out;
        }
    };
    let Some(gm_pixels) = &decoded.gain_map else {
        out.text = status_only("no-gain-map-pixels");
        return out;
    };
    out.files.push(("base.rgba", decoded.base.data.clone()));
    out.files.push(("gm.raw", gm_pixels.data.clone()));
    let Some(md) = &gm.metadata else {
        out.text = status_only("no-metadata");
        return out;
    };
    // libultrahdr refuses a map whose base rendition is the HDR one (ISO flag 0x04, XMP
    // BaseRenditionIsHDR True).
    if md.backward_direction {
        out.text = status_only("backward-direction");
        return out;
    }
    // The metadata came from the ISO payload when that payload alone gives exactly this value.
    let from_iso = gm
        .iso
        .as_ref()
        .and_then(|i| Metadata::from_iso(i).ok())
        .is_some_and(|m| &m == md);
    let mut t = String::from("status ok\n");
    let _ = writeln!(t, "base.width {}", image.base.width);
    let _ = writeln!(t, "base.height {}", image.base.height);
    let _ = writeln!(t, "gainmap.width {}", gm_pixels.width);
    let _ = writeln!(t, "gainmap.height {}", gm_pixels.height);
    let _ = writeln!(t, "gainmap.channels {}", gm_pixels.channels);
    t.push_str(&metadata_lines(md, from_iso));
    out.text = t;
    out
}

fn uhdr_cmd(dir: &Path, out: &Path, raw: bool) -> io::Result<()> {
    fs::create_dir_all(out)?;
    let limits = limits();
    let cancel = AtomicBool::new(false);
    for (name, path) in files(dir)? {
        let data = fs::read(&path)?;
        let r = uhdr_one(&data, &limits, &cancel);
        for (ext, bytes) in r.files.iter().filter(|_| raw) {
            fs::write(out.join(format!("{name}.{ext}")), bytes)?;
        }
        fs::write(out.join(format!("{name}.txt")), r.text)?;
    }
    Ok(())
}

// ---- avif ----

fn frac(f: Fraction) -> String {
    format!("{}/{}", f.numerator, f.denominator)
}

fn avif_one(data: &[u8], limits: &Limits, cancel: &AtomicBool) -> String {
    let image = match gainmap::read(data, limits, cancel) {
        Ok(i) => i,
        Err(e) => return status_only(e.class()),
    };
    // libavif ignores a gain map whose metadata it cannot use (unsupported version, wrong
    // altr group); gainmap keeps it with `metadata: None`. Both are "no gain map" here.
    let Some(gm) = image.gain_map.as_ref() else {
        return status_only("no-gainmap");
    };
    let (Some(iso), Some(_)) = (&gm.iso, &gm.metadata) else {
        return status_only("no-gainmap");
    };
    let mut t = String::from("status ok\n");
    let _ = writeln!(t, "base.width {}", image.base.width);
    let _ = writeln!(t, "base.height {}", image.base.height);
    let _ = writeln!(t, "gainmap.width {}", gm.image.width);
    let _ = writeln!(t, "gainmap.height {}", gm.image.height);
    let _ = writeln!(t, "flags {}", u8::from(iso.use_base_colour_space()));
    let _ = writeln!(t, "base_hdr_headroom {}", frac(iso.base_hdr_headroom));
    let _ = writeln!(
        t,
        "alternate_hdr_headroom {}",
        frac(iso.alternate_hdr_headroom)
    );
    for (c, ch) in iso.channels.iter().enumerate() {
        let _ = writeln!(t, "{c}.min {}", frac(ch.gain_map_min));
        let _ = writeln!(t, "{c}.max {}", frac(ch.gain_map_max));
        let _ = writeln!(t, "{c}.gamma {}", frac(ch.gamma));
        let _ = writeln!(t, "{c}.base_offset {}", frac(ch.base_offset));
        let _ = writeln!(t, "{c}.alternate_offset {}", frac(ch.alternate_offset));
    }
    t
}

fn avif_cmd(dir: &Path, out: &Path) -> io::Result<()> {
    fs::create_dir_all(out)?;
    let limits = limits();
    let cancel = AtomicBool::new(false);
    for (name, path) in files(dir)? {
        let data = fs::read(&path)?;
        fs::write(
            out.join(format!("{name}.txt")),
            avif_one(&data, &limits, &cancel),
        )?;
    }
    Ok(())
}

// ---- xmp ----

fn json_string(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\0'..='\u{1f}' | '\u{7f}' => {
                let _ = write!(out, "\\u{:04x}", u32::from(c));
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

/// `,"key":"value"` or `,"key":["a","b"]`; nothing for an absent value.
struct Obj {
    text: String,
}

impl Obj {
    fn new() -> Obj {
        Obj {
            text: String::from("{"),
        }
    }

    fn key(&mut self, key: &str) {
        if self.text.len() > 1 {
            self.text.push(',');
        }
        json_string(&mut self.text, key);
        self.text.push(':');
    }

    fn one(&mut self, key: &str, v: Option<&String>) {
        if let Some(v) = v {
            self.key(key);
            json_string(&mut self.text, v);
        }
    }

    fn list(&mut self, key: &str, v: &[String]) {
        if !v.is_empty() {
            self.key(key);
            self.text.push('[');
            for (i, s) in v.iter().enumerate() {
                if i > 0 {
                    self.text.push(',');
                }
                json_string(&mut self.text, s);
            }
            self.text.push(']');
        }
    }

    fn raw(&mut self, key: &str, v: &str) {
        self.key(key);
        self.text.push_str(v);
    }

    fn finish(mut self) -> String {
        self.text.push('}');
        self.text
    }
}

fn xmp_one(name: &str, data: &[u8], limits: &Limits, cancel: &AtomicBool) -> String {
    let mut o = Obj::new();
    o.key("SourceFile");
    json_string(&mut o.text, name);
    match gainmap::read(data, limits, cancel) {
        Err(e) => o.raw("Error", &format!("{:?}", e.class())),
        Ok(image) => match &image.gain_map {
            None => o.raw("Error", "\"no-gain-map\""),
            Some(gm) => {
                let mut p = Obj::new();
                p.one("Version", gm.primary_version.as_ref());
                o.raw("primary", &p.finish());
                let mut g = Obj::new();
                if let Some(x) = &gm.xmp {
                    g.one("Version", x.version.as_ref());
                    g.list("GainMapMin", &x.gain_map_min);
                    g.list("GainMapMax", &x.gain_map_max);
                    g.list("Gamma", &x.gamma);
                    g.list("OffsetSDR", &x.offset_sdr);
                    g.list("OffsetHDR", &x.offset_hdr);
                    g.one("HDRCapacityMin", x.hdr_capacity_min.as_ref());
                    g.one("HDRCapacityMax", x.hdr_capacity_max.as_ref());
                    g.one("BaseRenditionIsHDR", x.base_rendition_is_hdr.as_ref());
                }
                if let Some(a) = &gm.apple {
                    g.one("HDRGainMapVersion", a.version.as_ref());
                    g.one("HDRGainMapHeadroom", a.headroom.as_ref());
                }
                o.raw("gainmap", &g.finish());
            }
        },
    }
    o.finish()
}

fn xmp_cmd(dir: &Path, out: &Path) -> io::Result<()> {
    let limits = limits();
    let cancel = AtomicBool::new(false);
    let mut text = String::new();
    for (name, path) in files(dir)? {
        let data = fs::read(&path)?;
        text.push_str(&xmp_one(&name, &data, &limits, &cancel));
        text.push('\n');
    }
    fs::write(out, text)
}

// ---- gen ----

/// A segment of the first JPEG image before its first SOS.
struct Seg {
    /// Offset of the `FF` of the marker.
    at: usize,
    payload: Range<usize>,
}

/// The segments of the first image, up to and including the first SOS (at most 64).
fn segments(d: &[u8]) -> Vec<Seg> {
    let mut out = Vec::new();
    if d.get(..2) != Some(&[0xff, 0xd8][..]) {
        return out;
    }
    let mut i = 2;
    while out.len() < 64 {
        if d.get(i) != Some(&0xff) {
            break;
        }
        let Some(&m) = d.get(i + 1) else { break };
        if m == 0xff {
            i += 1;
            continue;
        }
        if m == 0x01 || (0xd0..=0xd9).contains(&m) {
            i += 2;
            continue;
        }
        let Some(len) = d.get(i + 2..i + 4) else {
            break;
        };
        let len = usize::from(u16::from_be_bytes([len[0], len[1]]));
        let end = (i + 2 + len).min(d.len());
        out.push(Seg {
            at: i,
            payload: i + 4..end.max(i + 4),
        });
        if m == 0xda {
            break;
        }
        i += 2 + len;
    }
    out
}

fn put32(d: &mut [u8], at: usize, v: u32, big: bool) {
    let b = if big {
        v.to_be_bytes()
    } else {
        v.to_le_bytes()
    };
    if let Some(s) = d.get_mut(at..at + 4) {
        s.copy_from_slice(&b);
    }
}

fn get32(d: &[u8], at: usize, big: bool) -> Option<u32> {
    let b: [u8; 4] = d.get(at..at + 4)?.try_into().ok()?;
    Some(if big {
        u32::from_be_bytes(b)
    } else {
        u32::from_le_bytes(b)
    })
}

/// The file offsets of the offset and size fields of MP entry 1, and the byte order.
fn mpf_entry1(d: &[u8], mpf: &Range<usize>) -> Option<(usize, usize, bool)> {
    let base = mpf.start + 4;
    let big = match d.get(base..base + 2)? {
        b"MM" => true,
        b"II" => false,
        _ => return None,
    };
    let ifd = base + usize::try_from(get32(d, base + 4, big)?).ok()?;
    let count = {
        let b: [u8; 2] = d.get(ifd..ifd + 2)?.try_into().ok()?;
        usize::from(if big {
            u16::from_be_bytes(b)
        } else {
            u16::from_le_bytes(b)
        })
    };
    for t in 0..count.min(32) {
        let tag = ifd + 2 + t * 12;
        let id: [u8; 2] = d.get(tag..tag + 2)?.try_into().ok()?;
        let id = if big {
            u16::from_be_bytes(id)
        } else {
            u16::from_le_bytes(id)
        };
        if id == 0xb002 {
            let entries = base + usize::try_from(get32(d, tag + 8, big)?).ok()?;
            return Some((entries + 16 + 8, entries + 16 + 4, big));
        }
    }
    None
}

fn gen_one(name: &str, d: &[u8], out: &Path) -> io::Result<usize> {
    const MAX: usize = 64;
    let mut n = 0usize;
    let mut emit = |kind: &str, bytes: &[u8]| -> io::Result<()> {
        if n < MAX {
            fs::write(out.join(format!("{name}.{kind}{n:02}")), bytes)?;
            n += 1;
        }
        Ok(())
    };
    let segs = segments(d);

    // Truncations: at the start and the end of each of the first 16 segments, at the end of the
    // primary image and around the start of the second one.
    for s in segs.iter().take(16) {
        emit("trunc", d.get(..s.at).unwrap_or(d))?;
        emit("trunc", d.get(..s.payload.end).unwrap_or(d))?;
    }
    if let Some(p) = d.windows(4).position(|w| w == [0xff, 0xd9, 0xff, 0xd8]) {
        for cut in [p + 1, p + 2, p + 3, p + 4, p + 5, p + 40] {
            emit("trunc", d.get(..cut).unwrap_or(d))?;
        }
    }

    // Bit flips: every third byte of the first 40 of the MPF payload, every fifth of the first
    // 70 bytes after the ISO namespace of each ISO payload.
    let iso_ns = b"urn:iso:std:iso:ts:21496:-1\0";
    for s in &segs {
        let p = d.get(s.payload.clone()).unwrap_or_default();
        let (start, step, len) = if p.starts_with(b"MPF\0") {
            (s.payload.start, 3, 40)
        } else if p.starts_with(iso_ns) {
            (s.payload.start + iso_ns.len(), 5, 70)
        } else {
            continue;
        };
        for off in (0..len).step_by(step) {
            let at = start + off;
            if at < s.payload.end {
                let mut m = d.to_vec();
                m[at] ^= 0x80;
                emit("flip", &m)?;
            }
        }
    }

    // MPF offset edits: the offset and the size of MP entry 1.
    for s in &segs {
        if !d
            .get(s.payload.clone())
            .is_some_and(|p| p.starts_with(b"MPF\0"))
        {
            continue;
        }
        let Some((off_at, size_at, big)) = mpf_entry1(d, &s.payload) else {
            continue;
        };
        let len = u32::try_from(d.len()).unwrap_or(u32::MAX);
        let was = get32(d, off_at, big).unwrap_or(0);
        for v in [0, u32::MAX, len, was.wrapping_sub(1), was.wrapping_add(1)] {
            let mut m = d.to_vec();
            put32(&mut m, off_at, v, big);
            emit("mpfoff", &m)?;
        }
        for v in [0, u32::MAX] {
            let mut m = d.to_vec();
            put32(&mut m, size_at, v, big);
            emit("mpfsize", &m)?;
        }
    }
    Ok(n)
}

fn gen_cmd(dir: &Path, out: &Path) -> io::Result<()> {
    fs::create_dir_all(out)?;
    for (name, path) in files(dir)? {
        let d = fs::read(&path)?;
        if d.len() <= 8 << 20 && d.starts_with(&[0xff, 0xd8]) {
            gen_one(&name, &d, out)?;
        }
    }
    Ok(())
}
