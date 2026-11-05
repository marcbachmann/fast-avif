# The encoder

`crates/enc` codes one still picture as an AV1 key frame for AVIF. It replaces libaom as an AVIF
encoder: it should code at least libaom's quality at the same size, on photos and graphics alike,
in less time and far less memory. It is not bit-exact with libaom, so every search can take its
own shortcuts.

## What it codes

| | Colour | Alpha |
|---|---|---|
| Layout | 8-bit 4:4:4, profile 1 | 8-bit monochrome, profile 0 |
| Colour | BT.601 matrix, full range, sRGB transfer | |
| Headers | still picture, reduced still-picture header, one key frame | same |
| Tuning | `Options::tune_iq`, libaom's tune IQ | none, as libaom's tune PSNR |

The caller chooses the qindex (0 is lossless) and the effort (0 to 9). Mapping a 0-100 quality
to a qindex is up to the caller. The output is the sequence header and frame OBUs, plus the
`av1C` payload, and the container is the caller's too.

Not coded: inter frames, intra block copy, loop restoration, superres, film grain, 4:2:0,
4:2:2, and 10 or 12 bits. The search is written generically, so a bit depth or layout can be
added without rewriting it.

## How a picture is coded

The encoder lives in its own crate, apart from the decoder, and is built on
`fast-avif-shared`. That crate gives it intra prediction and edge preparation exactly as the
decoder sees them, the inverse transforms, the CDF tables, scans, quantizer tables and the
SIMD layer. The reconstruction frame is the decoder's own picture buffer, so the encoder
predicts from exactly the pixels a decoder will have.

1. **Rows in.** `Encoder::push` takes source rows, and a superblock row is coded once it is
   complete. The encoder keeps the source and the reconstruction of one superblock row, plus an
   edge buffer for the row above, the same one the decoder keeps.
2. **Search, then code.** Each superblock is first searched dry. Blocks are coded against
   snapshots of the reconstruction and the contexts, and their costs are counted from the CDFs
   as they stood when the superblock began. The plan that wins is then coded for real by the same
   code, which reuses the search's transform blocks and its bit counts, so nothing is
   reconstructed twice.
3. **Tile columns.** Photos over 512×512 get two tile columns (`tile_cols_log2`). `still_on`
   hands the columns after the first to the caller's `Spawner`, and the caller codes any column
   no worker took. The columns merge before the filter search. The learnt prunes count per tile
   column, so the bytes are the same whichever thread codes a column (`tile_threads`,
   `tile_threads_graphic`). Screen content and smaller pictures stay one tile, because each
   tile's CDFs cost a fixed 100-300 B.
4. **Filters last.** The frame header is written after the tile data, so the deblocking and
   CDEF strengths are chosen once every tile is coded. `filters::search` decodes the frame
   once through a `FilterTrials`, the decoder's `DecoderTrials`, and filters it again for each
   candidate. The root crate's `enc::still` passes `DecoderTrials` in, so no build reaches the
   search without it.
5. **Write.** The headers and the tile data go out through the range encoder (`ec.rs`, libaom's
   `od_ec_enc` on the decoder's CDF layout).

## The search

**Rate and distortion.** Rates are counted from the live CDFs (`rd.rs`). λ comes from the
qindex as in libaom's `rd.c`. With tune IQ, λ is scaled per 16×16 block by its activity, and by
libaom's IQ weight at low qindexes. Distortion is the squared error of the reconstruction.

**Luma modes.** Modes are ranked by SATD, or by a DCT estimate of quantization error and rate
(`rank_rd`), and the best few go on to a full RD trial (`keep_modes`). Angle deltas,
filter-intra, palettes and CfL follow. Chroma takes the luma winner's mode, DC, smooth and CfL,
or every mode at the high efforts.

**Partitions.** The search tries None and Split, plus H and V where Split beat None
(`rect`, `rect_ratio`). Other prunes:
- libaom's intra CNN at the low efforts, ported bit-identical to libaom's C, with a guard that
  keeps it off drawn blocks;
- variance bands at efforts 0 to 3;
- prunes learnt per picture and activity class, which stop searching rect partitions and 8×8
  splits once 32 trials showed they rarely win (`rect_adaptive`, `split8_adaptive`).

**Transforms.** Transform depths go one or two below the largest transform. Types are searched
in full or from libaom's reduced intra set, and ranked by transform-domain distortion where
`tx_domain_dist` is set. The search uses `fdct_fast`, rav1e's lifting kernels, which have
libaom's scale but not its coefficients. `fdct.rs` is the bit-exact port of libaom's C
transforms; it is the reference the tests check `fdct_fast` against, and it handles WHT for
lossless.

**Quantization.** The quantizer has quantizer matrices and a chroma delta q under tune IQ, and
Variance Boost per superblock through delta q. RDOQ runs at efforts 8 and 9.

**Efforts.** `Options::effort` sets the search for an effort, with libaom's
`cpu-used = 9 - effort` as the model. `fast` adds the cpu-used 5 shortcuts that cost some
quality, and `drawn` replaces the photo cuts of efforts 0 to 3 for text and graphics. The doc
comment of each `Options` field records the measurement that set it.

## SIMD

The encoder's own kernels have a scalar lane plus AVX2 and NEON lanes that give the scalar
lane's output exactly:
- forward transforms (`fdct_fast`);
- quantizer and dequantizer (`quant.rs`);
- SATD and squared error (`dist.rs`);
- coefficient contexts and the counted AC rate (`contexts.rs`).

Prediction, inverse transforms and the filters use the decoder's kernels. Each module's
`lanes_agree` or `simd_matches_scalar` test compares every lane on the machine it runs on, so
the AVX2 lanes are checked on x86 and the NEON lanes on aarch64.

## Tests

`cargo test -p fast-avif-enc --profile opt-dev --lib` runs the encoder's tests:
- **The decoder agrees.** Streams from random decisions, random delta q, random quantizer
  matrices, lossless, palettes, tiles, threads and every low-effort cut are decoded and must
  equal the encoder's reconstruction (`random_decisions`, `tiles_decode_as_reconstructed`,
  `low_efforts_decode_as_reconstructed` and others).
- **The writer round-trips.** Symbols, bools, raw bits and coefficient tokens written by `ec.rs`
  are read back by the decoder's range decoder.
- **Bit-exact with libaom.** `fdct::tests::matches_libaom` checks the forward transforms against
  digests of libaom's C output (see below). `partition_cnn::tests::matches_libaom` checks the
  CNN through `scripts/cnn-partition-compare.sh`.
- **Kernels.** Every SIMD lane against the scalar one, and the forward transforms round-tripped
  through the decoder's inverse at every kernel level.

`scripts/enc-faults.sh` injects the faults listed in `enc-faults.py` one by one and reports
which tests fail on each.

### Benchmarks

`frame::tests::corpus` is an ignored test that codes a directory of P6 PPMs and prints time,
bytes and PSNR per picture:

```sh
ENC_CORPUS=<dir> ENC_QIDX=120 cargo test -p fast-avif-enc --release --lib frame::tests::corpus -- --ignored --nocapture
```

`ENC_*` variables set each `Options` field, and `ENC_CROP` crops the pictures.
`scripts/enc-corpus.sh` runs several configurations, and `scripts/enc-ab.sh` alternates two trees
on one pin.

### libaom's forward-transform digests

`fdct::tests::LIBAOM_DIGESTS` holds FNV-1a digests of libaom v3.15.1's C forward transforms on
10 blocks per size and type. To produce them again:
1. Configure libaom with `cmake -DAOM_TARGET_CPU=generic -DENABLE_TESTS=0 -DENABLE_DOCS=0
   -DENABLE_EXAMPLES=0 -DENABLE_TOOLS=0`, which is needed only for the config headers.
2. Build this harness with `gcc -O1 -ffunction-sections -fdata-sections -I<aom> -I<build>
   harness.c av1/encoder/av1_fwd_txfm1d.c av1/encoder/av1_fwd_txfm2d.c av1/common/av1_txfm.c
   av1/encoder/hybrid_fwd_txfm.c -Wl,--gc-sections`.

```c
#include <stdint.h>
#include <stdio.h>

#include "config/av1_rtcd.h"

typedef void (*Fwd)(const int16_t *, int32_t *, int, TX_TYPE, int);

static const Fwd fwd[19] = {
  av1_fwd_txfm2d_4x4_c,   av1_fwd_txfm2d_8x8_c,   av1_fwd_txfm2d_16x16_c,
  av1_fwd_txfm2d_32x32_c, av1_fwd_txfm2d_64x64_c, av1_fwd_txfm2d_4x8_c,
  av1_fwd_txfm2d_8x4_c,   av1_fwd_txfm2d_8x16_c,  av1_fwd_txfm2d_16x8_c,
  av1_fwd_txfm2d_16x32_c, av1_fwd_txfm2d_32x16_c, av1_fwd_txfm2d_32x64_c,
  av1_fwd_txfm2d_64x32_c, av1_fwd_txfm2d_4x16_c,  av1_fwd_txfm2d_16x4_c,
  av1_fwd_txfm2d_8x32_c,  av1_fwd_txfm2d_32x8_c,  av1_fwd_txfm2d_16x64_c,
  av1_fwd_txfm2d_64x16_c,
};
static const int W[19] = { 4, 8, 16, 32, 64, 4, 8, 8, 16, 16, 32, 32, 64, 4, 16, 8, 32, 16, 64 };
static const int H[19] = { 4, 8, 16, 32, 64, 8, 4, 16, 8, 32, 16, 64, 32, 16, 4, 32, 8, 64, 16 };
/* 0 DCT, 1 ADST, 2 FLIPADST, 3 identity, as libaom's vtx_tab / htx_tab */
static const int vtx[16] = { 0, 1, 0, 1, 2, 0, 2, 1, 2, 3, 0, 3, 1, 3, 2, 3 };
static const int htx[16] = { 0, 0, 1, 1, 0, 2, 2, 2, 1, 3, 3, 0, 3, 1, 3, 2 };

static int has_1d(int kind, int n) {
  if (kind == 0) return 1;
  if (kind == 3) return n <= 32;
  return n <= 16;
}

static uint64_t s;
static uint32_t next(void) {
  s ^= s << 13;
  s ^= s >> 7;
  s ^= s << 17;
  return (uint32_t)(s >> 32);
}

int main(void) {
  static int16_t in[64 * 64];
  static int32_t out[64 * 64];
  for (int size = 0; size < 19; size++) {
    const int w = W[size], h = H[size];
    const int cw = w < 32 ? w : 32, ch = h < 32 ? h : 32;
    for (int ty = 0; ty < 17; ty++) {
      if (ty == 16 ? size != 0 : !(has_1d(vtx[ty], h) && has_1d(htx[ty], w)))
        continue;
      s = 0x9e3779b97f4a7c15ULL ^ ((uint64_t)size << 8 | (uint64_t)ty);
      uint64_t d = 0xcbf29ce484222325ULL;
      for (int b = 0; b < 10; b++) {
        for (int i = 0; i < w * h; i++)
          in[i] = b == 0 ? 255 : b == 1 ? -255 : (int16_t)(next() % 511) - 255;
        if (ty == 16)
          av1_fwht4x4_c(in, out, w);
        else
          fwd[size](in, out, w, (TX_TYPE)ty, 8);
        for (int i = 0; i < cw * ch; i++) {
          const uint32_t v = (uint32_t)out[i];
          for (int k = 0; k < 4; k++) {
            d ^= (v >> (8 * k)) & 0xff;
            d *= 0x100000001b3ULL;
          }
        }
      }
      printf("        (%d, %d, 0x%016llx),\n", size, ty, (unsigned long long)d);
    }
  }
  return 0;
}
```

## Known weak spots

The weak spots, all at libaom's bytes:
- a synthetic strip pattern of sawtooth, diagonal and XOR planes;
- fine texture at Q90;
- antialiased text at Q80, which is not monotonic in qindex, because the quantizer steps
  resonate with its grey levels;
- grey noise, which RDOQ does not bring near libaom's size.

Speed against libaom moved with every step, so measure it again before quoting a number.

## Sources and licences

| Source | Licence | Use |
|---|---|---|
| libaom v3.15.1 | BSD-2, AOM Patent License 1.0 ([PATENTS](PATENTS)) | ported: range encoder, forward transforms, intra CNN, λ, tune IQ's settings; its notices are kept in the ported files |
| rav1e, through rusty-av1-toolkit | BSD-2 | `fdct_fast`'s kernels, with the headers kept |
| An AGPL all-intra libaom port | AGPL-3.0 | not read and not copied |

The step-by-step history of the encoder, with the measurement behind each decision, is the
former `doc/encoder-progress.md`:

```sh
git show 132588e4:doc/encoder-progress.md
```
