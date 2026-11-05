# fast-avif

An AV1 still-image encoder and decoder for AVIF, in Rust only: no C and no assembly.

- **Encoder:** codes 8-bit 4:4:4 colour and 8-bit monochrome alpha as one key frame. It has
  every intra prediction mode, palettes, an RD search, libaom's tune IQ, and deblocking and CDEF
  strengths searched on the decoded picture. Efforts run from 0 to 9, and tile columns can be
  coded on the caller's threads with the bytes of one thread.
- **Decoder:** rav1d's complete AV1 decoder, without its C API and assembly. It can decode by
  superblock rows, on the caller's threads and into the caller's memory.

fast-avif writes and reads the AV1 payload of an AVIF file. The container (`ftyp`, `meta` and
their boxes) is the caller's.

## Use

```toml
[dependencies]
fast-avif = { git = "https://github.com/marcbachmann/fast-avif", features = ["encoder"] }
```

```rust
use fast_avif::enc::{still, Options};
use fast_avif::{Decoder, PlanarImageComponent, Rav1dError};

let (width, height) = (64, 48);
let y = vec![128u8; width * height];
let (u, v) = (y.clone(), y.clone());

// qindex 0-255 (0 is lossless), effort 0-9, screen content, pixel count
let options = Options::effort(120, 4, false, (width * height) as u64).tune_iq();
let coded = still(width as u32, height as u32, &[&y, &u, &v], &options);
// coded.data: the sequence header and frame OBUs; coded.config: the `av1C` payload

let mut decoder = Decoder::new()?;
decoder.send_data(coded.data.into_boxed_slice(), None, None, None)?;
let picture = match decoder.get_picture() {
    // with frame threads the first call returns before the picture is done; the second waits
    Err(Rav1dError::TryAgain) => decoder.get_picture()?,
    result => result?,
};
let y = picture.plane(PlanarImageComponent::Y);
```

The planes are packed rows of `width` bytes. One plane is monochrome; three are 4:4:4 YCbCr,
BT.601, full range, sRGB.

- [`doc/encoder.md`](doc/encoder.md) covers the encoder: how it codes a picture, its search
  and efforts, and its tests.
- [`doc/decoder.md`](doc/decoder.md) covers the decoder: its API (rows, threads, picture
  memory), its SIMD and its conformance tests.

| Feature | Default | |
|---|---|---|
| `bitdepth_8` | yes | 8-bit decoding |
| `bitdepth_16` | yes | 10- and 12-bit decoding |
| `encoder` | no | the encoder, `fast_avif::enc` |

The decoder writes why a header or a picture failed to stderr only when `DEBUG` names
`fast-avif`, as Node's `debug` reads it: `DEBUG=fast-avif` or `DEBUG=*`.

## Crates

| Crate | Path | |
|---|---|---|
| `fast-avif` | `.` | the one to depend on |
| `fast-avif-enc` | `crates/enc` | the encoder |
| `fast-avif-dec` | `crates/dec` | the decoder |
| `fast-avif-shared` | `crates/shared` | tables, DSP, picture buffers and CPU detection both use |
| `fast-avif-cli` | `tools` | `dav1d`, the conformance runner |

## Platforms

SIMD is chosen at run time from the CPU's features:
- SSE4.1 and AVX2 on x86 and x86_64;
- NEON on aarch64;
- `simd128` on wasm32 when the build enables it;
- scalar code elsewhere.

Everything builds on stable Rust 1.82 or newer, except 32-bit `arm`, which needs nightly.
`rust-toolchain.toml` pins a nightly for development, so use `cargo +stable` for a stable build.

CI builds and tests:
- Linux on x86_64, i686 and aarch64;
- macOS on x86_64 and aarch64;
- Windows on x86_64;
- Android on aarch64;
- wasm32 in wasmtime.

## Build and test

```sh
cargo build --release
cargo test --release --workspace --lib --features encoder
cargo test --release --tests
.github/workflows/test.sh -r target/release/dav1d    # conformance vectors, needs Meson
```

`opt-dev` is a faster-to-build profile with debug assertions. `checked-release` is a release
build with every check on. See [CONTRIBUTING.md](CONTRIBUTING.md) before sending a change.

## License

BSD 2-Clause, see [COPYING](COPYING). The decoder comes from
[rav1d](https://github.com/memorysafety/rav1d), the Rust port of
[dav1d](https://code.videolan.org/videolan/dav1d). The encoder ports parts of libaom and rav1e.
[THANKS.md](THANKS.md) credits them, and [doc/PATENTS](doc/PATENTS) holds the AV1 patent
license.
