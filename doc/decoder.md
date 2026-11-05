# The decoder

`crates/dec` is a complete AV1 decoder. It comes from rav1d, the Rust port of dav1d, and
decodes what dav1d decodes, to the same pixels:
- 8, 10 and 12 bits;
- 4:0:0, 4:2:0, 4:2:2 and 4:4:4;
- every tool, intra and inter, with film grain, operating points and scalable layers.

AVIF needs one key frame, but the decoder is not cut down to it.

## What differs from rav1d

- **No C.** The C API, its `Dav1d*` mirror types, the callbacks for allocation and logging, and
  every raw pointer and stride that was passed alongside its safe argument are gone. The decoder
  is used through the Rust API alone.
- **No assembly.** dav1d's assembly is replaced by Rust SIMD kernels written after it, and
  those are the only kernels.
- **No libc and no parking_lot.** The locks are std's, and a poisoned lock is recovered, as
  parking_lot's never poisoned.
- **Crates.** The decoder is split from what it shares with the encoder: headers, tables, CDFs,
  intra prediction, inverse transforms, the range decoder, CPU detection and picture buffers live
  in `crates/shared`.
- **A wider Rust API.** It can decode by superblock rows, run its workers on the caller's
  threads, read data in place, put pictures in the caller's memory and parse a sequence header
  alone.
- **Strides.** They are never negative.

## API

`fast_avif` re-exports the API of `crates/dec/src/rust_api.rs`, which started as dav1d-rs's.

```rust
use fast_avif::{Decoder, PlanarImageComponent, Rav1dError, Settings};

let mut settings = Settings::new();
settings.set_n_threads(1);
settings.set_max_frame_delay(1);
let mut decoder = Decoder::with_settings(&settings)?;
decoder.send_data(data.into_boxed_slice(), None, None, None)?;
let picture = decoder.get_picture()?;
let y = picture.plane(PlanarImageComponent::Y);
```

### Sending data and taking pictures

`send_data` takes one temporal unit, or any part of an OBU stream. When it returns
`Err(TryAgain)`, take the pictures that are ready, then call `send_pending_data` until it returns
`Ok`. `get_picture` returns `TryAgain` while it has no picture. Call it once per unit you send,
because two calls in a row drain the decoder: the second waits on the oldest frame thread.
`flush` drops the frames still in flight and any data still pending.

`send_data_owned` takes anything that is `AsRef<[u8]>` and reads its bytes in place. The decoder
drops it once nothing reads it any more, at the latest when the decoder itself is dropped. If its
`Drop` panics, the panic is caught.

### Threads

`Settings::set_n_threads` sets the worker count, where 0 means one per CPU, and
`set_max_frame_delay` sets how many frames are decoded at once. `Decoder::with_spawner` takes
its `n_threads - 1` workers from a `Spawner`, a function that runs a closure on one of the
caller's threads and says whether it took it. The thread decoding works too until its frame is
done, and with no worker taken it decodes alone. Frames are decoded one at a time. Dropping the
decoder returns once every worker has returned.

### Rows

`set_decode_by_rows(true)` sets each frame up when it is parsed, without decoding it.
- Each `decode_rows` call decodes one more superblock row, and returns how many picture rows no
  later row will change.
- `rows_picture` is the picture being written. Read its finished rows through
  `Picture::plane_ptr`; `plane` panics on it.
- `get_picture` returns the picture once every row is done.

This mode only applies to frames decoded on the calling thread alone. A frame with film grain,
one that copies blocks from rows above (intra block copy) or one that is not output is decoded
whole. `set_rows_spawner` decodes the tile columns of each row on threads from a `Spawner` as
well as on the calling thread.

### Picture memory

`Settings::set_allocator` takes a `PictureAllocator`, which hands out a `PictureBuffer` per
picture. Each plane must:
- start at a multiple of `PICTURE_ALIGNMENT`;
- have a stride that is a multiple of it and at least `PictureParameters::min_stride`;
- take `PictureParameters::plane_len` bytes that overlap no other plane;
- end at least `PICTURE_ALIGNMENT` bytes before the memory does, because the SIMD kernels read
  that far past it.

`PictureParameters::plane_layout` is the layout the default pool uses. The allocator is called
from the threads that decode, and a panic in it fails the frame. Without one, each decoder pools
its own buffers. A `Picture` is `Send` and `Sync`.

### Headers and errors

`parse_sequence_header` reads the first sequence header OBU of some data without a decoder.
Errors are `Rav1dError`, with dav1d's errno values. The decoder writes why a header or a picture
failed to stderr only when `DEBUG` names `fast-avif`.

## SIMD

The kernels are written once on a lane type, `simd128.rs`'s eight `i16` lanes, which maps to
SSE4.1 on x86, NEON on aarch64 and `simd128` on wasm32. On a target with none of these it maps
to a scalar stand-in. The kernels never name a target.

Some kernels add an AVX2 tier on x86_64, and some an aarch64 tier with dot products where the
toolchain has them. Kernels are chosen at run time from the CPU's features.
`rav1d_set_cpu_flags_mask`, the CLI's `--cpumask`, limits the decoder to a lower tier or to the
scalar code.

The kernels cover intra prediction, inverse transforms, motion compensation and warp, loop
filter, CDEF, loop restoration, film grain, palettes, reference motion vectors and the range
decoder's symbol adaptation. The `*_matches_scalar` tests compare vector code with the scalar
code on the machine they run on.

## The conformance runner

`tools/dav1d.rs` builds a binary named `dav1d` that takes dav1d's flags:
- `-i`, `-o`, `--muxer md5|null`, `--verify`;
- `--threads`, `--framedelay`, `--filmgrain`, `--oppoint`, `--alllayers`, `--sizelimit`,
  `--strict`, `--decodeframetype`, `--cpumask`, `--limit`.

It reads IVF, Section 5 and Annex B streams and hashes each picture's planes as dav1d's md5
muxer does, so dav1d's test vectors and their md5s check it unchanged. It runs on the Rust API.

## Tests

- **Conformance.** `.github/workflows/test.sh -r target/release/dav1d` runs the 792 vectors of
  `tests/dav1d-test-data` through Meson, with `-f N` for a frame delay with threads.
  `tests/dav1d_argon.bash` runs the Argon suite.
- **`tests/rust_api_rows.rs`.** Rows, data read in place, allocators, workers on the caller's
  threads, flushes and sequence headers. Each must give the pictures a plain decode gives, and an
  allocator or picture memory that fails must fail only its frame.
- **`tests/uninit.rs`.** Decodes with every new allocation filled with one byte, then with
  another. A picture that changes between the two read memory nothing wrote.
- **wasm32.** `examples/ivfhash.rs` hashes every picture row. CI compares a native build with a
  `wasm32-wasip1` build run in wasmtime.
- **Unit tests.** `cargo test -p fast-avif-dec --lib` runs the decoder's unit tests, the SIMD
  lanes among them.

## Sources and licences

| Source | Licence |
|---|---|
| dav1d, VideoLAN and the dav1d authors | BSD-2 |
| rav1d, the Rust port, Internet Security Research Group | BSD-2 |
| dav1d-rs, the start of `rust_api.rs` | MIT, notice in the file |

See [COPYING](../COPYING) and [THANKS.md](../THANKS.md).
