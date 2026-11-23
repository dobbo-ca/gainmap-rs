//! Resource controls: a counting global allocator measures the real peak of `read` and `decode`
//! and checks it against `max_alloc`; the threads of the nested JPEG decode stay under
//! `max_threads`; this crate spawns no thread.

mod common;

use std::alloc::{GlobalAlloc, Layout, System};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use common::*;
use gainmap::{Error, Limit, Limits};

// ---- counting allocator -------------------------------------------------------------------

static CUR: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

struct Counting;

fn grow(n: usize) {
    let now = CUR.fetch_add(n, Ordering::Relaxed) + n;
    PEAK.fetch_max(now, Ordering::Relaxed);
}

// SAFETY: every method forwards to `System` with the same arguments and only adds counters.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: the caller upholds the `GlobalAlloc::alloc` contract; passed through.
        let p = unsafe { System.alloc(layout) };
        if !p.is_null() {
            grow(layout.size());
        }
        p
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: `ptr` and `layout` come from a matching `alloc`, which was `System.alloc`.
        unsafe { System.dealloc(ptr, layout) };
        CUR.fetch_sub(layout.size(), Ordering::Relaxed);
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        // SAFETY: the caller upholds the `GlobalAlloc::realloc` contract; passed through.
        let p = unsafe { System.realloc(ptr, layout, new_size) };
        if !p.is_null() {
            if new_size >= layout.size() {
                grow(new_size - layout.size());
            } else {
                CUR.fetch_sub(layout.size() - new_size, Ordering::Relaxed);
            }
        }
        p
    }
}

#[global_allocator]
static ALLOC: Counting = Counting;

/// The tests of this file share one counter, so they run one at a time.
static LOCK: Mutex<()> = Mutex::new(());

fn lock() -> std::sync::MutexGuard<'static, ()> {
    LOCK.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Runs `f` and returns its result and the peak of live bytes above the level when it started.
/// The result is still alive when the peak is read, so it counts.
fn measured<R>(f: impl FnOnce() -> R) -> (R, usize) {
    let base = CUR.load(Ordering::Relaxed);
    PEAK.store(base, Ordering::Relaxed);
    let r = f();
    (r, PEAK.load(Ordering::Relaxed).saturating_sub(base))
}

/// What `read` may hold beyond its ledger: small fixed fields of the result, `Vec` doubling of a
/// few entries, the strings of the XMP properties. The inputs have tens of properties.
const READ_SLACK: usize = 256 << 10;
/// What `jpeg` may hold beyond its ledger: tables and frame state under 64 KiB each, spare
/// vector capacity, thread set-up (the same 1 MiB as jpeg-rs tests/resources.rs).
const DECODE_SLACK: usize = 1 << 20;

fn within(what: &str, peak: usize, max_alloc: usize, slack: usize) {
    assert!(
        peak <= max_alloc.saturating_add(slack),
        "{what}: peak {peak} > max_alloc {max_alloc} + slack {slack}"
    );
}

fn limits_of(max_alloc: usize, max_threads: usize) -> Limits {
    Limits {
        max_alloc,
        max_threads,
        ..Limits::default()
    }
}

fn ultra_hdr(base: usize, gm: usize) -> Vec<u8> {
    let gm = with_segments(&jpeg_gray(gm, gm), &[app1_xmp(&gm_xmp("2", "2.5"))]);
    with_mpf(
        &[app1_xmp(&primary_xmp())],
        &jpeg_rgb(base, base),
        &gm,
        true,
    )
    .file
}

// ---- tests --------------------------------------------------------------------------------

// Small inputs: the call succeeds above 1 MiB, and its measured peak stays within max_alloc plus
// the slack at every cap, whether it succeeds or hits the limit.
#[test]
fn read_peak_within_max_alloc() {
    let _g = lock();
    let inputs = [
        ("jpeg", ultra_hdr(16, 8)),
        ("avif", avif_tmap(0)),
        ("heic", heic_apple()),
    ];
    for max_alloc in [64 << 10, 1 << 20, Limits::default().max_alloc] {
        let lim = limits_of(max_alloc, 1);
        for (name, data) in &inputs {
            let at = format!("{name} max_alloc {max_alloc}");
            let (r, peak) = measured(|| gainmap::read(data, &lim, &no_cancel()));
            match &r {
                Ok(_) => {}
                Err(e) => {
                    assert!(max_alloc < 1 << 20, "{at}: {e}");
                    assert_eq!(e, &Error::LimitExceeded(Limit::Alloc), "{at}");
                }
            }
            within(&at, peak, max_alloc, READ_SLACK);
        }
    }
}

// A larger XMP packet: a cap below its size gives LimitExceeded(Alloc) before the copy.
#[test]
fn read_charges_the_xmp_packet() {
    let _g = lock();
    // Many properties: about 55 KB, the most that fits in one APP1 segment.
    let mut props = String::new();
    for i in 0..1500 {
        props.push_str(&format!("<hdrgm:P{i}>value {i}</hdrgm:P{i}>"));
    }
    let xml = xmp_doc(HDRGM_DECL, "hdrgm:Version=\"1.0\"", &props);
    assert!(xml.len() > 40_000 && xml.len() < 60_000, "{}", xml.len());
    let data = with_segments(&jpeg_rgb(16, 16), &[app1_xmp(&xml)]);
    let lim = limits_of(16 << 10, 1);
    let (r, peak) = measured(|| gainmap::read(&data, &lim, &no_cancel()).map(|_| ()));
    assert_eq!(r, Err(Error::LimitExceeded(Limit::Alloc)));
    within("xmp at 16 KiB", peak, lim.max_alloc, READ_SLACK);
    let lim = limits_of(8 << 20, 1);
    let (r, peak) = measured(|| gainmap::read(&data, &lim, &no_cancel()).map(|_| ()));
    assert_eq!(r, Ok(()));
    within("xmp at 8 MiB", peak, lim.max_alloc, READ_SLACK);
}

// Decode of two images: the base stays alive while the gain map decodes. The peak stays within
// max_alloc for every cap and thread count, and an Ok result is the same for all of them.
#[test]
fn decode_peak_within_max_alloc() {
    let _g = lock();
    let data = ultra_hdr(256, 128);
    let img = gainmap::read(&data, &limits_of(1 << 30, 1), &no_cancel()).unwrap();
    let want = gainmap::decode(&data, &img, &limits_of(1 << 30, 1), &no_cancel()).unwrap();
    assert_eq!(want.base.data.len(), 256 * 256 * 4);
    for t in [1, 2] {
        for max_alloc in [
            256 << 10,
            1 << 20,
            2 << 20,
            8 << 20,
            Limits::default().max_alloc,
        ] {
            let lim = limits_of(max_alloc, t);
            let at = format!("threads {t} max_alloc {max_alloc}");
            let (r, peak) = measured(|| gainmap::decode(&data, &img, &lim, &no_cancel()));
            match &r {
                Ok(d) => assert_eq!(d, &want, "{at}"),
                Err(e) => assert_eq!(e, &Error::LimitExceeded(Limit::Alloc), "{at}"),
            }
            if max_alloc >= 8 << 20 {
                assert!(r.is_ok(), "{at}");
            }
            within(&at, peak, max_alloc, DECODE_SLACK);
        }
    }
}

// The threads of the nested JPEG decode stay under max_threads, and 1 never spawns.
#[test]
fn nested_decode_threads_stay_under_max_threads() {
    let _g = lock();
    let data = ultra_hdr(256, 128);
    let img = gainmap::read(&data, &limits_of(1 << 30, 1), &no_cancel()).unwrap();
    let gm = img.gain_map.as_ref().unwrap();
    for coded in [&img.base, &gm.image] {
        let bytes = img
            .coded_bytes(&data, coded, &limits(), &no_cancel())
            .unwrap();
        for t in [1usize, 2] {
            let l = jpeg::Limits {
                max_threads: t,
                ..jpeg::Limits::default()
            };
            let (r, probe) = jpeg::decode_probe(
                &bytes,
                &jpeg::DecodeOptions::default(),
                &l,
                &no_cancel(),
                true,
            );
            assert!(r.is_ok());
            assert!(
                probe.threads_peak <= t,
                "threads {t}: {}",
                probe.threads_peak
            );
            if t == 1 {
                assert_eq!(probe.threads_peak, 1);
            }
        }
    }
    // And the result of the whole call is the same for both values.
    let one = gainmap::decode(&data, &img, &limits_of(1 << 30, 1), &no_cancel());
    let two = gainmap::decode(&data, &img, &limits_of(1 << 30, 2), &no_cancel());
    assert!(one.is_ok());
    assert_eq!(one, two);
}

// The grep-able claim of docs/threading.md: nothing in src/ starts a thread.
#[test]
fn src_spawns_no_thread() {
    fn rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(rd) = fs::read_dir(dir) else { return };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                rs_files(&p, out);
            } else if p.extension().is_some_and(|x| x == "rs") {
                out.push(p);
            }
        }
    }
    let mut files = Vec::new();
    rs_files(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
        &mut files,
    );
    assert!(files.len() >= 8, "found only {} source files", files.len());
    let banned = ["thread::spawn", "thread::scope", "thread::Builder"];
    let mut hits = Vec::new();
    for f in &files {
        let Ok(text) = fs::read_to_string(f) else {
            continue;
        };
        for (i, line) in text.lines().enumerate() {
            let t = line.trim_start();
            if !t.starts_with("//") && banned.iter().any(|b| t.contains(b)) {
                hits.push(format!("{}:{}", f.display(), i + 1));
            }
        }
    }
    assert!(hits.is_empty(), "thread use in src: {hits:?}");
}
