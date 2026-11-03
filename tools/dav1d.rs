//! `dav1d -i <file> [-o -|<path>] [--muxer md5|null] [--verify <md5>] [--threads N] [--framedelay N]
//! [--filmgrain 0|1] [--oppoint N] [--alllayers 0|1] [--sizelimit N] [--strict 0|1]
//! [--decodeframetype all|reference|intra|key] [--cpumask <flags>] [--limit N] [-q]`: the conformance runner on the Rust API. Reads IVF,
//! Section 5 and Annex B streams and hashes every picture's planes as dav1d's md5 muxer does, so
//! the md5s of `tests/dav1d-test-data` verify it.

use std::io::Write;
use std::process::ExitCode;

use rav1d::cpu::rav1d_set_cpu_flags_mask;
use rav1d::{DecodeFrameType, Decoder, PixelLayout, PlanarImageComponent, Rav1dError, Settings};

struct Args {
    input: String,
    output: Option<String>,
    muxer: Option<String>,
    verify: Option<String>,
    threads: u32,
    frame_delay: u32,
    film_grain: Option<bool>,
    operating_point: u8,
    all_layers: bool,
    size_limit: u32,
    strict: bool,
    frame_type: DecodeFrameType,
    cpu_mask: Option<String>,
    limit: usize,
    quiet: bool,
}

fn args() -> Result<Args, String> {
    let mut a = Args {
        input: String::new(),
        output: None,
        muxer: None,
        verify: None,
        threads: 0,
        frame_delay: 0,
        film_grain: None,
        operating_point: 0,
        all_layers: true,
        size_limit: 0,
        strict: true,
        frame_type: DecodeFrameType::All,
        cpu_mask: None,
        limit: 0,
        quiet: false,
    };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        let (k, inline) = match arg.split_once('=') {
            Some((k, v)) if k.starts_with("--") => (k.to_string(), Some(v.to_string())),
            _ => (arg.clone(), None),
        };
        let mut value = || inline.clone().or_else(|| it.next()).ok_or_else(|| format!("{k} needs a value"));
        match k.as_str() {
            "-i" | "--input" => a.input = value()?,
            "-o" | "--output" => a.output = Some(value()?),
            "--muxer" => a.muxer = Some(value()?),
            "--verify" => a.verify = Some(value()?),
            "--threads" => a.threads = value()?.parse().map_err(|e| format!("--threads: {e}"))?,
            "--filmgrain" => a.film_grain = Some(value()? != "0"),
            "--cpumask" => a.cpu_mask = Some(value()?),
            "--limit" => a.limit = value()?.parse().map_err(|e| format!("--limit: {e}"))?,
            "-q" | "--quiet" => a.quiet = true,
            "-v" | "--version" => {
                println!("{}", rav1d::rav1d_version());
                std::process::exit(0);
            }
            "--framedelay" => a.frame_delay = value()?.parse().map_err(|e| format!("--framedelay: {e}"))?,
            "--oppoint" => a.operating_point = value()?.parse().map_err(|e| format!("--oppoint: {e}"))?,
            "--alllayers" => a.all_layers = value()? != "0",
            "--sizelimit" => a.size_limit = value()?.parse().map_err(|e| format!("--sizelimit: {e}"))?,
            "--strict" => a.strict = value()? != "0",
            "--decodeframetype" => {
                a.frame_type = match value()?.as_str() {
                    "all" => DecodeFrameType::All,
                    "reference" => DecodeFrameType::Reference,
                    "intra" => DecodeFrameType::Intra,
                    "key" => DecodeFrameType::Key,
                    other => return Err(format!("--decodeframetype {other}: all, reference, intra or key")),
                }
            }
            "--demuxer" => {
                value()?;
            }
            _ => return Err(format!("unknown argument {k}")),
        }
    }
    if a.input.is_empty() {
        return Err("no input (-i)".into());
    }
    Ok(a)
}

/// A level name, or a number as `strtoul` reads it, so `-1` is every flag. x86's levels hold the
/// flags below them, as its kernels are chosen level by level.
fn cpu_mask(s: &str) -> Result<u32, String> {
    let named: &[(&str, u32)] = if cfg!(any(target_arch = "x86", target_arch = "x86_64")) {
        &[("sse2", 1), ("ssse3", 3), ("sse41", 7), ("avx2", 15), ("avx512icl", 31)]
    } else if cfg!(any(target_arch = "arm", target_arch = "aarch64")) {
        &[("neon", 1), ("dotprod", 2), ("i8mm", 4), ("sve", 8), ("sve2", 16)]
    } else if cfg!(any(target_arch = "riscv32", target_arch = "riscv64")) {
        &[("rvv", 1)]
    } else {
        &[]
    };
    if s == "none" {
        return Ok(0);
    }
    if let Some(&(_, mask)) = named.iter().find(|(name, _)| *name == s) {
        return Ok(mask);
    }
    if let Some(hex) = s.strip_prefix("0x") {
        return u32::from_str_radix(hex, 16).map_err(|e| format!("--cpumask: {e}"));
    }
    s.parse::<i64>().map(|n| n as u32).map_err(|e| format!("--cpumask {s}: {e}"))
}

/// Reads the leb128 at `at`, giving the value and the bytes it took.
fn leb128(data: &[u8], at: usize) -> Option<(usize, usize)> {
    let mut value = 0usize;
    for i in 0..8 {
        let b = *data.get(at + i)?;
        value |= ((b & 0x7f) as usize) << (7 * i);
        if b & 0x80 == 0 {
            return Some((value, i + 1));
        }
    }
    None
}

fn ivf_units(data: &[u8]) -> Vec<&[u8]> {
    let mut units = Vec::new();
    let mut at = u16::from_le_bytes([data[6], data[7]]) as usize;
    while at + 12 <= data.len() {
        let size = u32::from_le_bytes(data[at..at + 4].try_into().unwrap()) as usize;
        at += 12;
        if size == 0 || at + size > data.len() {
            break;
        }
        units.push(&data[at..at + size]);
        at += size;
    }
    units
}

/// Section 5: temporal units of OBUs with size fields, each starting at a temporal delimiter.
fn section5_units(data: &[u8]) -> Option<Vec<&[u8]>> {
    let mut units = Vec::new();
    let (mut at, mut start) = (0, 0);
    while at < data.len() {
        let header = data[at];
        let obu_type = header >> 3 & 0xf;
        if header & 0x2 == 0 {
            return None;
        }
        if obu_type == 2 && at > start {
            units.push(&data[start..at]);
            start = at;
        }
        let ext = (header & 0x4 != 0) as usize;
        let (len, took) = leb128(data, at + 1 + ext)?;
        at += 1 + ext + took + len;
    }
    if at != data.len() {
        return None;
    }
    units.push(&data[start..]);
    Some(units)
}

/// Annex B: temporal units of frame units of length-prefixed OBUs without size fields, one OBU
/// per unit sent.
fn annexb_units(data: &[u8]) -> Option<Vec<&[u8]>> {
    let mut units = Vec::new();
    let mut at = 0;
    while at < data.len() {
        let (tu, took) = leb128(data, at)?;
        at += took;
        let tu_end = at.checked_add(tu).filter(|&e| e <= data.len())?;
        while at < tu_end {
            let (fu, took) = leb128(data, at)?;
            at += took;
            let fu_end = at.checked_add(fu).filter(|&e| e <= tu_end)?;
            while at < fu_end {
                let (len, took) = leb128(data, at)?;
                at += took;
                let end = at.checked_add(len).filter(|&e| e <= fu_end)?;
                units.push(&data[at..end]);
                at = end;
            }
        }
    }
    Some(units)
}

struct Md5 {
    state: [u32; 4],
    block: [u8; 64],
    filled: usize,
    len: u64,
}

impl Md5 {
    const K: [u32; 64] = [
        0xd76aa478, 0xe8c7b756, 0x242070db, 0xc1bdceee, 0xf57c0faf, 0x4787c62a, 0xa8304613, 0xfd469501,
        0x698098d8, 0x8b44f7af, 0xffff5bb1, 0x895cd7be, 0x6b901122, 0xfd987193, 0xa679438e, 0x49b40821,
        0xf61e2562, 0xc040b340, 0x265e5a51, 0xe9b6c7aa, 0xd62f105d, 0x02441453, 0xd8a1e681, 0xe7d3fbc8,
        0x21e1cde6, 0xc33707d6, 0xf4d50d87, 0x455a14ed, 0xa9e3e905, 0xfcefa3f8, 0x676f02d9, 0x8d2a4c8a,
        0xfffa3942, 0x8771f681, 0x6d9d6122, 0xfde5380c, 0xa4beea44, 0x4bdecfa9, 0xf6bb4b60, 0xbebfbc70,
        0x289b7ec6, 0xeaa127fa, 0xd4ef3085, 0x04881d05, 0xd9d4d039, 0xe6db99e5, 0x1fa27cf8, 0xc4ac5665,
        0xf4292244, 0x432aff97, 0xab9423a7, 0xfc93a039, 0x655b59c3, 0x8f0ccc92, 0xffeff47d, 0x85845dd1,
        0x6fa87e4f, 0xfe2ce6e0, 0xa3014314, 0x4e0811a1, 0xf7537e82, 0xbd3af235, 0x2ad7d2bb, 0xeb86d391,
    ];
    const S: [u32; 64] = [
        7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20, 5, 9,
        14, 20, 5, 9, 14, 20, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 6, 10, 15,
        21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
    ];

    fn new() -> Self {
        Self { state: [0x67452301, 0xefcdab89, 0x98badcfe, 0x10325476], block: [0; 64], filled: 0, len: 0 }
    }

    fn body(&mut self) {
        let m: [u32; 16] = std::array::from_fn(|i| u32::from_le_bytes(self.block[4 * i..][..4].try_into().unwrap()));
        let [mut a, mut b, mut c, mut d] = self.state;
        for i in 0..64 {
            let (f, g) = match i / 16 {
                0 => ((b & c) | (!b & d), i),
                1 => ((d & b) | (!d & c), (5 * i + 1) % 16),
                2 => (b ^ c ^ d, (3 * i + 5) % 16),
                _ => (c ^ (b | !d), (7 * i) % 16),
            };
            let f = f.wrapping_add(a).wrapping_add(Self::K[i]).wrapping_add(m[g]);
            a = d;
            d = c;
            c = b;
            b = b.wrapping_add(f.rotate_left(Self::S[i]));
        }
        for (s, v) in self.state.iter_mut().zip([a, b, c, d]) {
            *s = s.wrapping_add(v);
        }
    }

    fn update(&mut self, mut data: &[u8]) {
        self.len += data.len() as u64;
        while !data.is_empty() {
            let n = (64 - self.filled).min(data.len());
            self.block[self.filled..self.filled + n].copy_from_slice(&data[..n]);
            self.filled += n;
            data = &data[n..];
            if self.filled == 64 {
                self.body();
                self.filled = 0;
            }
        }
    }

    fn finish(mut self) -> String {
        let bits = (self.len * 8).to_le_bytes();
        self.update(&[0x80]);
        while self.filled != 56 {
            self.update(&[0]);
        }
        self.update(&bits);
        self.state.iter().flat_map(|s| s.to_le_bytes()).map(|b| format!("{b:02x}")).collect()
    }
}

/// Every plane row of `p`, luma then chroma, as dav1d's md5 muxer hashes them.
fn hash_picture(md5: &mut Md5, p: &rav1d::Picture) {
    let (w, h) = (p.width() as usize, p.height() as usize);
    let sample = if p.bit_depth() > 8 { 2 } else { 1 };
    let (ss_hor, ss_ver) = match p.pixel_layout() {
        PixelLayout::I400 => return hash_plane(md5, p, PlanarImageComponent::Y, w * sample, h),
        PixelLayout::I420 => (1, 1),
        PixelLayout::I422 => (1, 0),
        PixelLayout::I444 => (0, 0),
    };
    hash_plane(md5, p, PlanarImageComponent::Y, w * sample, h);
    for c in [PlanarImageComponent::U, PlanarImageComponent::V] {
        hash_plane(md5, p, c, ((w + ss_hor) >> ss_hor) * sample, (h + ss_ver) >> ss_ver);
    }
}

fn hash_plane(md5: &mut Md5, p: &rav1d::Picture, c: PlanarImageComponent, bytes: usize, rows: usize) {
    let (stride, data) = (p.stride(c) as usize, p.plane(c));
    for y in 0..rows {
        md5.update(&data[y * stride..][..bytes]);
    }
}

fn run(a: &Args) -> Result<ExitCode, String> {
    if let Some(m) = a.muxer.as_deref().filter(|m| !matches!(*m, "md5" | "null")) {
        return Err(format!("--muxer {m}: this runner writes md5 or null"));
    }
    if let Some(o) = a.output.as_deref().filter(|o| a.muxer.is_none() && !(matches!(*o, "-" | "/dev/null") || o.ends_with(".md5"))) {
        return Err(format!("-o {o}: this runner writes an md5, name a .md5 file or -"));
    }
    if let Some(mask) = &a.cpu_mask {
        rav1d_set_cpu_flags_mask(cpu_mask(mask)?);
    }
    let data = std::fs::read(&a.input).map_err(|e| format!("{}: {e}", a.input))?;
    let units = if data.len() >= 32 && &data[..4] == b"DKIF" {
        ivf_units(&data)
    } else if data.len() >= 2 && data[0] == 0x12 && data[1] == 0 {
        section5_units(&data).ok_or("malformed Section 5 stream")?
    } else {
        annexb_units(&data).ok_or("unknown stream format")?
    };
    // As dav1d's CLI: -o /dev/null is the null muxer, any other output without a muxer the md5.
    let null = a.muxer.as_deref() == Some("null") || a.muxer.is_none() && a.output.as_deref() == Some("/dev/null");
    let hashing = a.verify.is_some() || !null && (a.muxer.as_deref() == Some("md5") || a.muxer.is_none() && a.output.is_some());
    let mut settings = Settings::new();
    settings.set_n_threads(a.threads);
    settings.set_max_frame_delay(a.frame_delay);
    settings.set_strict_std_compliance(a.strict);
    settings.set_operating_point(a.operating_point);
    settings.set_all_layers(a.all_layers);
    settings.set_frame_size_limit(a.size_limit);
    settings.set_decode_frame_type(a.frame_type);
    // As dav1d's CLI: an md5 is of the picture without grain unless --filmgrain says so.
    settings.set_apply_grain(a.film_grain.unwrap_or(!hashing));
    let mut decoder = Decoder::with_settings(&settings).map_err(|e| format!("{e:?}"))?;
    let mut md5 = Md5::new();
    let mut pictures = 0;
    // Every picture the decoder has; true once the limit is reached.
    let mut drain = |decoder: &mut Decoder| -> Result<bool, Rav1dError> {
        loop {
            match decoder.get_picture() {
                Ok(p) => {
                    if hashing {
                        hash_picture(&mut md5, &p);
                    }
                    pictures += 1;
                    if pictures == a.limit {
                        return Ok(true);
                    }
                }
                Err(Rav1dError::TryAgain) => return Ok(false),
                Err(e) => return Err(e),
            }
        }
    };
    let decode = || -> Result<(), Rav1dError> {
        for unit in units {
            let mut sent = decoder.send_data(unit.into(), None, None, None);
            loop {
                if drain(&mut decoder)? {
                    return Ok(());
                }
                match sent {
                    Err(Rav1dError::TryAgain) => sent = decoder.send_pending_data(),
                    Err(e) => return Err(e),
                    Ok(()) => break,
                }
            }
        }
        drain(&mut decoder).map(|_| ())
    };
    if let Err(e) = decode() {
        if !a.quiet {
            eprintln!("Error decoding frame: {e:?}");
        }
        return Ok(ExitCode::FAILURE);
    }
    if !hashing {
        return Ok(ExitCode::SUCCESS);
    }
    let digest = md5.finish();
    if let Some(want) = &a.verify {
        if *want != digest {
            if !a.quiet {
                eprintln!("MD5 mismatch: {digest} (got), {want} (expected), {pictures} pictures");
            }
            return Ok(ExitCode::FAILURE);
        }
    } else {
        match a.output.as_deref() {
            Some("-") | None => println!("{digest}"),
            Some(path) => std::fs::File::create(path).and_then(|mut f| writeln!(f, "{digest}")).map_err(|e| format!("{path}: {e}"))?,
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn main() -> ExitCode {
    let a = match args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    };
    match run(&a) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}
