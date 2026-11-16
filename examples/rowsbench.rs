//! `rowsbench <file.obu> <whole|rows|rows-nolf|whole-nolf> <helpers>`: decodes the one picture of
//! an OBU stream once, whole with `helpers` workers on a pool of the caller's threads or by rows
//! with `helpers` threads for the rows spawner, and prints its wall and CPU time, the peak RSS and
//! a hash of the picture's rows. `-nolf` turns the in-loop filters off; `-read` reads every row
//! once it is final, as an application converting them would.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Instant;

use fast_avif::{
    Decoder, InloopFilterType, Picture, PixelLayout, PlanarImageComponent, Rav1dError, Settings,
    Spawner,
};

type Work = Box<dyn FnOnce() + Send>;

/// `n` threads that take work while one is idle, like an application's helpers.
fn pool(n: usize) -> Spawner {
    let queue = Arc::new((Mutex::new(VecDeque::<Work>::new()), Condvar::new()));
    let running = Arc::new(AtomicUsize::new(0));
    for _ in 0..n {
        let queue = Arc::clone(&queue);
        let running = Arc::clone(&running);
        std::thread::spawn(move || loop {
            let work = {
                let mut q = queue.0.lock().unwrap();
                loop {
                    if let Some(w) = q.pop_front() {
                        break w;
                    }
                    q = queue.1.wait(q).unwrap();
                }
            };
            work();
            running.fetch_sub(1, Ordering::AcqRel);
        });
    }
    Arc::new(move |work: Work| {
        let mut now = running.load(Ordering::Acquire);
        loop {
            if now >= n {
                return false;
            }
            match running.compare_exchange_weak(now, now + 1, Ordering::AcqRel, Ordering::Acquire) {
                Ok(_) => break,
                Err(m) => now = m,
            }
        }
        queue.0.lock().unwrap().push_back(work);
        queue.1.notify_one();
        true
    })
}

fn cpu_ms() -> f64 {
    let stat = std::fs::read_to_string("/proc/self/stat").unwrap_or_default();
    let rest = stat.rsplit(')').next().unwrap_or("");
    let f: Vec<&str> = rest.split_whitespace().collect();
    let ticks: f64 = f.get(11).and_then(|u| u.parse().ok()).unwrap_or(0.0)
        + f.get(12).and_then(|s| s.parse().ok()).unwrap_or(0.0);
    ticks * 10.0
}

fn hwm_kb() -> u64 {
    std::fs::read_to_string("/proc/self/status")
        .unwrap_or_default()
        .lines()
        .find_map(|l| l.strip_prefix("VmHWM:"))
        .and_then(|v| v.trim().trim_end_matches(" kB").trim().parse().ok())
        .unwrap_or(0)
}

fn hash(p: &Picture) -> u64 {
    let mut h = 0xcbf29ce484222325u64;
    let (w, ht) = (p.width() as usize, p.height() as usize);
    let sample = if p.bit_depth() > 8 { 2 } else { 1 };
    for (i, c) in [
        PlanarImageComponent::Y,
        PlanarImageComponent::U,
        PlanarImageComponent::V,
    ]
    .into_iter()
    .enumerate()
    {
        if i > 0 && p.pixel_layout() == PixelLayout::I400 {
            break;
        }
        let (ss_hor, ss_ver) = match (i, p.pixel_layout()) {
            (0, _) => (0, 0),
            (_, PixelLayout::I420) => (1, 1),
            (_, PixelLayout::I422) => (1, 0),
            _ => (0, 0),
        };
        let (pw, ph) = ((w + ss_hor) >> ss_hor, (ht + ss_ver) >> ss_ver);
        let stride = p.stride(c) as usize;
        let base = p.plane_ptr(c);
        for y in 0..ph {
            let row = unsafe { std::slice::from_raw_parts(base.add(y * stride), pw * sample) };
            for &b in row {
                h = (h ^ b as u64).wrapping_mul(0x100000001b3);
            }
        }
    }
    h
}

/// Hashes the bytes of rows `from..to` of each plane into `sum`, about a nanosecond a byte.
fn read(p: &Picture, from: usize, to: usize, mut sum: u64) -> u64 {
    let sample = if p.bit_depth() > 8 { 2 } else { 1 };
    for (i, c) in [
        PlanarImageComponent::Y,
        PlanarImageComponent::U,
        PlanarImageComponent::V,
    ]
    .into_iter()
    .enumerate()
    {
        if i > 0 && p.pixel_layout() == PixelLayout::I400 {
            break;
        }
        let s = (i > 0 && p.pixel_layout() == PixelLayout::I420) as usize;
        let w = ((p.width() as usize + s) >> s) * sample;
        let stride = p.stride(c) as usize;
        let base = p.plane_ptr(c);
        for y in (from + s) >> s..(to + s) >> s {
            let row = unsafe { std::slice::from_raw_parts(base.add(y * stride), w) };
            for &b in row {
                sum = (sum ^ b as u64).wrapping_mul(0x100000001b3);
            }
        }
    }
    sum
}

fn main() -> Result<(), Rav1dError> {
    let args: Vec<String> = std::env::args().collect();
    let data = std::fs::read(&args[1]).expect("input");
    let mode = args[2].as_str();
    let reads = mode.ends_with("-read");
    let helpers: usize = args[3].parse().expect("helpers");
    let whole = mode.starts_with("whole");
    let mut s = Settings::new();
    s.set_max_frame_delay(1);
    s.set_n_threads(if whole { 1 + helpers as u32 } else { 1 });
    if mode.ends_with("-nolf") {
        s.set_inloop_filters(InloopFilterType::empty());
    }
    let spawner = pool(helpers);
    let start = Instant::now();
    let cpu0 = cpu_ms();
    let mut d = match whole && helpers > 0 {
        true => Decoder::with_spawner(&s, &spawner)?,
        false => Decoder::with_settings(&s)?,
    };
    if !whole {
        d.set_decode_by_rows(true);
        if helpers > 0 {
            d.set_rows_spawner(Some(spawner));
        }
    }
    let mut calls = 0;
    match d.send_data(data.into_boxed_slice(), None, None, None) {
        Ok(()) | Err(Rav1dError::TryAgain) => {}
        Err(e) => return Err(e),
    }
    let mut sum = 0;
    if !whole {
        let rows = d.rows_picture()?;
        let h = rows.height();
        let mut done = 0;
        loop {
            let n = d.decode_rows()?;
            if reads {
                sum = read(&rows, done as usize, n as usize, sum);
            }
            done = n;
            calls += 1;
            if n == h {
                break;
            }
        }
    }
    let p = d.get_picture()?;
    if whole && reads {
        sum = read(&p, 0, p.height() as usize, sum);
    }
    let wall = start.elapsed().as_secs_f64() * 1e3;
    let cpu = cpu_ms() - cpu0;
    println!(
        "mode={mode} h={helpers} wall_ms={wall:.1} cpu_ms={cpu:.0} hwm_kb={} {}x{} calls={calls} read={sum} hash={:016x}",
        hwm_kb(),
        p.width(),
        p.height(),
        hash(&p)
    );
    Ok(())
}
