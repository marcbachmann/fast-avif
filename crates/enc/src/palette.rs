//! Palette blocks: candidate palettes, colour index maps and their syntax, as `read_pal_plane`,
//! `read_pal_uv` and `read_pal_indices` read them.

use std::cmp;

use crate::rd::Sink;

/// The colours above and left that a block's palette may reuse, sorted and without repeats, as
/// the decoder merges them.
pub fn cache(above: &[u8], left: &[u8]) -> ([u8; 16], usize) {
    let mut out = [0u8; 16];
    let mut n = 0;
    let (mut a, mut l) = (above, left);
    let mut push = |v: u8, out: &mut [u8; 16]| {
        if n == 0 || out[n - 1] != v {
            out[n] = v;
            n += 1;
        }
    };
    while !l.is_empty() && !a.is_empty() {
        if l[0] < a[0] {
            push(l[0], &mut out);
            l = &l[1..];
        } else {
            if a[0] == l[0] {
                l = &l[1..];
            }
            push(a[0], &mut out);
            a = &a[1..];
        }
    }
    for &v in l.iter().chain(a) {
        push(v, &mut out);
    }
    (out, n)
}

fn bits_for(v: u32) -> u8 {
    (32 - v.leading_zeros()) as u8
}

/// The colours of a luma or U palette, sorted: which cache entries it reuses, then the rest as
/// deltas from the one before.
pub fn write_colors<S: Sink>(s: &mut S, pal: &[u8], cache: &[u8], pl: bool) {
    let not_pl = !pl as u32;
    let mut used = 0;
    let mut reused = [false; 8];
    for &c in cache {
        if used == pal.len() {
            break;
        }
        let hit = pal.iter().position(|&p| p == c).filter(|&i| !reused[i]);
        s.bool_equi(hit.is_some());
        if let Some(i) = hit {
            reused[i] = true;
            used += 1;
        }
    }
    let mut new = [0u32; 8];
    let mut n = 0;
    for (&c, _) in pal.iter().zip(reused).filter(|(_, r)| !r) {
        new[n] = c as u32;
        n += 1;
    }
    let new = &new[..n];
    let Some(&first) = new.first() else { return };
    s.bools(first, 8);
    if new.len() == 1 {
        return;
    }
    let max_delta = new.windows(2).map(|w| w[1] - w[0] - not_pl).max().unwrap();
    let min_bits = 8 - 3;
    let mut bits = cmp::max(bits_for(max_delta), min_bits);
    s.bools((bits - min_bits) as u32, 2);
    let mut prev = first;
    for &c in &new[1..] {
        s.bools(c - prev - not_pl, bits);
        prev = c;
        if prev + not_pl >= 255 {
            break;
        }
        bits = cmp::min(bits, bits_for(255 - prev - not_pl));
    }
}

/// The V colours of a chroma palette, in the order of the U colours: raw, or as signed deltas
/// when that is shorter.
pub fn write_v<S: Sink>(s: &mut S, v: &[u8]) {
    let mut deltas = [0i32; 7];
    let deltas = &mut deltas[..v.len().saturating_sub(1)];
    for (d, w) in deltas.iter_mut().zip(v.windows(2)) {
        let e = w[1] as i32 - w[0] as i32;
        *d = if e > 128 {
            e - 256
        } else if e < -128 {
            e + 256
        } else {
            e
        };
    }
    let min_bits = 8 - 4;
    let bits = cmp::max(
        bits_for(deltas.iter().map(|d| d.unsigned_abs()).max().unwrap_or(0)),
        min_bits,
    );
    let delta_bits = 2
        + 8
        + deltas
            .iter()
            .map(|&d| bits as usize + (d != 0) as usize)
            .sum::<usize>();
    if bits <= min_bits + 3 && delta_bits < 8 * v.len() {
        s.bool_equi(true);
        s.bools((bits - min_bits) as u32, 2);
        s.bools(v[0] as u32, 8);
        for &d in deltas.iter() {
            s.bools(d.unsigned_abs(), bits);
            if d != 0 {
                s.bool_equi(d < 0);
            }
        }
    } else {
        s.bool_equi(false);
        for &c in v {
            s.bools(c as u32, 8);
        }
    }
}

/// A block's colour indexes, `w`×`h` visible of a map `stride` wide, in wavefront order.
pub fn write_indices<S: Sink>(
    s: &mut S,
    cdf: &mut [[u16; 8]; 5],
    map: &[u8],
    stride: usize,
    w: usize,
    h: usize,
    n: u8,
) {
    s.uniform(map[0] as u32, n as u32);
    for i in 1..w + h - 1 {
        let first = cmp::min(i, w - 1);
        let last = (i + 1).saturating_sub(h);
        for j in (last..=first).rev() {
            let (sym, ctx) = symbol(map, stride, (i - j) * stride + j, j > 0, i > j);
            s.symbol(sym, &mut cdf[ctx], n - 1);
        }
    }
}

/// The symbol and context of the index at `at`: its place in rav1d's `order_palette` order,
/// the distinct neighbours first and then the other colours ascending.
fn symbol(map: &[u8], stride: usize, at: usize, left: bool, top: bool) -> (u8, usize) {
    let v = map[at];
    let (ctx, near, k) = if !left {
        (0, [map[at - stride], 0, 0], 1)
    } else if !top {
        (0, [map[at - 1], 0, 0], 1)
    } else {
        let (l, t, tl) = (map[at - 1], map[at - stride], map[at - stride - 1]);
        if t == l && t == tl {
            (4, [t, 0, 0], 1)
        } else if t == l {
            (3, [t, tl, 0], 2)
        } else if t == tl || l == tl {
            (2, [tl, if t == tl { l } else { t }, 0], 2)
        } else {
            (1, [cmp::min(t, l), cmp::max(t, l), tl], 3)
        }
    };
    if let Some(m) = near[..k].iter().position(|&c| c == v) {
        return (m as u8, ctx);
    }
    let below = near[..k].iter().filter(|&&c| c < v).count();
    ((k + v as usize - below) as u8, ctx)
}

/// The index of the nearest colour, the first of equals.
pub fn nearest(pal: &[u8], v: u8) -> u8 {
    let mut best = (u32::MAX, 0);
    for (i, &p) in pal.iter().enumerate() {
        let d = (p as i32 - v as i32).unsigned_abs();
        if d < best.0 {
            best = (d, i as u8);
        }
    }
    best.1
}

pub fn nearest2(u: &[u8], v: &[u8], (pu, pv): (u8, u8)) -> u8 {
    let mut best = (u32::MAX, 0);
    for i in 0..u.len() {
        let du = u[i] as i32 - pu as i32;
        let dv = v[i] as i32 - pv as i32;
        let d = (du * du + dv * dv) as u32;
        if d < best.0 {
            best = (d, i as u8);
        }
    }
    best.1
}

/// Up to 8 palette colours.
#[derive(Clone, Copy, Default)]
pub struct Colours {
    len: u8,
    c: [u8; 8],
}

impl Colours {
    fn of(c: impl IntoIterator<Item = u8>) -> Self {
        let mut out = Self::default();
        for (d, c) in out.c.iter_mut().zip(c) {
            *d = c;
            out.len += 1;
        }
        out
    }
}

impl std::ops::Deref for Colours {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        &self.c[..self.len as usize]
    }
}

/// Luma palettes worth trying for these pixels: their colours when there are 2 to 8, else
/// k-means palettes of 8 and 4 colours when there are no more than 64.
pub fn candidates_y(px: &[u8]) -> impl Iterator<Item = Colours> {
    let mut hist = [0u32; 256];
    for &p in px {
        hist[p as usize] += 1;
    }
    let mut all = [0u8; 256];
    let mut n = 0;
    for c in (0..=255u8).filter(|&c| hist[c as usize] > 0) {
        all[n] = c;
        n += 1;
    }
    let colors = &all[..n];
    let mut out = [Colours::default(); 2];
    let n = match colors.len() {
        2..=8 => {
            out[0] = Colours::of(colors.iter().copied());
            1
        }
        9..=64 => {
            out = [8, 4].map(|k| kmeans1(&hist, colors, k));
            2
        }
        _ => 0,
    };
    out.into_iter().take(n)
}

/// The first of the smallest distances, as `min_by` with `total_cmp` picks it.
fn nearest_of(d: impl Iterator<Item = f64>) -> usize {
    let mut best = (f64::INFINITY, 0);
    for (i, d) in d.enumerate() {
        if i == 0 || d.total_cmp(&best.0).is_lt() {
            best = (d, i);
        }
    }
    best.1
}

fn kmeans1(hist: &[u32; 256], colors: &[u8], k: usize) -> Colours {
    let mut by_count = [0u8; 64];
    let by_count = &mut by_count[..colors.len()];
    by_count.copy_from_slice(colors);
    by_count.sort_by_key(|&c| cmp::Reverse(hist[c as usize]));
    let mut centres = [0f64; 8];
    let centres = &mut centres[..k];
    for (m, &c) in centres.iter_mut().zip(&by_count[..k]) {
        *m = c as f64;
    }
    for _ in 0..8 {
        let mut sum = [(0f64, 0f64); 8];
        for &c in colors {
            let i = nearest_of(centres.iter().map(|&m| (m - c as f64).abs()));
            let w = hist[c as usize] as f64;
            sum[i].0 += w * c as f64;
            sum[i].1 += w;
        }
        for (c, s) in centres.iter_mut().zip(&sum) {
            if s.1 > 0.0 {
                *c = s.0 / s.1;
            }
        }
    }
    let mut out = [0u8; 8];
    let out = &mut out[..k];
    for (o, c) in out.iter_mut().zip(centres.iter()) {
        *o = c.round() as u8;
    }
    out.sort_unstable();
    Colours::of(dedup(out))
}

fn dedup<T: Copy + PartialEq>(sorted: &[T]) -> impl Iterator<Item = T> + '_ {
    sorted
        .iter()
        .enumerate()
        .filter(|&(i, c)| i == 0 || sorted[i - 1] != *c)
        .map(|(_, &c)| c)
}

/// Chroma palettes (U colours sorted, V alongside) worth trying for these pixel pairs. A block
/// with more than 64 pairs has none, which is known after the 65th distinct pair.
pub fn candidates_uv(u: &[u8], v: &[u8]) -> impl Iterator<Item = (Colours, Colours)> {
    let mut out = [(Colours::default(), Colours::default()); 2];
    let mut seen = [0u64; 1024];
    let mut keys = [0u16; 64];
    let mut n = 0;
    for (&a, &b) in u.iter().zip(v) {
        let k = (a as usize) << 8 | b as usize;
        if seen[k >> 6] & 1 << (k & 63) == 0 {
            seen[k >> 6] |= 1 << (k & 63);
            if n == 64 {
                return out.into_iter().take(0);
            }
            keys[n] = k as u16;
            n += 1;
        }
    }
    let keys = &mut keys[..n];
    keys.sort_unstable();
    let mut counts = [((0u8, 0u8), 0u32); 64];
    let counts = &mut counts[..n];
    for (c, &k) in counts.iter_mut().zip(keys.iter()) {
        c.0 = ((k >> 8) as u8, k as u8);
    }
    for (&a, &b) in u.iter().zip(v) {
        let k = (a as u16) << 8 | b as u16;
        counts[keys.binary_search(&k).unwrap()].1 += 1;
    }
    let split = |c: &[(u8, u8)]| {
        (
            Colours::of(c.iter().map(|p| p.0)),
            Colours::of(c.iter().map(|p| p.1)),
        )
    };
    let n = match counts.len() {
        0 | 1 => 0,
        2..=8 => {
            let mut pairs = [(0u8, 0u8); 8];
            for (p, c) in pairs.iter_mut().zip(counts.iter()) {
                *p = c.0;
            }
            out[0] = split(&pairs[..counts.len()]);
            1
        }
        _ => {
            out = [8, 4].map(|k| {
                let (pairs, n) = kmeans2(counts, k);
                split(&pairs[..n])
            });
            2
        }
    };
    out.into_iter().take(n)
}

fn kmeans2(counts: &[((u8, u8), u32)], k: usize) -> ([(u8, u8); 8], usize) {
    let mut by_count = [((0u8, 0u8), 0u32); 64];
    let by_count = &mut by_count[..counts.len()];
    by_count.copy_from_slice(counts);
    by_count.sort_by_key(|c| cmp::Reverse(c.1));
    let mut centres = [(0f64, 0f64); 8];
    let centres = &mut centres[..k];
    for (m, c) in centres.iter_mut().zip(&by_count[..k]) {
        *m = (c.0 .0 as f64, c.0 .1 as f64);
    }
    let dist = |a: (f64, f64), b: (u8, u8)| (a.0 - b.0 as f64).powi(2) + (a.1 - b.1 as f64).powi(2);
    for _ in 0..8 {
        let mut sum = [(0f64, 0f64, 0f64); 8];
        for &(p, n) in counts {
            let i = nearest_of(centres.iter().map(|&m| dist(m, p)));
            let w = n as f64;
            sum[i].0 += w * p.0 as f64;
            sum[i].1 += w * p.1 as f64;
            sum[i].2 += w;
        }
        for (c, s) in centres.iter_mut().zip(&sum) {
            if s.2 > 0.0 {
                *c = (s.0 / s.2, s.1 / s.2);
            }
        }
    }
    let mut out = [(0u8, 0u8); 8];
    let rounded = &mut out[..k];
    for (o, c) in rounded.iter_mut().zip(centres.iter()) {
        *o = (c.0.round() as u8, c.1.round() as u8);
    }
    rounded.sort_unstable();
    let mut deduped = [(0u8, 0u8); 8];
    let mut n = 0;
    for (d, c) in deduped.iter_mut().zip(dedup(rounded)) {
        *d = c;
        n += 1;
    }
    (deduped, n)
}

#[cfg(test)]
mod tests {
    use fast_avif_dec::decode::order_palette;

    use super::*;
    use crate::frame::tests::Rng;

    /// The symbols and contexts are those of rav1d's `order_palette` in wavefront order.
    #[test]
    fn symbols_as_rav1d_orders() {
        let mut rng = Rng(0x5eed_9a1e);
        for _ in 0..3000 {
            let (w, h) = (
                cmp::min(4usize << rng.below(5), 64),
                cmp::min(4usize << rng.below(5), 64),
            );
            let n = 2 + rng.below(7) as u8;
            let spread = 1 + rng.below(n as u32);
            let map: Vec<u8> = (0..w * h).map(|_| rng.below(spread) as u8).collect();
            let mut order = [[0u8; 8]; 64];
            let mut ctx = [0u8; 64];
            for i in 1..w + h - 1 {
                let first = cmp::min(i, w - 1);
                let last = (i + 1).saturating_sub(h);
                order_palette(&map, w, i, first, last, &mut order, &mut ctx);
                for (m, j) in (last..=first).rev().enumerate() {
                    let idx = map[(i - j) * w + j];
                    let want = (
                        order[m].iter().position(|&o| o == idx).unwrap() as u8,
                        ctx[m] as usize,
                    );
                    assert_eq!(
                        want,
                        symbol(&map, w, (i - j) * w + j, j > 0, i > j),
                        "{w}x{h} n {n} at {i},{j}"
                    );
                }
            }
        }
    }
}
