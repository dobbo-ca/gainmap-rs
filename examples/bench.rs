//! `bench <dir>`: reads every file of `<dir>` (sorted) into memory, then times `gainmap::read`
//! and `gainmap::decode` (the base image and the gain map) over all of them, best of three
//! rounds, with one thread and with the default thread count. Prints a markdown table:
//! files, MB, files decoded, read ms, decode ms with 1 thread, decode ms with the default
//! threads, the thread count. tests/parity/bench.sh runs it.

use std::hint::black_box;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::atomic::AtomicBool;
use std::time::Instant;

use gainmap::{Codec, GainMapImage, Limits};

/// Best of three rounds of `f`, in milliseconds.
fn best_of_three(mut f: impl FnMut()) -> f64 {
    let mut best = f64::MAX;
    for _ in 0..3 {
        let t = Instant::now();
        f();
        best = best.min(t.elapsed().as_secs_f64() * 1e3);
    }
    best
}

fn main() -> ExitCode {
    let Some(dir) = std::env::args_os().nth(1) else {
        eprintln!("usage: bench <dir>");
        return ExitCode::from(2);
    };
    let mut paths: Vec<PathBuf> = match std::fs::read_dir(&dir) {
        Ok(rd) => rd
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.is_file())
            .collect(),
        Err(e) => {
            eprintln!("bench: {e}");
            return ExitCode::from(2);
        }
    };
    paths.sort();
    let files: Vec<Vec<u8>> = paths.iter().filter_map(|p| std::fs::read(p).ok()).collect();
    let bytes: usize = files.iter().map(Vec::len).sum();

    let cancel = AtomicBool::new(false);
    let threads = Limits::default().max_threads;
    let one = Limits {
        max_threads: 1,
        ..Limits::default()
    };
    let many = Limits::default();

    let read_ms = best_of_three(|| {
        for f in &files {
            let _ = black_box(gainmap::read(black_box(f.as_slice()), &one, &cancel));
        }
    });

    // The files that read with a gain map. Only the JPEGs are timed: the libultrahdr side of the
    // comparison decodes JPEGs only.
    let parsed: Vec<(&Vec<u8>, GainMapImage)> = files
        .iter()
        .filter_map(|f| {
            let image = gainmap::read(f, &one, &cancel).ok()?;
            image.gain_map.is_some().then_some((f, image))
        })
        .collect();
    let decodable: Vec<&(&Vec<u8>, GainMapImage)> = parsed
        .iter()
        .filter(|(f, image)| {
            image.base.codec == Codec::Jpeg && gainmap::decode(f, image, &one, &cancel).is_ok()
        })
        .collect();
    let decode = |limits: &Limits| {
        best_of_three(|| {
            for (f, image) in decodable.iter().copied() {
                let _ = black_box(gainmap::decode(
                    black_box(f.as_slice()),
                    image,
                    limits,
                    &cancel,
                ));
            }
        })
    };
    let one_ms = decode(&one);
    let many_ms = decode(&many);
    eprintln!(
        "bench: {} files read, {} with a gain map, {} decoded",
        files.len(),
        parsed.len(),
        decodable.len()
    );

    println!(
        "| files | MB | decoded | read ms | decode ms (1 thread) | decode ms (N threads) | N |"
    );
    println!("|---|---|---|---|---|---|---|");
    println!(
        "| {} | {:.1} | {} | {read_ms:.1} | {one_ms:.1} | {many_ms:.1} | {threads} |",
        files.len(),
        bytes as f64 / 1e6,
        decodable.len()
    );
    ExitCode::SUCCESS
}
