//! Pictures decoded with every new allocation filled with one byte and then another: a picture that
//! changes read memory nothing wrote.

use std::alloc::{GlobalAlloc, Layout, System};
use std::hash::{DefaultHasher, Hasher};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU8, Ordering};

use rav1d::{Decoder, Picture, PixelLayout, PlanarImageComponent, Rav1dError, Settings};

static FILL: AtomicU8 = AtomicU8::new(0);

struct Filling;

unsafe impl GlobalAlloc for Filling {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let p = System.alloc(layout);
        if !p.is_null() {
            p.write_bytes(FILL.load(Ordering::Relaxed), layout.size());
        }
        p
    }

    unsafe fn dealloc(&self, p: *mut u8, layout: Layout) {
        System.dealloc(p, layout)
    }

    unsafe fn realloc(&self, p: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        let q = System.realloc(p, layout, size);
        if !q.is_null() && size > layout.size() {
            q.add(layout.size())
                .write_bytes(FILL.load(Ordering::Relaxed), size - layout.size());
        }
        q
    }
}

#[global_allocator]
static GLOBAL: Filling = Filling;

fn ivf_frames(data: &[u8]) -> Vec<&[u8]> {
    let mut frames = Vec::new();
    if data.len() < 32 || &data[..4] != b"DKIF" {
        return frames;
    }
    let mut at = u16::from_le_bytes([data[6], data[7]]) as usize;
    while at + 12 <= data.len() {
        let size = u32::from_le_bytes(data[at..at + 4].try_into().unwrap()) as usize;
        at += 12;
        if size == 0 || at + size > data.len() {
            break;
        }
        frames.push(&data[at..at + size]);
        at += size;
    }
    frames
}

fn digest(p: &Picture) -> u64 {
    let mut h = DefaultHasher::new();
    let (w, hgt) = (p.width() as usize, p.height() as usize);
    let sample = if p.bit_depth() > 8 { 2 } else { 1 };
    for (i, c) in [
        PlanarImageComponent::Y,
        PlanarImageComponent::U,
        PlanarImageComponent::V,
    ]
    .into_iter()
    .enumerate()
    {
        let (sx, sy) = match p.pixel_layout() {
            PixelLayout::I400 if i > 0 => continue,
            PixelLayout::I420 if i > 0 => (1, 1),
            PixelLayout::I422 if i > 0 => (1, 0),
            _ => (0, 0),
        };
        let (stride, data) = (p.stride(c) as usize, p.plane(c));
        for y in 0..(hgt + sy) >> sy {
            h.write(&data[y * stride..][..((w + sx) >> sx) * sample]);
        }
    }
    h.finish()
}

fn decode(file: &Path, threads: u32, by_rows: bool) -> Vec<Result<u64, Rav1dError>> {
    let data = std::fs::read(file).unwrap();
    let mut out = Vec::new();
    let mut settings = Settings::new();
    settings.set_n_threads(threads);
    settings.set_max_frame_delay(1);
    let mut d = Decoder::with_settings(&settings).unwrap();
    d.set_decode_by_rows(by_rows);
    let drain = |d: &mut Decoder, out: &mut Vec<Result<u64, Rav1dError>>| loop {
        while d.decode_rows().is_ok() {}
        match d.get_picture() {
            Ok(p) => out.push(Ok(digest(&p))),
            Err(Rav1dError::TryAgain) => {
                if d.decode_rows().is_err() {
                    break;
                }
            }
            Err(e) => out.push(Err(e)),
        }
    };
    for frame in ivf_frames(&data) {
        let mut sent = d.send_data(frame.into(), None, None, None);
        for _ in 0..1000 {
            match sent {
                Ok(()) => break,
                Err(Rav1dError::TryAgain) => {}
                Err(e) => {
                    out.push(Err(e));
                    break;
                }
            }
            drain(&mut d, &mut out);
            sent = d.send_pending_data();
        }
    }
    drain(&mut d, &mut out);
    out
}

fn streams() -> Vec<PathBuf> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, out);
            } else if p.extension().is_some_and(|x| x == "ivf") {
                out.push(p);
            }
        }
    }
    let mut out = Vec::new();
    for bits in ["8-bit", "10-bit", "12-bit", "multi-bit", "oss-fuzz"] {
        walk(
            &Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/dav1d-test-data")
                .join(bits),
            &mut out,
        );
    }
    out.sort();
    out
}

/// One test, as the fill is the process's: one thread whole and by rows, and four threads. Six
/// minutes over the test data: `cargo test --test uninit -- --ignored`.
#[test]
#[ignore]
fn reads_no_memory_nothing_wrote() {
    let filter = std::env::var("ROWS_FILTER").unwrap_or_default();
    let mut differ = Vec::new();
    let mut pictures = 0;
    for file in streams()
        .iter()
        .filter(|f| f.to_string_lossy().contains(&filter))
    {
        for (threads, by_rows) in [(1, false), (1, true), (4, false)] {
            FILL.store(0, Ordering::Relaxed);
            let zero = decode(file, threads, by_rows);
            FILL.store(0xff, Ordering::Relaxed);
            let ones = decode(file, threads, by_rows);
            let upto = |d: &Vec<Result<u64, Rav1dError>>| {
                d.iter().position(|o| o.is_err()).unwrap_or(d.len())
            };
            let n = upto(&zero).min(upto(&ones));
            if zero[..n] != ones[..n] {
                differ.push(format!(
                    "{} threads {threads} by rows {by_rows}",
                    file.display()
                ));
            }
            pictures += n;
        }
    }
    eprintln!("{pictures} pictures, {} differ: {differ:#?}", differ.len());
    assert!(differ.is_empty());
}
