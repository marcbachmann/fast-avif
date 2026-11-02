# Encoder progress

The running handoff for `doc/encoder.md`: where the work stands, how to build and test it,
and one paragraph per step, newest first.

## State

- Branch `encoder`. Built: range encoder, headers, every intra tool with palettes, libaom's
  forward transforms, the RD search with RDOQ, tune IQ (Variance Boost, SSIM λ, quantizer
  matrices, chroma delta q), deblocking and CDEF strengths searched on the decoded picture.
- Not yet: restoration, threads, alpha and lossless parity checks.
- SIMD (AVX2, NEON twins with the same output): forward transforms, quantizer and
  dequantizer, SATD, squared error, coefficient contexts and the counted AC rate. Lane tests
  run on `worker` (x86) and through the queue host `local` (aarch64).
- image-processing branch `integrate-high-quality-avif` codes AVIF colour with it
  (`src/engine/avif/own.rs`, calibration `own::compare` with `AVIF_CORPUS` of PPMs). AVIF gates
  188/195 at rav1d a12c8720: misses at libaom's bytes on the strip pattern, Q90 fine texture,
  antialiased text Q80; noise is 63% larger at its quality.
- Open decisions in `doc/encoder.md` are taken at their proposed defaults (the
  implementation started on 2026-10-24 without answers to them): in the fork, 3× at effort 4, peak by
  width plus tokens, PSNR gate plus SSIMULACRA2, 8-bit 4:4:4 and monochrome first, effort 0–9
  as today.
- Porting reference: libaom v3.15.1 at `~/Development/livingdocsIO/libaom-v3.15.1`.

## Build and test

On `worker` through the queue, in the `ip-build:trixie` image, with the stable toolchain the
image ships (`RUSTUP_TOOLCHAIN=stable`; the repo pins a nightly):

```sh
D=/root/.cache/ld-image-processing-deps
docker run --rm --cgroup-parent="$COMMANDQUEUE_SLICE" -v "$PWD":/work -v $D:/deps -w /work \
  -e CARGO_TARGET_DIR=/deps/target-rav1d-enc -e CARGO_HOME=/deps/cargo-home-rav1d-enc \
  -e RUSTUP_TOOLCHAIN=stable ip-build:trixie \
  cargo test -p rav1d --profile opt-dev --features encoder --lib enc::
```

An incremental build takes about 17 s.

## Log

**Open, 2026-11-02: threads can change the bytes.** At efforts 4-7 with tile columns and the filter search, outside
the fast search, `still_on` on threads and `still` on one thread code a graphic to different bytes:
520x40 at qidx 120, tile_cols_log2 1, 3649 against 3656 B; 700x90, 8159 against 8107. 75333cf9 gives the same sizes, so this
is older than the low-effort work. `tile_threads` passes because it codes a photo. The doc of
`tile_threads` claims the same bytes, so the claim or the code has to change.

**2026-11-02, efforts 0-1: bits of a one-type transform block estimated; where the day ended.** Those blocks
count bits only to choose between coding and not. An estimate of 4 bits per nonzero level plus twice its log2 gives
instructions e0 0.96-0.98 at +0.002 dB mean, -0.01 worst, and e1 0.98-0.99 at -0.023, -0.14 worst (5b7169d8). Efforts 2-3
would pay -0.04 for 2-4% and stay as they are. Against c93e4db9, which already had effort 0's variance partitions, five photos, the median of five
paired rounds on the laptop with an old-against-old control at 0.97-1.02, qidx 200, 140, 90, 60: e0 0.70, 0.69, 0.74, 0.72;
e1 0.38, 0.54, 0.70, 0.74; e2 0.43, 0.59, 0.69, 0.73; e3 0.45, 0.60, 0.83, 0.77. Bytes -0.9 to +1.1%, dB +0.006 to -0.238.
Open: the matrix against libaom, steady x86 timing, the JS gates and a refuter, all on worker; Cuts is still trial code;
the rate bound and the CfL fit at effort 4 and up need those gates.

**2026-11-02, the search's replay of a block does no work twice.** Chosen because the replay that closes
a block's search counted its coefficients and reconstructed it again. It now takes the bits counted
during the search (e1f02b11) and leaves luma alone, and chroma too if the winner was the last tried (f04be867). The first
gives cycles e0 0.91-0.98, e2 0.95-0.98, e4 0.96-0.98; the second instructions e0 0.93-0.95, e2 0.95-0.96, e4 0.985-0.994.
Bytes and dB equal in 36 of 36 and 16 of 16 cells, photos and drawn. A fault that keeps stale chroma fails
tests by name, and my first version took a chroma palette trial for plain DC, which review caught
before the run; palette blocks keep the old path. `enc::` 54 passed.

**2026-11-02, the writing pass reconstructs nothing.** Chosen because a profile of the encoder alone
at effort 0 shows every block predicted and reconstructed again after the search left the same pixels in
place. Predicted: about 6% at effort 0, the same bytes everywhere. `search_sb` now restores only the
contexts, and `code_b` takes the search's transform blocks as they are, except where a palette needs its
index map. Bytes and dB equal in 36 of 36 cells, efforts 0-6, photos and drawn. Instructions, qidx 200 to 60:
e0 0.93-0.95, e1 0.95-0.97, e2 0.96-0.98, e3 0.97-0.98, e4 0.99, e6 0.998. Restoring the old pixels
fails 12 tests by name. `enc::` 54 passed. `ENC_NO_DECODE=1` leaves the bench's decode out of a count;
it had been 30% of the cycles and understated every ratio before it.

**2026-11-02, efforts 0-3: a transform type's bits counted only where they could win.** Chosen because
counted coefficients are a quarter of the encoding at effort 1, qidx 60. Predicted: a large share of
the counts gone at no cost. A type is skipped where its distortion and 4 bits per nonzero coefficient
already lose to the best so far. Five photos: 3 changes nothing in any cell, 4 skips 13-51% at
under 0.1% bytes and 0.002 dB, 6 starts to cost bytes. The gain is smaller than the share of skipped
counts suggested: instructions 0.96-0.99x, cycles 0.96-0.99x. Wall time could not see it, so this is
the first step judged by `time -l` counters, where two runs of one binary agree to 0.1% and about
1.3%. Cycles of the commit before today's chroma work against this one, at qidx 200, 140, 90, 60: e1 0.59, 0.71, 0.87,
0.92; e2 0.58, 0.73, 0.80, 0.83; e3 0.54, 0.72, 0.81, 0.86. e0 reads 1.05, 0.93, 0.98, 0.90, with an
old-against-old spread up to 1.32 at qidx 200, so e0 is not resolved. `enc::` 54 passed.
Next: effort 0 and 1 at high quality, where they are furthest from libaom.

**2026-11-02, efforts 1-3: partitions settled by variance outside a band.** Chosen because the
single threshold of effort 0 costs 0.26-0.39 dB at these efforts. Predicted: a band keeps most of
its speed at a small part of its loss. A block under 1 squared quantizer step stays whole, one over 64
splits, and the rest search as before with the CNN. Five photos, ten curves' worth of dB at equal
bytes: mean -0.03 to -0.08, worst -0.28. Without the CNN the band loses nothing (+0.01 to +0.03 on efforts 1-2)
but runs up to 1.58x at qidx 60, so the CNN stays. Paired timing on the laptop against an
old-against-old control, new/old median: qidx 200 e1 0.54, e2 0.50, e3 0.46; 140 0.74, 0.80, 0.71;
90 0.99, 0.86, 0.89; 60 1.02, 1.03, 0.90. Only qidx 200 and 140 clear the control's quartiles well, so
this speeds low quality and leaves high quality about as it was. At effort 4 the same band loses up to 1.43 dB, so it stays off there. d2edcb76: bytes equal to the
measured arm in 15 of 15 cells, `enc::` 54 passed. Not yet on the matrix.

**2026-11-02, drawn pictures: their own search at efforts 0-2.** Chosen because the committed matrix
has every effort from 4 up under libaom's time, photos at efforts 0-1 near it, and drawn pictures
at efforts 0-1 at 1.7-2.3x with about 11 and 4.5 dB in hand at equal bytes. Predicted: 0.43x at
effort 0, which is about libaom's time. Each cut alone on six drawn pictures, then combined against
the old search. Effort 2 drops luma angles and takes chroma DC plus CfL by the fit; effort 1 also
five luma modes and no 8x8; effort 0 also one luma candidate, depth 0, no 64x64, no filter-intra,
no chroma angles and CfL by share. Time on the laptop, sum over the seven qindexes, faster of two
rounds, no control: e0 0.45x, e1 0.58x, e2 0.64x. dB at equal bytes against the old search, in
range: text screenshot -0.1 to -2.4, logo -0.2 to -2.5, red -1 to -4.5, plum to -4.3, the small
plum and star gain at some rates and lose up to -4.4; bytes up to 2.5x at the lowest qualities.
The photo search is rejected: 0.29x, but -10.7 dB on text. The partitions by variance took text
down the same way. A test now decodes every low-effort cut for photos and drawn pictures against
its reconstruction: 54 passed, and it fails on an injected alpha range. Effort 3 is unchanged.
Not yet measured against libaom on these commits, which needs the matrix on worker.

**2026-11-02, efforts 0-3: CfL by a closed-form fit, tried only where luma explains the chroma.**
Chosen because chroma was the largest stage left at effort 0. Predicted: about 0.85x at effort 0
at under 0.1 dB. The fit takes the alphas from the least squares of the source on the luma AC
without predicting. Against the walk it replaces it is level on photos (within 0.05 dB) and gains
up to 1.2 dB on hard-edged colour, where the outward walk stopped early, with one dip of 0.44 dB
(plum, effort 2, qidx 140, on a near-flat curve). Choosing DC or CfL from the fit's gain alone
lost up to 0.58 dB at about the same bytes, so CfL still has to win by RD. Dropping CfL is 0.80-0.88x at effort 0
and costs photos 0.10 dB, but red-at-12-oclock 41-55% more bytes and plum 5-19%. Share 0.6 of the
variance explained, with a floor of 2 squared steps, keeps both pictures at least level but for
one 0.03-0.36 dB dip on plum. Chroma coefficients per pixel over five photos: e0 17 -> 12, e1 24-28 -> 17-19, e2 28-33 -> 18-23,
e3 58-72 -> 49-62. dB at equal bytes on photos: mean -0.04 to -0.06, worst -0.20, mostly at qidx
200. Time, ten paired rounds on a loaded laptop with an old-against-old control, qidx 140 and 90:
e0 0.92, 0.94; e1 0.92, 0.92; e2 0.94, 0.98; e3 0.98, 0.99. Clear of the control only at e1; e3
is inside it. Needs a steady x86 run. Next: the fit at effort 4 and up, with the gates.

**2026-11-02, effort 0: partitions by variance, one luma code.** Chosen because effort 0 sat at
about 2.5x libaom's effort 0 on photos. Predicted: 0.4x its old time, at no more than 0.3 dB.
`pick_partition` takes None or Split from the block's variance against the quantizer step
(`var_split` 4) instead of trying both, without the partition CNN. `one_pass` skips the DCT-only
luma RD when one candidate and no depth search leave nothing to choose, which codes the same
bytes and dB at efforts 0 and 1, all four qindexes. Five photos, laptop (aarch64), three
interleaved rounds, effort 4 control within 2%: qidx 200 199-200 -> 82-83 ms (0.41x), 140 257-259
-> 116-117 (0.45x), 90 289-292 -> 164-169 (0.57x), 60 299-304 -> 197-198 (0.66x). dB at effort
0's curve -0.12 to -0.33 mean, worst -0.81, from the sweep of the variance cut alone. So the
prediction holds only at low quality. No steady x86 run and no matrix on this commit yet; `enc::`
tests 53 passed, 3 ignored. Next: chroma, the largest stage left at effort 0.

**2026-10-27, column cache, CDEF repeats, lazy partition snaps.** Three more exact steps,
pinned on x86 (effort 4): `fdct_fast::Columns` keeps a residual's column pass between transform
types with the same vertical transform (DCT_DCT/DCT_ADST, ADST_DCT/ADST_ADST share one); a
CDEF trial of the header's own strengths is skipped; `pick_partition` restores its start only
once a candidate has changed it and snaps the best only before a later candidate overwrites
it. Photo q189 fast 1621 → 1587 ms, default q75 6571 → 6490, screenshot q144 fast 1014 → 996.
`predict_plane`'s own 9% has no line over 1.6% (residual subtraction 0.7%, the transform
block's push 0.5%); a further cut there is a search change (fewer trials, or trials without
reconstruction), not an exact one.

**2026-10-27, fast-search filters and chroma, quantizer skips.** Profiled with `perf` on
`worker` (x86, `ENC_EFFORT=4 ENC_FAST=1`): the photo spent 23% in the filter search (CDEF
trials), 11% in the quantizer's f64 divisions, 5% in the CfL alpha scan (32 predictions of
both planes), 3% in the partition CNN. `Options::fast` now picks filters as libaom at
cpu-used 6 and 5 (`LPF_PICK_FROM_Q`, `CDEF_FAST_SEARCH_LVL3`: primaries 0, 2, 4, 8, 14 and
secondaries 0, 2, 7 trials instead of 23) and searches each chroma plane's CfL alpha and
the chroma angle delta outward from 0 while the cost falls (`uv_outward`, libaom's
`cfl_search_range` 3 and angle search): screenshot q144 1268 → 1118 ms at −0.5% bytes and
+0.07 dB, photo q189 2388 → 1903 ms at −0.02 dB, q75 3697 → 3118 ms at −0.03 dB. Three steps
with the same bytes for every search: a group of 8 (AVX2) or 4 (NEON) coefficients that all
quantize to 0 (128a < q(128 − rounding)) skips the division (photo fast −5%, default q75
−1.2%); the dequantizer reads up to the last nonzero level in raster order, which the
quantizer returns (−1.6%); the CNN's convolutions compiled for AVX2 (−3%). Together, pinned
on x86: photo q189 fast 2388 → 1625 ms, screenshot q144 fast 1268 → 1010 ms. On the 13-picture
keep corpus (`own::compare::efforts`, effort 4, 8 at a time) the new fast search against the
old at libaom's bytes: photos −0.01 dB at Q40 and −0.04 at Q75, graphics −0.07 and +0.20,
time ×0.73–0.84; the one loss is a logo at Q40, −0.71 dB at its qindex for −2% bytes (−0.16
at equal bytes). Pinned to
image-processing branch `avif-fast-speed` (Rust avif 44/0, engine-avif and formats JS
1771/0). Trap: a job cannot create files in a shipped tree on `worker` (`Permission denied`
for the job user; only the container as root can), and a `[patch."<git url>"]` section
appended there did nothing, so two harness runs compared a tree with itself; replace the
dependency line inside the container instead.

**2026-10-26, block copies and context saves.** Profiled on an M3 with `ENC_EFFORT=4 ENC_FAST=1`
(the product's options in the corpus bench): a 3000×2000 photo at qindex 189 and a 1864×1620
screenshot at 144. `memmove` was 9–11% (block rows copied at a runtime width, contexts saved
and put back unit by unit) and the transform block's context sum 4.5% (a saturating fold that
does not vectorize; 1024 levels under 2^20 cannot wrap). Copies at a width known per block
size, 16-unit context saves, the sum without saturation and the levels zeroed once per trial,
each the same bytes on three configurations: photo q189 fast 3252 → 2786 ms (−14%), q75 full
search 8951 → ~7900 ms (−12%), screenshot q144 fast 1549 → 1363 ms (−12%). The filter strength
search is now 23% of the photo (CDEF trials). Not taken: stopping the CDEF strength walk after
two rises picks other strengths on 3 of 72 searches (6 pictures × 3 qindexes); two tile
columns for screen content cost +0.2–0.6% bytes on the screenshot and +0.3–1.7% on a diagram.
On the laptop the WHT round-trip test reaches `unreachable!` at the scalar CPU level: rav1d's
NEON build has no Rust WHT; `--skip enc::fdct::tests::round_trip_through_rav1d` there.

**2026-10-25, tile columns on threads.** Two tile columns on the full-size corpus (qindex 140
and 60) cost +0.67% and +0.49% bytes at +0.048 and +0.020 dB; on threads the corpus took
3311 → 1947 ms, hills 960×512 646 → 395 ms at 17.4 MB peak (16.1 MB on one tile). The fixed cost
of a tile's own CDFs failed three AVIF gates on small and flat pictures (graph paper 431 → 704 B,
a 300×200 grey ramp −1.5 dB, a posterised sepia photo −1.1 dB at libvips' bytes), so screen
content and pictures up to 512×512 stay one tile. A refuter broke the first version's borrowed
planes (a spawner that panics after taking work, or refuses and runs it later, left workers
reading freed planes); workers now hold the planes through an `Arc` and the caller codes every
column no worker started. The wavefront's first step (search CDFs from the superblock
above-right, a row's first qindex from the base) costs −0.017 to −0.047 dB at equal bytes and
stays on branch `wavefront`.

**2026-10-25, three exact steps that gained nothing.** Profile at f5df65cb (crops, qindex 140,
effort 4): `predict_plane` 10% self with no hot line, the quantizer 9.6%, `write_coefs` 5.9%,
`ac_bits` 6.2%, `sse` 4.7%; stages rank, depth and chroma about a quarter each. Each step below
kept the bytes and timed within noise on a pinned A/B (q140, q32, screen q100):
the quantizer's two f64 divisions per 8 coefficients replaced by an f32 reciprocal corrected by
its remainder (the divider was not the limit); a block stopping once its luma distortion
passes the partition candidate's budget (3 of 465 budget crossings stop there, bits and chroma
push the rest over); a transform type whose quantization error alone loses skipping its rate
count. The AVX2 and scalar quantizers disagree at rounding 128 for a zero coefficient (level 1
against 0); no caller uses it.

**2026-10-25, rebased onto `main`; the filter search on one decode.** `encoder` now sits on
marcbachmann/main 082c9e21 (frames decoded by superblock rows, a superblock row of filter and
edge buffers on one thread, tile columns on the caller's threads), no conflicts; bytes and
speed as before, the lib suite and `tests/decode_rows.rs` pass. The strength search decoded the
frame 24 times; now one decode with the filters off keeps the loop filter's levels and masks
frame-wide (`keep_filter_data`, the multi-thread layout) and an unfiltered copy of the picture,
and each candidate refilters it (`rav1d_filter_trials`, `rav1d_refilter_sbrow`) (b9f14be4).
Same bytes; −10.0% photo case q140, −8.7% screen q140, −12.0% q32. Memory: the search holds
the picture twice, a 1400×1400 frame peaks at 46 MB VmHWM instead of 39–40. A refuter round
found no difference over 4250 pictures and 78 corpus encodes; the session now refuses
segmentation and a trial may not change the CDEF index bits, sharpness or mode deltas. Against
libaom at effort 4: photos 0.63–1.13× at Q75 (most 0.70–0.90×), 1.08–1.57× at Q40, graphics
0.70–1.05×, gradient 2.1–2.5×; quality unchanged.

**2026-10-25, the block search's own overhead.** Same bytes at every step, pinned on 13 crops:
- Trials restore only the planes they write (d05b0004): −3.3% q32, −3.8% q140.
- Level vectors and transform block lists recycled, replays reuse their levels, the chroma
  search replays the luma list instead of a clone (cd37774a): −0.8%, −0.7%; `malloc` and
  `free` had been 1.7%.
- Palette index symbols from the left, top and top-left indexes instead of rav1d's
  `order_palette` per pixel (85f4adc4): −3.9%, −5.2% with screen tools on.
- Palette k-means takes each distance once (bc59493d): −1.6% at q140.
- Trials within a block restore only the contexts (be69cb36): a trial predicts each transform
  block from pixels outside the block or ones it coded itself and overwrites what it measures.
  Photo case (screen tools off) q140 −4.6%, screen q140 −2.8%, q32 −1.6%. A refuter poisoned
  the block's pixels (random, 0, 255) before every trial over 12 odd-sized pictures × 5 qindex
  × 5 settings and the 13 full pictures: every hash equal; a planted stale read changed 6-7 of 12.
- Dead end: skipping the quantizer's division for chunks that are all zero, ±0.5%.

Against libaom (cpu-used 5, one thread, pinned) at effort 4 after this round: photos 0.69–1.25×
libaom's time at Q75 (most 0.77–1.04×), 1.15–1.65× at Q40; graphics 0.98–1.26×, gradient
2.1–2.6× (24 ms in libaom); quality unchanged.

Left in the photo case at q140: `predict_plane` 9%, quantizer 8%, filter strength search 9-12%
(24 full decodes; since b9f14be4 one), intra edge preparation 2%.

**2026-10-25, SIMD for the search.** Profile at 845161bb, qindex 32: rate counting
(`write_coefs` with a counter) 23%, quantizer 8%, dequantizer and `eob_of` 6%; at qindex 140
prediction 14%, SATD 3%. Each step same bytes, pinned on 13 crops (q32 full / q140 pre-ranked):
- Quantizer in f64 lanes (exact floor for operands under 2^31) returning the end of block and
  the levels' error; dequantizer in i32 lanes; the search hands its end of block to
  `predict_plane` (9ea2a667): 7035 → 5798 ms, 3918 → 3307 ms.
- SATD over a whole block, 16 columns per AVX2 register (05978c01): −2.9% at q140, +0.4% at
  q32 where SATD is rare.
- Squared error (29fd75f5): −6.3%, −9.1%.
- Coefficient contexts of a block at once (e7b148a8). The log's "not possible" below was wrong:
  the 4.9% of reads that differ from rav1d's one-by-one decode land past a column's end, and
  libaom's level buffer with 4 rows of padding under each column reads 0 there as rav1d does
  (0 of 1.77M reads differ). −1.9% / +0.1% alone; filling the padded buffer in a scalar pass
  without SIMD contexts was +10% / +3%.
- With every context known, the counted AC rate sums in raster order (307557d2): AVX2 masked
  gathers of the base and base-range costs; NEON (no gather) takes the scalar raster loop.
  −15.2%, −4.4%.

Together: 7035 → 4540 ms at qindex 32 (−35%), 3918 → 2802 ms at qindex 140 (−28%), bytes
unchanged. A refuter round (009945d8) found the AVX2 `ac_bits` dropping the base-range cost of
an `i32::MIN` level (unreachable: levels stay under 2^15) and lane tests that compared the
scalar lane with itself: `Lane::detect()` reads rav1d's CPU flags, empty on x86 until a decoder
or `Dsp::new` sets them; tests take `Lane::testable()`. It found the bytes equal on AVX2, NEON
and 845161bb over 140 encodes. Dead end: the quantizer in 8 f32 lanes with an integer
correction, plus skipping chunks past the end of block in `ac_bits`, −0.5% / −0.3%.
Against libaom (cpu-used 5, one thread, pinned, image-processing `own::compare` at opt-level 3)
at effort 4 after this round: photos 0.81–1.46× libaom's time at Q75 and 1.36–1.92× at Q40,
graphics 1.11–3.04× (gradient, 25 ms in libaom); quality at libaom's bytes as before (photos
−0.08 to +0.30 dB, hills Q75 −0.71). The `ip-build:trixie` image on `worker` was gone (all images pruned) and was rebuilt
from image-processing `scripts/build.Dockerfile`.

**2026-10-25, libaom's intra CNN partition pruning** (branch `encoder-cnn`). `partition_cnn.rs`
ports `intra_mode_cnn_partition` with the CNN and dense layers it runs, in f32 in libaom's C
order; `scripts/cnn-partition-compare.sh <libaom>` builds libaom's own `cnn.c`/`ml.c` and model
(generic C, no FMA) and compares: 5304 blocks (2400 random, all 256 qindexes, 2648 corpus
superblocks), 9.1 M values, all bit-identical, no decision differs at any of the three
resolutions. `Options::cnn_prune` (off by default, `ENC_CNN_PRUNE`): level 1 prunes Split below
the no-split threshold and H/V above the split threshold, level 2 also None; only in superblocks
wholly inside the picture, from the superblock's coded qindex, None never pruned where Split is
not searched (`levels`). Pinned on the 13 crops at the product settings, best of 3 rounds:
qindex 32 7038 / 5243 / 4496 ms (off / 1 / 2), qindex 140 3914 / 2961 / 2680 ms. At equal bytes
(slopes from qindex 28/36 and 132/148: 1% bytes is 0.093 dB at 32, 0.049 at 140) the ten photos
lose 0.015 / 0.033 dB at 32 for -28% / -38% and nothing at 140 (+0.003 / +0.007) for -24% /
-32%; the three synthetic pictures (gradient, logo, UI) lose 0.25 / 1.19 dB at 32 and 0.44 /
0.90 at 140 (graphic-logo 61.49 -> 58.82 dB at 32 with level 2, graphic-ui +12% bytes at 140).
Over all 13: -0.133 / -0.280 dB at 32, -0.083 / -0.104 at 140.

**2026-10-25, libaom's search shortcuts.** Timed against libaom (cpu-used 5, one thread, every
crate at opt-level 3, pinned): 1.9-6.5x slower, graphics worst. A read of libaom's all-intra
speed features listed what its search skips; taken so far:
- Exact (same bytes): the chosen residuals are replayed instead of searched again before chroma,
  in the dry code and in the coding pass (-6.4% / -10.4% at qindex 140 / 32); partition
  candidates and luma depth trials stop once their decided blocks cost the best so far
  (-1.8% / -0.3%, -1.6% / -2.2%); palette maps and index bits are kept per block (-2.3% on
  the crops, -6.1% on three graphics).
- Changing output, judged at equal bytes on the crops (1% bytes is 0.097 dB at qindex 32,
  0.052 at 140) and by the AVIF gates (191/195, the same misses): one transform depth to
  effort 5 for blocks inside the picture (-11.7% for -0.044 dB at 32, -6.2% for -0.040 at
  140; blocks over the edge keep two, a 1x9000 line lost 3.2 dB without); the reduced intra
  transform set in the full search too (-3.3% for -0.014 dB).
- Rejected: modes ranked by SATD without rate (-21.6% for -0.228 dB); intra pruning in the
  full search (a grey ramp's own quality 52.25 -> 48.5 dB, gate failed); the CfL alpha search
  in scalar code (+3%, SSE2 baseline); an f64 quantizer (+11-13%, u64 to f64 has no packed
  conversion below AVX-512); a sign-bit lower bound on the rate (never prunes); skipping the
  quantizer's division where the level is 0 by a multiply-compare (+4.6% at qindex 32, +1.1%
  at 140, -1.3% on graphics: the branch mispredicts on mixed blocks; the fifth miss on that
  division).
- One more taken: above qindex 64 the full search ranks by SATD and estimates the best 6
  (-16.8% at qindex 140 for -0.065 dB; blocks over the edge rank all).
- Stage times at the defaults, 13 crops, qindex 32: ranking 47% (scoring 40%, of it the DCT
  estimate 70%), depths 19%, chroma 21%, filters 8%. Parked, refuted twice by the same gate
  item (a posterised sepia photo at Q80, 0.13 dB over its gate): a context-free rate in the
  ranking estimate (-6.9%, gate -0.66 dB) and SATD pre-ranking of angle deltas (-6.3%, gate
  -0.03 dB); cheaper ranking at qindex 64 and under needs a ranking that holds on posterised
  content. Also not worth it: an f32 quantizer in the estimate (no faster than the integer
  one), exact rate for only the best 1-2 transform types by a crude estimate (-3% for +0.6%
  bytes).
- Memory: a 1400x1400 frame (the largest single frame) peaks at 31 MB for the whole bench
  process without the filter search and 41 MB with it, about 25 MB of it the bench's own
  source; the encoder holds about 6-16 MB.
- Search time by block side at qindex 32: 8 px 22%, 16 px 29%, 32 px 26%, 64 px 13%. No 8x8
  above qindex 100 measured -37.6% at qindex 140 on the crops for -0.04 dB, yet failed four
  more AVIF gates (alpha PNG, grey ramp, HEIC, text page); a stronger whole-cost split prune
  at 16x16 changed almost nothing. Partition pruning needs a predictor (libaom's intra CNN).
- libaom's intra CNN partition prune (merged off by default, bit-identical to libaom's C):
  photos -24 to -38% for at most 0.03 dB at equal bytes, but as a default it fails AVIF gates
  at level 2 (natural 2, screen 1: a HEIC 44% larger, two flat halves -1.4 dB, sepia Q80) and
  at level 1 for all (the same plus a product on white Q50). The model learned libaom's
  partitions; our RD wants others on flat and patterned content. Parked: it needs a guard for
  those or a model of our own decisions.
- Presets against libaom (cpu-used 5, one thread, opt-level 3 everywhere, pinned; 13
  pictures at Q40 and Q75, dB at libaom's bytes over three codes): effort 4 (gates 191/195)
  1.12-2.64x libaom's time on photos at -0.08 to +0.30 dB (hills Q75 -0.71), graphics
  1.48-4.17x at +0.25 to +3.19 dB; effort 3 (CNN prune 2/1, SATD pre-ranking, no 8x8 above
  qindex 100) 0.44-0.75x on photos at -0.38 to +0.21 dB (hills Q75 -1.18), graphics
  0.45-1.12x at -0.45 to +2.77 dB. Whether effort 4 should trade gates for speed is open
  (escalation 2026-10-25, default: no).
- Hills (960x512) at Q75 is the worst photo at effort 4 (-0.71 dB at libaom's bytes): at
  qindex 68 it gets the search for big pictures; the full search there gives +0.44 dB at equal
  bytes for +68% time (+0.14 dB, +32% with SATD pre-ranking). On the crops the pre-ranking
  costs -0.062 dB at qindex 80 and -0.044 at 100 for -18%, so it stays; whether big pictures
  above qindex 64 should get the full search is a quality-for-time call, left open.
- Refuted later the same day (see the SIMD entry): contexts of a block at once are exact in a
  padded level buffer.
- Trap: the image-processing calibration in the dev profile compiles `Encoder<Rd>` at
  opt-level 0 and timed the encoder 4-5x too slow.

**2026-10-24, rate counted once.** Coefficients are written through a cached scan table with the
lo context inline (ee598570), and the chosen transform blocks' bits come from the residual
search instead of a second count in `coef_bits` (same bytes; an assert held them equal over the
unit tests and the corpus at qindex 140, 32 and 200 with RDOQ). Pinned on 13 crops: qindex 140
3940 ms, qindex 32 9907 ms (from 5978 and 21231 at the start of the speed work).
Two dead ends: a reused level buffer zeroed only where the transform reads (−0.5% and +0.1%,
the memset was not the cost), and stopping the depth search when a depth costs more than the
one above it (−3.7% and −3.3%, but −0.022 dB with 0.2% more bytes at 140 and −0.037 dB at 32).

**2026-10-24, search speed.** Pinned on 13 crops of 384 px: qindex 140 5978 → 5577 ms and
qindex 32 21231 → 17521 ms with the same or near bytes and dB: symbol costs from a table built at
compile time (same bytes), libaom's reduced intra transform-type set where the search is not
full (qindex 140 −1.8%, 0.2% fewer bytes; at 32 it lost 0.034 dB, so off there), coefficient
costs from tables filled per superblock as `av1_fill_coeff_costs` (same bytes, −4.6% and
−13%), transform types ranked by their coefficients' error with only the winner reconstructed
(−0.3% and −3.4%, ±0.006 dB). The quantizer's u64 division is the hottest line in a profile,
yet three replacements lost: a 32-bit division +6.5%, reciprocal tables rebuilt per superblock
+4.5%, reciprocal tables cached per qindex +7.5% (16 KB per transform size crowd L1). At qindex
32 a final code takes about 190 residual searches per 8×8: ranking 60, depths 46, chroma 68.
Then: split pruning on the unsplit block's whole cost (distortion alone kept dense palette
blocks whole and a chart grew 2226 → 3752 B), prune 1 at effort 4-5 (−8% at qindex 140, same
dB); the residual search's reconstruction copied instead of computed again (same bytes, −3.4%
and −6.3%). Now 4954 ms and 15740 ms, AVIF gates 190/195. A 16-wide AVX2 SATD gained 0.6%:
most SATD calls are 4 and 8 wide and stay scalar.

**2026-10-24, the gate misses on synthetic patterns.** The strip pattern of the AVIF gates
(sawtooth R, diagonal G, XOR B) loses 1.9 dB at libaom's bytes at effort 4 and gains 0.1 at
effort 9. Settings one at a time on a 1024×256 crop at Q50 (−3.20 dB): all luma or chroma
modes kept change nothing; filters off −3.76; H and V partitions at every block +2.6 dB, 4×4
blocks +1.3. The gain is at 16×16 and 8×8 (at 64 and 32 nothing), and comes from 8×4/4×8: at
effort 4 an 8×8 never splits, so the "Split beat None" rule never tries H and V there. Trying
them costs 2× time on photos for nothing (+0.03 dB always, −0.03 at 8×8 alone); a gate on
None's bits per pixel recovers the strip at 0.5 bit but costs antialiased text 3.7 dB. That is
no costing bug: the search's rate estimate is 2.5% under the coded bytes and its distortion is
the decoded picture's within 0.2 dB, with H/V or without; the extra blocks move the search
along λ, and the tune-IQ λ weight (×1.56 at qindex 133) makes that a poor trade on text.
Without the weight: text +1.5 dB at libaom's bytes, photos at Q90 −0.1 to −0.3, mean +0.24 →
+0.30 over 28 codes, bytes at a qindex up to 18% higher; kept, as libaom's tune IQ has it.
Antialiased text (grey levels 26 apart) is not monotonic in qindex: 49147 B at 48, 56810 at 54,
49321 at 60, 54919 at 64, with or without quantizer matrices, IQ filters, Variance Boost, SSIM λ
or skip prediction, and in the search's own estimate; without palettes the curve is smooth and
2.6× larger, and searching transform types at every depth fixes 54 but not 64. The quantizer
steps resonate with the level grid; the gate's Q80 lands on 54.
RDOQ does not bring grey noise near libaom's bytes (53 KB against 32 KB at qindex 144, the
same at 2, 8 and 64 coefficients); at equal bytes it is +0.2 to +1.4 dB ahead.

**2026-10-24, speed.** On 13 crops of 384 px at qindex 140, effort 4: 8535 → 5690 ms. The
chroma palette search keeps a bitmap of colour pairs and stops past 64 (857fbe94, same bytes);
effort ≤5 compares depths with DCT only and searches transform types at the chosen depth
(b982f643, 96623 B at 42.738 dB); context snapshots are a fixed struct and the end of block
comes from a cached inverse scan (a12c8720, same bytes). A 32-bit quantizer division was 6.5%
slower on a pinned A/B (6001 against 6394 ms, three rounds) and was dropped. Profile now:
`predict_plane` 24%, `write_coefs` 12%, `rd_residual` 5.5%, the scalar SATD 5%.

**2026-10-24, filters searched on the decoded picture.** The frame header is written last, so
`filters::search` decodes the coded tiles with rav1d under candidate headers: deblocking levels
per plane by steps of 8, 4, 2, 1 and off, then CDEF primary and secondary strengths for luma
and chroma, about 20 decodes. Mean at libaom's bytes +1.01 → +1.10 dB; a HEIC pattern 45.26 →
49.71 dB. Tune IQ's filter rules (no CDEF to qindex 32, sharpness 7 to 112 and 1 to 160) come
first. libaom's `predict_skip_txfm` (level 1) skips the residual search where the prediction
error is under its DC and AC limits: −4% time, same quality.

**2026-10-24, tune IQ complete for the quantizer.** `Options::tune_iq` turns on what libaom's
tune IQ changes and the encoder has: Variance Boost, the SSIM rate weighting
(`av1_set_mb_ssim_rdmult_scaling`: λ per block by the geometric mean of its 16×16 activity
factors over the picture's), quantizer matrices (all-intra luma levels, 4:4:4 chroma levels),
chroma AC delta q of min(base/2, 24) and the IQ rdmult weight. Against libaom at equal bytes
(PSNR, 10 pictures, Q30-Q90): photos −0.08 to +0.65 dB (were −1.08 to +0.95 with Variance Boost
alone; flat-area photos and dense photos needed 10-20% more bytes), graphics +1 to +8 dB except
a logo at Q80 (−0.63); mean +1.03 dB over 40 codes. The qindex meeting libaom's bytes is now
uniform over photos: about libaom's below 60, 6% finer at 148, 6% finer at 196. Still to port:
sharpness 7 with adaptive sharpness, CDEF_ADAPTIVE and the deblock and CDEF searches.

**2026-10-24, comparison with palettes.** At libaom's bytes (PSNR, 4:4:4, decoded with
filters): photos +0.11 to +1.85 dB, a gradient +3.0 to +6.0, a UI mock +4.2 to +5.5, a hard-edged
logo +4.2 to +4.4 (was −5 to −11 without palettes); mean +2.41 dB over 14 codes. Several graphics
points are extrapolated past the three qindexes measured. libaom is tuned for IQ, not PSNR, so
the JS gates still decide. Time 18–85 s against libaom's 0.1–1.6 s: speed is next.

**2026-10-24, palettes.** `palette.rs` writes what `read_pal_plane`, `read_pal_uv` and
`read_pal_indices` read: cache reuse flags, delta-coded sorted colours (raw or signed deltas for
V), and wavefront index maps with `order_palette`'s contexts. The search tries a block's own
colours when it has 2 to 8, else k-means palettes of 8 and 4 when it has up to 64, for luma as a
mode and for chroma (U, V pairs) beside the chroma modes. On a synthetic graphic: 3368 bytes at
43.8 dB without, 1336 at 70.6 dB with. The random-decision fuzzer draws palettes from a small
colour set so caches are reused.

**2026-10-24, first comparison with libaom** (image-processing `own::compare`, before
palettes): at libaom's bytes the own encoder is +0.11 to +1.83 dB on photos, a hard-edged logo
-5 to -11 dB, and 60-100x slower. Allocation-free residual search halved the time.

**2026-10-24, RD search.** Each superblock is searched dry: blocks are coded into a `Counter`
on a copy of the CDFs from the superblock's start, against snapshots of the reconstruction and
contexts, then the winning plan is coded by the same code into the range writer. Partitions
None/Split/H/V, the 13 luma modes, angle deltas of the two best directions, filter-intra and
depths; chroma modes, angles and CfL with alphas fitted per plane. Residuals try each coded
transform type through fdct, a deadzone quantizer (48/128, 64 lossless), counted rate and pixel
distortion. 473 bytes at 42.59 dB where DC prediction takes 929 at 41.83. Faults: worst
candidate, rate ignored in residuals, bottom-edge contexts, all caught. Next: compare with
libaom at equal bytes, then filters.

**2026-10-24, frame coder and forward transforms.** `Encoder<D: Decide>` codes a frame a
superblock row at a time with every partition, mode, angle, CfL, filter-intra, depth, transform
type and delta q a decider asks for; random decisions, delta q, quantizer matrices, lossless and
two tile columns decode in rav1d to the encoder's reconstruction. `fdct.rs` is libaom's forward
transforms, exact to its C code on 194 digests (handoff in `encoder-fdct-handoff.md`).

**2026-10-24, range encoder.** `Writer` ports libaom's `od_ec_enc` to rav1d's CDF layout and
adapts CDFs as `msac.rs` does. Random sequences of every symbol kind round-trip through
`MsacContext` with the x86 assembly, and the adapted CDFs match lane for lane. Faults in the
interval, the adaptation rate and the last byte each fail the tests. Two harness traps came
from the decoder's assembly, not the encoder: it loads CDFs with aligned vector loads (an
unaligned array crashes with SIGSEGV), and its adaptation works in signed 16 bits, so a
probability lane of 0, which no AV1 CDF holds, wraps. The tests keep CDFs aligned and lanes in
1..32767. Next: headers and OBUs, then the block writer for DC_PRED blocks.
