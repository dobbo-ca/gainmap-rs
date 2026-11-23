//! Resource controls: a counting global allocator measures the real peak of `read` and `decode`
//! and checks it against `max_alloc`; the threads of the nested JPEG decode stay under
//! `max_threads`; this crate spawns no thread.

mod common;

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};

use common::*;
use gainmap::{Error, Limit, Limits};

// ---- counting allocator -------------------------------------------------------------------

static CUR: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

struct Counting;

// Which threads allocate while `TRACK` is set: each thread takes a slot number on its first
// allocation and sets that bit in `SEEN`. A thread that does work for `decode` allocates (its
// own buffers, its job state), so the number of set bits is the number of threads that ran it.
static TRACK: AtomicBool = AtomicBool::new(false);
static SEEN: AtomicU64 = AtomicU64::new(0);
static NEXT_SLOT: AtomicUsize = AtomicUsize::new(0);

thread_local! {
    // Const-initialised with no destructor, so the allocator can touch it without allocating.
    static SLOT: Cell<usize> = const { Cell::new(usize::MAX) };
}

fn note_thread() {
    let _ = SLOT.try_with(|s| {
        if s.get() == usize::MAX {
            s.set(NEXT_SLOT.fetch_add(1, Ordering::Relaxed));
        }
        SEEN.fetch_or(1 << s.get().min(63), Ordering::Relaxed);
    });
}

fn grow(n: usize) {
    let now = CUR.fetch_add(n, Ordering::Relaxed) + n;
    PEAK.fetch_max(now, Ordering::Relaxed);
    if TRACK.load(Ordering::Relaxed) {
        note_thread();
    }
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

/// What `read` may hold beyond its ledger: small fixed fields of the result and `Vec` doubling of
/// a few entries. The big-input tests below need several MiB, so this is under a tenth of that.
const READ_SLACK: usize = 256 << 10;
/// What `jpeg` may hold beyond its ledger: tables and frame state under 64 KiB each, spare
/// vector capacity, thread set-up (the same 1 MiB as jpeg-rs tests/resources.rs). The decode
/// tests below need 9 MiB or more, so this is about a ninth of that.
const DECODE_SLACK: usize = 1 << 20;

/// The base and gain map sizes of the big Ultra HDR file: the base alone decodes to 9 MiB of
/// RGBA, and the frame has 2.4 million pixels, enough for jpeg-rs to cut it into threaded work.
const BIG_BASE: usize = 1536;
const BIG_GM: usize = 768;

/// The smallest `max_alloc` (to within 64 KiB) at which `ok` holds, found by bisection, as
/// `(just_below, enough)`: `ok(just_below)` is false and `ok(enough)` is true.
fn min_alloc(ok: impl Fn(usize) -> bool) -> (usize, usize) {
    let (mut lo, mut hi) = (0, 64 << 20);
    assert!(ok(hi), "fails even at {hi} bytes");
    while hi - lo > 64 << 10 {
        let mid = lo + (hi - lo) / 2;
        if ok(mid) {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    (lo, hi)
}

fn is_alloc_error(e: &Error) -> bool {
    matches!(e, Error::LimitExceeded(Limit::Alloc) | Error::OutOfMemory)
}

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

// A HEIC file whose gain map XMP item is several MiB: 2000 properties of 1.5 KB each. It is one
// extent, so `read` borrows it from the input and then copies it once for the XMP parse.
fn heic_big_xmp() -> Vec<u8> {
    let xml = big_xmp_xml();
    let mut s = Heif::new(b"heic");
    let size = s.prop(ispe(64, 48));
    let small = s.prop(ispe(32, 24));
    let cfg = s.prop(hvcc());
    let aux = s.prop(auxc(APPLE_AUX));
    s.item(1, b"hvc1", &[1; 4], &[size, cfg]);
    s.item(2, b"hvc1", &[2; 4], &[small, cfg, aux]);
    s.xmp_item(3, &xml);
    s.link(b"auxl", 2, &[1]);
    s.link(b"cdsc", 3, &[2]);
    s.build()
}

// The same file with the XMP item cut into 8 extents, which `read` has to copy into one buffer,
// and 40 `iovl` overlay items of 8000 inputs each, which stay in the parsed graph for the whole
// call (about 4 MiB of overlay inputs, plus their `dimg` lists).
fn heic_big_graph() -> Vec<u8> {
    let xml = big_xmp_xml();
    let mut s = Heif::new(b"heic");
    let size = s.prop(ispe(64, 48));
    let small = s.prop(ispe(32, 24));
    let cfg = s.prop(hvcc());
    let aux = s.prop(auxc(APPLE_AUX));
    s.item(1, b"hvc1", &[1; 4], &[size, cfg]);
    s.item(2, b"hvc1", &[2; 4], &[small, cfg, aux]);
    s.xmp_item(3, &xml);
    s.split(3, 8);
    s.link(b"auxl", 2, &[1]);
    s.link(b"cdsc", 3, &[2]);
    const INPUTS: usize = 8000;
    // Version 0, flags 0, four fill words, a 64 x 48 canvas, then an (x, y) pair per input.
    let mut iovl = vec![0u8; 2 + 8];
    iovl.extend_from_slice(&64u16.to_be_bytes());
    iovl.extend_from_slice(&48u16.to_be_bytes());
    iovl.extend(vec![0u8; 4 * INPUTS]);
    for id in 10..50 {
        s.item(id, b"iovl", &iovl, &[size]);
        s.link(b"dimg", id, &[1; INPUTS]);
    }
    s.build()
}

fn big_xmp_xml() -> String {
    let mut props = String::from(
        "<HDRGainMap:HDRGainMapVersion>65536</HDRGainMap:HDRGainMapVersion>\
         <HDRGainMap:HDRGainMapHeadroom>4.5</HDRGainMap:HDRGainMapHeadroom>",
    );
    let value = "v".repeat(1500);
    for i in 0..2000 {
        props.push_str(&format!("<HDRGainMap:P{i}>{value}</HDRGainMap:P{i}>"));
    }
    let xml = xmp_doc(APPLE_DECL, "", &props);
    assert!(xml.len() > 2 << 20 && xml.len() < 4 << 20, "{}", xml.len());
    xml
}

// A 3 MiB XMP item: the smallest cap that lets `read` succeed is above the packet size, one cap
// just below it gives an allocation error, and at every cap the measured peak stays within
// max_alloc plus a slack that is under a tenth of the packet.
#[test]
fn read_peak_with_a_big_xmp_item() {
    let _g = lock();
    let data = heic_big_xmp();
    let run = |cap: usize| {
        let lim = limits_of(cap, 1);
        measured(|| gainmap::read(&data, &lim, &no_cancel()).map(|_| ()))
    };
    let (below, need) = min_alloc(|cap| run(cap).0.is_ok());
    assert!(need >= 3 << 20, "the packet is not charged: {need}");
    for cap in [
        below,
        need,
        need / 2,
        1 << 20,
        2 * need,
        Limits::default().max_alloc,
    ] {
        let (r, peak) = run(cap);
        let at = format!("big xmp max_alloc {cap}");
        match r {
            Ok(()) => assert!(cap >= need, "{at}: succeeded below {need}"),
            Err(e) => {
                assert!(is_alloc_error(&e), "{at}: {e}");
                assert!(cap < need, "{at}: failed at or above {need}");
            }
        }
        within(&at, peak, cap, READ_SLACK);
    }
    // At a generous cap the call really does hold the packet: the gate sees a peak well above
    // the slack, so a ledger that charged nothing could not pass.
    let (r, peak) = run(Limits::default().max_alloc);
    assert_eq!(r, Ok(()));
    assert!(
        peak >= 3 << 20,
        "peak {peak} is below the size of the packet"
    );
}

// A large parsed graph (40 overlays of 8000 inputs) and a multi-extent XMP item that `read`
// copies: at every cap the measured peak stays within max_alloc plus the slack, and a cap below
// what the call needs gives an allocation error. The graph and the copy are held together, so
// the peak at a generous cap is well above the packet alone.
#[test]
fn read_peak_with_a_big_graph_and_split_xmp() {
    let _g = lock();
    let data = heic_big_graph();
    let run = |cap: usize| {
        let lim = limits_of(cap, 1);
        measured(|| gainmap::read(&data, &lim, &no_cancel()).map(|_| ()))
    };
    let (below, need) = min_alloc(|cap| run(cap).0.is_ok());
    assert!(need >= 3 << 20, "the packet is not charged: {need}");
    for cap in [
        below,
        need,
        need / 2,
        1 << 20,
        2 * need,
        Limits::default().max_alloc,
    ] {
        let (r, peak) = run(cap);
        let at = format!("big graph max_alloc {cap}");
        match r {
            Ok(()) => assert!(cap >= need, "{at}: succeeded below {need}"),
            Err(e) => {
                assert!(is_alloc_error(&e), "{at}: {e}");
                assert!(cap < need, "{at}: failed at or above {need}");
            }
        }
        within(&at, peak, cap, READ_SLACK);
    }
    // The overlay inputs (40 x 8000 x 12 bytes) and the copied packet (over 2 MiB) are both live
    // at the peak, so a call that charged or held only one of them would sit below this.
    let (r, peak) = run(Limits::default().max_alloc);
    assert_eq!(r, Ok(()));
    assert!(peak >= 5 << 20, "peak {peak} is below graph plus packet");
}

// Decode of two big images: the base (9 MiB of RGBA) stays alive while the gain map decodes. At
// each thread count the smallest cap that works is found, one just below it gives an allocation
// error, and the peak stays within max_alloc plus the slack at every cap; an Ok result is the
// same for all of them.
#[test]
fn decode_peak_within_max_alloc() {
    let _g = lock();
    let data = ultra_hdr(BIG_BASE, BIG_GM);
    let img = gainmap::read(&data, &limits_of(1 << 30, 1), &no_cancel()).unwrap();
    let want = gainmap::decode(&data, &img, &limits_of(1 << 30, 1), &no_cancel()).unwrap();
    let base_bytes = BIG_BASE * BIG_BASE * 4;
    assert_eq!(want.base.data.len(), base_bytes);
    for t in [1, 2] {
        let run = |cap: usize| {
            let lim = limits_of(cap, t);
            measured(|| gainmap::decode(&data, &img, &lim, &no_cancel()))
        };
        let (below, need) = min_alloc(|cap| run(cap).0.is_ok());
        assert!(
            need >= base_bytes,
            "threads {t}: the base is not charged: {need}"
        );
        for cap in [
            below,
            need,
            need / 2,
            256 << 10,
            2 * need,
            Limits::default().max_alloc,
        ] {
            let at = format!("threads {t} max_alloc {cap}");
            let (r, peak) = run(cap);
            match &r {
                Ok(d) => {
                    assert_eq!(d, &want, "{at}");
                    assert!(cap >= need, "{at}: succeeded below {need}");
                }
                Err(e) => {
                    assert!(is_alloc_error(e), "{at}: {e}");
                    assert!(cap < need, "{at}: failed at or above {need}");
                }
            }
            within(&at, peak, cap, DECODE_SLACK);
        }
        // At a generous cap the call holds both images, so the peak is far above the slack.
        let (r, peak) = run(Limits::default().max_alloc);
        assert!(r.is_ok());
        assert!(
            peak >= base_bytes,
            "threads {t}: peak {peak} is below the base"
        );
    }
}

/// The number of threads that allocated during `f`, as the largest of five runs: a run that
/// breaks the limit must fail the test even if another run happens to see fewer threads. Every
/// test of this file holds `lock()`, so the other test threads are idle, and a first call before
/// the runs starts the pool threads.
fn threads_used(f: impl Fn()) -> u32 {
    f();
    (0..5)
        .map(|_| {
            SEEN.store(0, Ordering::SeqCst);
            TRACK.store(true, Ordering::SeqCst);
            f();
            TRACK.store(false, Ordering::SeqCst);
            SEEN.load(Ordering::SeqCst).count_ones()
        })
        .max()
        .unwrap_or(0)
}

// `gainmap::decode` itself, on an image big enough that jpeg-rs threads it: max_threads 1 runs on
// the calling thread alone, and 2 uses at most two threads (the caller and one more), even though
// the shared jpeg pool has a thread for every core. Threads are counted by the allocations they
// make, so a thread that works without allocating would not be seen; jpeg's own tests count the
// workers of a call exactly, and the check at the top shows that this image is cut for them.
#[test]
fn decode_threads_stay_under_max_threads() {
    let _g = lock();
    let data = ultra_hdr(BIG_BASE, BIG_GM);
    let img = gainmap::read(&data, &limits_of(1 << 30, 1), &no_cancel()).unwrap();
    // jpeg really runs this base on more than one worker in its RGBA colour pass, the pass that
    // gainmap::decode uses (a pool task that wakes after the
    // caller took every job is not counted, so it may take a few tries).
    let base = img
        .coded_bytes(&data, &img.base, &limits(), &no_cancel())
        .unwrap();
    let jl = jpeg::Limits {
        max_threads: 2,
        ..jpeg::Limits::default()
    };
    let peak = (0..30)
        .map(|_| {
            jpeg::decode_probe(
                &base,
                &jpeg::DecodeOptions {
                    format: jpeg::Format::Rgba,
                    ..jpeg::DecodeOptions::default()
                },
                &jl,
                &no_cancel(),
                false,
            )
            .1
            .threads_peak
        })
        .max()
        .unwrap_or(0);
    // Under qemu-user (the arm64 job) jpeg-rs may run this image on one worker, so the check
    // that the image is big enough to thread holds on x86_64 only; the bounds below hold anywhere.
    if cfg!(target_arch = "x86_64") {
        assert!(
            peak > 1,
            "the image is too small for jpeg to thread: {peak}"
        );
    }

    for t in [1usize, 2] {
        let lim = limits_of(1 << 30, t);
        let used = threads_used(|| {
            assert!(gainmap::decode(&data, &img, &lim, &no_cancel()).is_ok());
        });
        assert!(
            used >= 1 && used as usize <= t,
            "max_threads {t}: {used} threads"
        );
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
    // Holds the lock so its allocations cannot land in the thread count of another test.
    let _g = lock();
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
