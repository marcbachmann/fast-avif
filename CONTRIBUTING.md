# Contributing

## Before sending a change

CI runs these, and a change has to pass all of them:

```sh
cargo +stable fmt --check
cargo +stable clippy --workspace --all-targets --features encoder -- -D warnings
cargo +stable test --release --workspace --lib --features encoder
cargo +stable test --release --tests
cargo build --release && .github/workflows/test.sh -r target/release/dav1d
```

Clippy also has to be clean on the nightly that `rust-toolchain.toml` pins. A newer stable
clippy flags more than that nightly does, so run both.

## What a change keeps

- **The decoder's pictures.** The conformance md5s fix every decoded pixel. A change that moves
  one is a bug, whatever it speeds up.
- **The encoder's invariant.** Every stream it writes decodes to exactly the reconstruction it
  made. The tests check this on random decisions, tiles, threads and every effort.
- **A measurement with every search change.** A change that alters the encoder's output gives
  its quality at equal bytes and its time, on photos and on graphics. Its `Options` field's doc
  comment records the result. A change meant to be exact shows the same bytes.
- **Benchmarks that can be compared.** Run them pinned, with an A/A of the base next to the A/B,
  and use the release profile that ships.
- **Scalar twins for SIMD.** Each vector kernel has a scalar version and a test that the two give
  the same output. Keep `unsafe` on the lowest call, the intrinsic, and not on the kernel.
- **Few comments.** Comment only what the code and its names cannot say. Tests live in the file
  they test, and their names say what they check.

## Commits

- Make one change per commit. The message says what changed and why.
- Keep history linear: rebase onto the target branch and merge fast-forward only.

## Licence

Contributions are under the BSD 2-Clause licence in [COPYING](COPYING). Code ported from
another project keeps that project's notice in the ported file. Its licence must allow that, as
BSD-2 and MIT do. Code under the AGPL, or under no stated licence, is not read and not copied.

AV1 is covered by the [Alliance for Open Media Patent License 1.0](doc/PATENTS). Read it before
contributing.
