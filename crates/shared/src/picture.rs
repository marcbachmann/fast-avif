#![deny(unsafe_op_in_unsafe_fn)]
use std::hint::assert_unchecked;
use std::marker::PhantomData;
use std::ops::Range;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::ptr::NonNull;
use std::sync::Arc;
use std::{array, mem};

use zerocopy::{AsBytes, FromBytes, FromZeroes};

use crate::bitdepth::BitDepth;
use crate::disjoint_mut::{
    AsMutPtr, DisjointImmutGuard, DisjointMut, DisjointMutGuard, SliceBounds,
};
use crate::error::{Rav1dError, Rav1dResult};
use crate::headers::{
    Rav1dContentLightLevel, Rav1dFrameHeader, Rav1dITUTT35, Rav1dMasteringDisplay,
    Rav1dPixelLayout, Rav1dSequenceHeader,
};
use crate::pixels::Pixels;
use crate::pool::MemPool;
use crate::props::Rav1dDataProps;
use crate::strided::Strided;
use crate::with_offset::WithOffset;

// Number of bytes to align AND pad picture memory buffers by, so that SIMD
// implementations can over-read by a few bytes, and use aligned read/write
// instructions.
pub const RAV1D_PICTURE_ALIGNMENT: usize = 64;
// TODO(kkysen) Eventually the [`impl Default`] might not be needed.
#[derive(Clone, Default)]
#[repr(C)]
pub struct Rav1dPictureParameters {
    pub w: i32,
    pub h: i32,
    pub layout: Rav1dPixelLayout,
    pub bpc: u8,
}

#[derive(Clone, FromZeroes, FromBytes, AsBytes)]
#[repr(C, align(64))]
pub struct AlignedPixelChunk([u8; RAV1D_PICTURE_ALIGNMENT]);

const _: () = assert!(mem::align_of::<AlignedPixelChunk>() == RAV1D_PICTURE_ALIGNMENT);
const _: () = assert!(mem::size_of::<AlignedPixelChunk>() == RAV1D_PICTURE_ALIGNMENT);

/// The guaranteed length multiple of [`Rav1dPictureDataComponentInner`]s.
/// This is checked and assumed.
const RAV1D_PICTURE_GUARANTEED_MULTIPLE: usize = 64;

/// Actual [`Rav1dPictureData`]'s components should be multiples of this,
/// as [`place`] checks an allocator's layout for,
/// though wrapped buffers may only be [`RAV1D_PICTURE_GUARANTEED_MULTIPLE`].
const RAV1D_PICTURE_MULTIPLE: usize = 64 * 64;

pub struct Rav1dPictureDataComponentInner {
    /// A ptr to the start of this slice of [`BitDepth::Pixel`]s*.
    ///
    /// It is aligned to [`RAV1D_PICTURE_ALIGNMENT`].
    ptr: NonNull<u8>,

    /// The length of [`Self::ptr`] in [`u8`] bytes.
    ///
    /// It is a multiple of [`RAV1D_PICTURE_GUARANTEED_MULTIPLE`].
    len: usize,

    /// The stride of [`Self::ptr`] in [`u8`] bytes.
    stride: isize,
}

impl Rav1dPictureDataComponentInner {
    /// `len` and `stride` are in terms of [`u8`] bytes.
    ///
    /// # Safety
    ///
    /// `ptr`, `len`, and `stride` must be as [`place`] checks them.
    unsafe fn new(ptr: Option<NonNull<u8>>, len: usize, stride: isize) -> Self {
        let ptr = match ptr {
            None => {
                return Self {
                    // Ensure it is aligned enough.
                    ptr: NonNull::<AlignedPixelChunk>::dangling().cast(),
                    len: 0,
                    stride,
                };
            }
            Some(ptr) => ptr,
        };

        assert!(len != 0); // If `len` was 0, `ptr` should've been `None`.
        assert!(ptr.cast::<AlignedPixelChunk>().is_aligned());

        assert!(stride >= 0);
        // Guaranteed by `place`.
        assert!(len % RAV1D_PICTURE_MULTIPLE == 0);
        Self { ptr, len, stride }
    }

    /// # Safety
    ///
    /// As opposed to `Self::new`, this is safe because `buf` is a `&mut` and thus unique,
    /// so it is sound to further subdivide it into disjoint `&mut`s.
    pub fn wrap_buf<BD: BitDepth>(buf: &mut [BD::Pixel], stride: usize) -> Self {
        let buf = AsBytes::as_bytes_mut(buf);
        let ptr = NonNull::new(buf.as_mut_ptr()).unwrap();
        assert!(ptr.cast::<AlignedPixelChunk>().is_aligned());
        let len = buf.len();
        assert!(len % RAV1D_PICTURE_GUARANTEED_MULTIPLE == 0);
        let stride = (stride * mem::size_of::<BD::Pixel>()) as isize;
        Self { ptr, len, stride }
    }
}

// SAFETY: The pixels are memory an allocator handed out for the picture, which it takes back on
// any thread; they are only reached through the `DisjointMut` around this, whose callers borrow
// disjoint ranges (checked only with debug assertions).
unsafe impl Send for Rav1dPictureDataComponentInner {}
// SAFETY: As for `Send`: a `&Self` reads only `ptr`, `len` and `stride`.
unsafe impl Sync for Rav1dPictureDataComponentInner {}

// SAFETY: We only store the raw pointer, so we never materialize a `&mut`.
unsafe impl AsMutPtr for Rav1dPictureDataComponentInner {
    type Target = u8;

    #[inline] // Inline so callers can see the assume.
    unsafe fn as_mut_ptr(ptr: *mut Self) -> *mut Self::Target {
        // SAFETY: Safe to dereference by unsafe preconditions.
        // Since we don't store any `&mut`s, just a raw ptr, we can have a `&Self`.
        let this = unsafe { &*ptr };

        // Assume this so that the compiler knows `ptr` is aligned.
        // Normally we'd store this as a slice so the compiler would know,
        // but since it's a ptr due to `DisjointMut`, we explicitly assume it here.
        // SAFETY: We already checked this in `Self::new`.
        unsafe { assert_unchecked(this.ptr.cast::<AlignedPixelChunk>().is_aligned()) };

        this.ptr.as_ptr()
    }

    #[inline] // Inline so callers can see the assume.
    fn len(&self) -> usize {
        // SAFETY: We already checked this in `Self::new`.
        unsafe { assert_unchecked(self.len % RAV1D_PICTURE_GUARANTEED_MULTIPLE == 0) };
        self.len
    }
}

pub struct Rav1dPictureDataComponent(DisjointMut<Rav1dPictureDataComponentInner>);

impl Pixels for Rav1dPictureDataComponent {
    #[inline]
    fn byte_len(&self) -> usize {
        self.0.len()
    }

    #[inline]
    fn as_byte_mut_ptr(&self) -> *mut u8 {
        self.0.as_mut_ptr()
    }
}

impl Strided for Rav1dPictureDataComponent {
    fn stride(&self) -> isize {
        // SAFETY: We're only accessing the `stride` fields, not `ptr`.
        unsafe { (*self.0.inner()).stride }
    }
}

impl Rav1dPictureDataComponent {
    pub fn wrap_buf<BD: BitDepth>(buf: &mut [BD::Pixel], stride: usize) -> Self {
        Self(DisjointMut::new(
            Rav1dPictureDataComponentInner::wrap_buf::<BD>(buf, stride),
        ))
    }

    pub fn with_offset(&self) -> Rav1dPictureDataComponentOffset<'_> {
        Rav1dPictureDataComponentOffset {
            data: self,
            offset: 0,
        }
    }

    #[inline]
    fn as_strided_byte_mut_ptr(&self) -> *mut u8 {
        self.0.as_mut_ptr()
    }

    /// Strided ptr to [`BitDepth::Pixel`]s.
    pub fn as_strided_mut_ptr<BD: BitDepth>(&self) -> *mut BD::Pixel {
        // SAFETY: Transmutation is safe because we verify this with `zerocopy` in `Self::slice`.
        self.as_strided_byte_mut_ptr().cast()
    }

    /// Strided ptr to [`BitDepth::Pixel`]s.
    pub fn as_strided_ptr<BD: BitDepth>(&self) -> *const BD::Pixel {
        self.as_strided_mut_ptr::<BD>().cast_const()
    }

    #[doc(hidden)]
    /// The first byte of the plane, null where the component has none; the rows are the
    /// stride apart.
    pub fn as_ptr(&self) -> *const u8 {
        if self.byte_len() == 0 {
            std::ptr::null()
        } else {
            self.as_strided_byte_mut_ptr().cast_const()
        }
    }

    /// The component's bytes, for [`Self::put_bytes`].
    #[inline]
    pub fn bytes(&self) -> Vec<u8> {
        self.0.index(..).to_vec()
    }

    #[inline]
    pub fn byte_slice(&self) -> DisjointImmutGuard<'_, Rav1dPictureDataComponentInner, [u8]> {
        self.0.index(..)
    }

    #[inline]
    pub fn put_bytes(&self, bytes: &[u8]) {
        self.0.index_mut(..).copy_from_slice(bytes);
    }

    pub fn copy_from(&self, src: &Self) {
        let dst = &mut *self.0.index_mut(..);
        let src = &*src.0.index(..);
        dst.clone_from_slice(src);
    }

    #[inline] // Inline to see bounds checks in order to potentially elide them.
    #[cfg_attr(debug_assertions, track_caller)]
    pub fn index<'a, BD: BitDepth>(
        &'a self,
        index: usize,
    ) -> DisjointImmutGuard<'a, Rav1dPictureDataComponentInner, BD::Pixel> {
        self.0.element_as(index)
    }

    #[inline] // Inline to see bounds checks in order to potentially elide them.
    #[cfg_attr(debug_assertions, track_caller)]
    pub fn index_mut<'a, BD: BitDepth>(
        &'a self,
        index: usize,
    ) -> DisjointMutGuard<'a, Rav1dPictureDataComponentInner, BD::Pixel> {
        self.0.mut_element_as(index)
    }

    #[inline] // Inline to see bounds checks in order to potentially elide them.
    #[cfg_attr(debug_assertions, track_caller)]
    pub fn slice<'a, BD, I>(
        &'a self,
        index: I,
    ) -> DisjointImmutGuard<'a, Rav1dPictureDataComponentInner, [BD::Pixel]>
    where
        BD: BitDepth,
        I: SliceBounds,
    {
        self.0.slice_as(index)
    }

    #[inline] // Inline to see bounds checks in order to potentially elide them.
    #[cfg_attr(debug_assertions, track_caller)]
    pub fn slice_mut<'a, BD, I>(
        &'a self,
        index: I,
    ) -> DisjointMutGuard<'a, Rav1dPictureDataComponentInner, [BD::Pixel]>
    where
        BD: BitDepth,
        I: SliceBounds,
    {
        self.0.mut_slice_as(index)
    }
}

pub type Rav1dPictureDataComponentOffset<'a> = WithOffset<&'a Rav1dPictureDataComponent>;

impl<'a> Rav1dPictureDataComponentOffset<'a> {
    #[inline] // Inline to see bounds checks in order to potentially elide them.
    #[cfg_attr(debug_assertions, track_caller)]
    pub fn index<BD: BitDepth>(
        &self,
    ) -> DisjointImmutGuard<'a, Rav1dPictureDataComponentInner, BD::Pixel> {
        self.data.index::<BD>(self.offset)
    }

    #[inline] // Inline to see bounds checks in order to potentially elide them.
    #[cfg_attr(debug_assertions, track_caller)]
    pub fn index_mut<BD: BitDepth>(
        &self,
    ) -> DisjointMutGuard<'a, Rav1dPictureDataComponentInner, BD::Pixel> {
        self.data.index_mut::<BD>(self.offset)
    }

    #[inline] // Inline to see bounds checks in order to potentially elide them.
    #[cfg_attr(debug_assertions, track_caller)]
    pub fn slice<BD: BitDepth>(
        &self,
        len: usize,
    ) -> DisjointImmutGuard<'a, Rav1dPictureDataComponentInner, [BD::Pixel]> {
        self.data.slice::<BD, _>((self.offset.., ..len))
    }

    #[inline] // Inline to see bounds checks in order to potentially elide them.
    #[cfg_attr(debug_assertions, track_caller)]
    pub fn slice_mut<BD: BitDepth>(
        &self,
        len: usize,
    ) -> DisjointMutGuard<'a, Rav1dPictureDataComponentInner, [BD::Pixel]> {
        self.data.slice_mut::<BD, _>((self.offset.., ..len))
    }

    /// The block of `rows` and `cols` around this offset, in pixels, for [`PicBlock`]'s rows.
    /// Panics unless the footprint lies in the component or is empty.
    #[inline]
    #[cfg_attr(debug_assertions, track_caller)]
    pub fn block<BD: BitDepth>(self, rows: Range<isize>, cols: Range<isize>) -> PicBlock<'a, BD> {
        self.block_of(rows, cols)
    }

    /// [`Self::block`] whose rows skip their release check against the footprint's pixels.
    ///
    /// # Safety
    ///
    /// Every row borrowed from the block lies in its footprint: `y` in `rows`, `x..x + n` in
    /// `cols`. Debug builds check this on every row.
    #[inline]
    #[cfg_attr(debug_assertions, track_caller)]
    pub unsafe fn block_unchecked<BD: BitDepth>(
        self,
        rows: Range<isize>,
        cols: Range<isize>,
    ) -> PicRows<'a, BD> {
        self.block_of(rows, cols)
    }

    #[inline]
    #[cfg_attr(debug_assertions, track_caller)]
    fn block_of<BD: BitDepth, const CHECKED: bool>(
        self,
        rows: Range<isize>,
        cols: Range<isize>,
    ) -> PicBlock<'a, BD, CHECKED> {
        #[inline(never)]
        #[cfg_attr(debug_assertions, track_caller)]
        fn out_of_bounds(rows: Range<isize>, cols: Range<isize>, at: usize, len: usize) -> ! {
            panic!("block rows {rows:?} columns {cols:?} at pixel offset {at} out of range for pixel length {len}");
        }

        /// Where row `y`'s columns start; `offset` wraps below 0 as `WithOffset`'s arithmetic does.
        fn row_start(offset: usize, stride: isize, y: isize, x: isize) -> Option<isize> {
            (offset as isize)
                .checked_add(y.checked_mul(stride)?)?
                .checked_add(x)
        }

        let stride = self.pixel_stride::<BD>();
        let len = self.data.pixel_len::<BD>();
        // Row starts are linear in `y`, so the first and the last row bound all the others.
        // `(lo, hi)`: the pixels from the first row's start to the last row's end.
        let range = if rows.is_empty() || cols.is_empty() {
            Some((self.offset as isize, self.offset as isize))
        } else {
            match (
                row_start(self.offset, stride, rows.start, cols.start),
                row_start(self.offset, stride, rows.end - 1, cols.start),
                cols.end.checked_sub(cols.start),
            ) {
                (Some(a), Some(b), Some(w)) => a
                    .max(b)
                    .checked_add(w)
                    .filter(|&end| a.min(b) >= 0 && end as usize <= len)
                    .map(|end| (a.min(b), end)),
                _ => None,
            }
        };
        let Some((lo, hi)) = range else {
            out_of_bounds(rows, cols, self.offset, len);
        };
        #[cfg(debug_assertions)]
        let _ = (lo, hi);
        PicBlock {
            #[cfg(debug_assertions)]
            at: self,
            #[cfg(not(debug_assertions))]
            origin: self.data.as_mut_ptr::<BD>().wrapping_add(self.offset),
            #[cfg(not(debug_assertions))]
            lo: lo.wrapping_sub(self.offset as isize),
            #[cfg(not(debug_assertions))]
            span: (hi - lo) as usize,
            stride,
            rows: (rows.start, rows.end),
            cols: (cols.start, cols.end),
            phantom: PhantomData,
        }
    }
}

/// The rows of a block in a picture component, its whole footprint checked once by
/// [`Rav1dPictureDataComponentOffset::block`] to lie in the component's allocation.
///
/// Release builds then check a row with one compare, against the pixels from the footprint's
/// first row to its last, so a row never leaves the allocation; without `CHECKED`
/// ([`Rav1dPictureDataComponentOffset::block_unchecked`]) not even that. Like
/// [`DisjointMut::index_mut`] in release, the rest relies on correct use, which only debug
/// builds check:
/// - the `n` pixels at `(x, y)` lie in the footprint: `y` in its rows, `x..x + n` in its
///   columns. Kernels declare the footprint from the block size they loop over.
/// - a borrowed row overlaps no mutable borrow of the same pixels while held.
///
/// With `debug_assertions` each row is checked against the footprint and borrowed on its own
/// through the [`DisjointMut`], so its tracker sees one row at a time, never the whole block.
#[derive(Clone, Copy)]
pub struct PicBlock<'a, BD: BitDepth, const CHECKED: bool = true> {
    #[cfg(debug_assertions)]
    at: Rav1dPictureDataComponentOffset<'a>,
    #[cfg(not(debug_assertions))]
    /// Pixel `(0, 0)` of the block; only the footprint is known to be in bounds.
    origin: *mut BD::Pixel,
    /// The footprint's pixels in the allocation, `origin + lo..origin + lo + span`.
    #[cfg(not(debug_assertions))]
    lo: isize,
    #[cfg(not(debug_assertions))]
    span: usize,
    stride: isize,
    rows: (isize, isize),
    cols: (isize, isize),
    phantom: PhantomData<(&'a Rav1dPictureDataComponent, BD)>,
}

/// A [`PicBlock`] whose rows the kernel keeps in the footprint (checked in debug builds only).
pub type PicRows<'a, BD> = PicBlock<'a, BD, false>;

#[cfg(not(debug_assertions))]
#[inline(never)]
#[cold]
fn row_out_of_bounds() -> ! {
    panic!("row out of its block's pixels");
}

#[cfg(debug_assertions)]
pub type PicRow<'a, BD> =
    DisjointImmutGuard<'a, Rav1dPictureDataComponentInner, [<BD as BitDepth>::Pixel]>;
#[cfg(not(debug_assertions))]
pub type PicRow<'a, BD> = &'a [<BD as BitDepth>::Pixel];

#[cfg(debug_assertions)]
pub type PicRowMut<'a, BD> =
    DisjointMutGuard<'a, Rav1dPictureDataComponentInner, [<BD as BitDepth>::Pixel]>;
#[cfg(not(debug_assertions))]
pub type PicRowMut<'a, BD> = &'a mut [<BD as BitDepth>::Pixel];

impl<'a, BD: BitDepth, const CHECKED: bool> PicBlock<'a, BD, CHECKED> {
    /// The offset of `n` pixels at `(x, y)` from pixel `(0, 0)`.
    #[inline(always)]
    #[cfg_attr(debug_assertions, track_caller)]
    fn at(&self, y: isize, x: isize, n: usize) -> isize {
        let ((top, bottom), (left, right)) = (self.rows, self.cols);
        let inside = y >= top && y < bottom && x >= left && x <= right && n <= (right - x) as usize;
        debug_assert!(
            inside,
            "{n} pixels at ({x}, {y}) outside block rows {top}..{bottom} columns {left}..{right}"
        );
        let i = y.wrapping_mul(self.stride).wrapping_add(x);
        #[cfg(not(debug_assertions))]
        if CHECKED
            && (i.wrapping_sub(self.lo) as usize > self.span.wrapping_sub(n) || n > self.span)
        {
            row_out_of_bounds();
        }
        i
    }

    /// `n` pixels of row `y` from column `x`.
    #[inline(always)]
    #[cfg_attr(debug_assertions, track_caller)]
    pub fn row(&self, y: isize, x: isize, n: usize) -> PicRow<'a, BD> {
        let i = self.at(y, x, n);
        #[cfg(debug_assertions)]
        return (self.at + i).slice::<BD>(n);
        // SAFETY: `block` checked that the footprint's pixels lie in the allocation, and `at`
        // that `i`'s `n` pixels lie in the footprint's, or `block_unchecked`'s caller that
        // they lie in the footprint.
        #[cfg(not(debug_assertions))]
        unsafe {
            std::slice::from_raw_parts(self.origin.wrapping_offset(i), n)
        }
    }

    /// `n` pixels of row `y` from column `x`, to write.
    #[inline(always)]
    #[cfg_attr(debug_assertions, track_caller)]
    pub fn row_mut(&self, y: isize, x: isize, n: usize) -> PicRowMut<'a, BD> {
        let i = self.at(y, x, n);
        #[cfg(debug_assertions)]
        return (self.at + i).slice_mut::<BD>(n);
        // SAFETY: As in `row`; no other borrow of these pixels is held (`DisjointMut`'s rule).
        #[cfg(not(debug_assertions))]
        unsafe {
            std::slice::from_raw_parts_mut(self.origin.wrapping_offset(i), n)
        }
    }
}

/// The planes of a picture and the memory they sit in, which goes back to its allocator when
/// the last picture of it drops.
pub struct Rav1dPictureData {
    pub data: [Rav1dPictureDataComponent; 3],
    #[doc(hidden)]
    pub memory: HeldMemory,
    /// The allocator of a copy of the picture.
    #[doc(hidden)]
    pub allocator: Rav1dPicAllocator,
}

/// A picture's memory behind a raw pointer, as its planes point into it. A picture drops on any
/// thread and inside the decoder, so a panic in the memory's `Drop` stays here.
pub struct HeldMemory(NonNull<PictureMemory>);

// SAFETY: `PictureMemory` is `Send + Sync`, and this is its only owner.
unsafe impl Send for HeldMemory {}
// SAFETY: As above; a `&HeldMemory` gives no access to the memory.
unsafe impl Sync for HeldMemory {}

impl Drop for HeldMemory {
    fn drop(&mut self) {
        // SAFETY: From `Box::leak` in `alloc_picture_data`, and dropped only here.
        let memory = unsafe { Box::from_raw(self.0.as_ptr()) };
        let _ = catch_unwind(AssertUnwindSafe(|| drop(memory)));
    }
}

// TODO(kkysen) Eventually the [`impl Default`] might not be needed.
// It's needed currently for a [`mem::take`] that simulates a move,
// but once everything is Rusty, we may not need to clear the `dst` anymore.
// This also applies to the `#[derive(Default)]`
// on [`Rav1dPictureParameters`] and [`Rav1dPixelLayout`].
#[derive(Clone, Default)]
#[repr(C)]
pub struct Rav1dPicture {
    pub seq_hdr: Option<Arc<Rav1dSequenceHeader>>,
    pub frame_hdr: Option<Arc<Rav1dFrameHeader>>,
    pub data: Option<Arc<Rav1dPictureData>>,
    pub stride: [isize; 2],
    pub p: Rav1dPictureParameters,
    pub m: Rav1dDataProps,
    pub content_light: Option<Arc<Rav1dContentLightLevel>>,
    pub mastering_display: Option<Arc<Rav1dMasteringDisplay>>,
    pub itut_t35: Arc<Box<[Rav1dITUTT35]>>,
}

impl Rav1dPicture {
    pub fn lf_offsets<BD: BitDepth>(&self, y: i32) -> [Rav1dPictureDataComponentOffset<'_>; 3] {
        // Init loopfilter offsets. Point the chroma offsets in 4:0:0 to the luma
        // plane here to avoid having additional in-loop branches in various places.
        // We never use those values, so it doesn't really matter what they point
        // at, as long as the offsets are valid.
        let layout = self.p.layout;
        let has_chroma = layout != Rav1dPixelLayout::I400;
        let data = &self.data.as_ref().unwrap().data;
        array::from_fn(|i| {
            let data = &data[has_chroma as usize * i];
            let ss_ver = layout == Rav1dPixelLayout::I420 && i != 0;
            data.with_offset() + (y as isize * data.pixel_stride::<BD>() >> ss_ver as u8)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::align::AlignedVec64;
    use crate::bitdepth::{BitDepth16, BitDepth8};

    const S: usize = 64;
    const H: usize = 6;

    /// `H` rows of `S` pixels, each pixel its index.
    fn plane<BD: BitDepth>() -> AlignedVec64<BD::Pixel> {
        let mut buf = AlignedVec64::<BD::Pixel>::new();
        buf.resize(S * H, 0.into());
        for (i, p) in buf.as_mut_slice().iter_mut().enumerate() {
            *p = (i as u8).into();
        }
        buf
    }

    fn rows_match<BD: BitDepth>(
        b: &PicBlock<BD>,
        buf: &[BD::Pixel],
        at: usize,
        rows: Range<isize>,
        cols: Range<isize>,
    ) {
        for y in rows {
            let n = (cols.end - cols.start) as usize;
            let i = at as isize + y * S as isize + cols.start;
            assert!(*b.row(y, cols.start, n) == buf[i as usize..][..n]);
        }
    }

    fn edges<BD: BitDepth>() {
        let mut buf = plane::<BD>();
        let want = buf.as_slice().to_vec();
        let pic = Rav1dPictureDataComponent::wrap_buf::<BD>(buf.as_mut_slice(), S);
        let at = 3 * S + 3;
        let all = (-3..H as isize - 3, -3..S as isize - 3);
        let b = (pic.with_offset() + at).block::<BD>(all.0.clone(), all.1.clone());
        rows_match(&b, &want, at, all.0, all.1);
        let last = (pic.with_offset() + (S * H - 1)).block::<BD>(-1..1, -1..1);
        assert!(last.row(0, 0, 1)[0] == want[S * H - 1]);
        let first = pic.with_offset().block::<BD>(0..1, 0..1);
        assert!(first.row(0, 0, 1)[0] == want[0]);
    }

    #[test]
    fn block_at_the_edges() {
        edges::<BitDepth8>();
        edges::<BitDepth16>();
    }

    #[test]
    fn block_empty() {
        let mut buf = plane::<BitDepth8>();
        let pic = Rav1dPictureDataComponent::wrap_buf::<BitDepth8>(buf.as_mut_slice(), S);
        pic.with_offset().block::<BitDepth8>(-9..-9, 0..4);
    }

    fn refused(rows: Range<isize>, cols: Range<isize>) {
        let mut buf = plane::<BitDepth16>();
        let pic = Rav1dPictureDataComponent::wrap_buf::<BitDepth16>(buf.as_mut_slice(), S);
        (pic.with_offset() + (3 * S + 3)).block::<BitDepth16>(rows, cols);
    }

    #[test]
    #[should_panic(expected = "out of range")]
    fn block_one_past_the_end() {
        refused(-3..H as isize - 3, -3..S as isize - 2);
    }

    #[test]
    #[should_panic(expected = "out of range")]
    fn block_one_row_below() {
        refused(-3..H as isize - 2, 0..1);
    }

    #[test]
    #[should_panic(expected = "out of range")]
    fn block_one_before_the_start() {
        refused(-3..0, -4..0);
    }

    #[test]
    #[should_panic(expected = "out of range")]
    fn block_one_row_above() {
        refused(-4..0, 0..1);
    }

    /// Rows are borrowed one at a time: another borrow may hold the row between them.
    #[test]
    fn block_rows_borrowed_alone() {
        let mut buf = plane::<BitDepth8>();
        let pic = Rav1dPictureDataComponent::wrap_buf::<BitDepth8>(buf.as_mut_slice(), S);
        let b = pic.with_offset().block::<BitDepth8>(0..3, 0..8);
        let _middle = pic
            .with_offset()
            .block::<BitDepth8>(1..2, 0..8)
            .row_mut(1, 0, 8);
        let (_a, _c) = (b.row(0, 0, 8), b.row_mut(2, 0, 8));
    }

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "overlapping DisjointMut")]
    fn block_rows_overlap_tracked() {
        let mut buf = plane::<BitDepth8>();
        let pic = Rav1dPictureDataComponent::wrap_buf::<BitDepth8>(buf.as_mut_slice(), S);
        let b = pic.with_offset().block::<BitDepth8>(0..2, 0..8);
        let _w = b.row_mut(1, 0, 8);
        let _r = b.row(1, 4, 4);
    }

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "outside block")]
    fn block_row_outside_the_footprint() {
        let mut buf = plane::<BitDepth8>();
        let pic = Rav1dPictureDataComponent::wrap_buf::<BitDepth8>(buf.as_mut_slice(), S);
        let b = pic.with_offset().block::<BitDepth8>(0..2, 0..8);
        let _ = b.row(0, 1, 8);
    }

    #[cfg(not(debug_assertions))]
    #[test]
    #[should_panic(expected = "row out of its block's pixels")]
    fn block_row_past_its_pixels_refused_in_release() {
        let mut buf = plane::<BitDepth8>();
        let pic = Rav1dPictureDataComponent::wrap_buf::<BitDepth8>(buf.as_mut_slice(), S);
        let b = pic.with_offset().block::<BitDepth8>(0..1, 0..8);
        let _ = b.row(H as isize, 0, 8);
    }

    #[cfg(not(debug_assertions))]
    #[test]
    #[should_panic(expected = "row out of its block's pixels")]
    fn block_row_before_its_pixels_refused_in_release() {
        let mut buf = plane::<BitDepth8>();
        let pic = Rav1dPictureDataComponent::wrap_buf::<BitDepth8>(buf.as_mut_slice(), S);
        let b = (pic.with_offset() + S).block::<BitDepth8>(0..1, 0..8);
        let _ = b.row(-1, 0, 8);
    }
}

/// The size and format of a picture to allocate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PictureParameters {
    pub width: u32,
    pub height: u32,
    /// 8, 10 or 12; samples above 8 bits take 2 bytes.
    pub bpc: u8,
    pub layout: Rav1dPixelLayout,
}

/// Where a picture's planes lie in its memory, in bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlaneLayout {
    /// Of Y, U and V from the start of the memory; U and V are ignored for [`Rav1dPixelLayout::I400`].
    pub offsets: [usize; 3],
    /// Of Y, and of U and V.
    pub strides: [usize; 2],
    /// The memory's length.
    pub len: usize,
}

impl PictureParameters {
    fn subsampling(&self) -> (usize, usize) {
        (
            (self.layout != Rav1dPixelLayout::I444) as usize,
            (self.layout == Rav1dPixelLayout::I420) as usize,
        )
    }

    /// The planes `0..planes()` in memory: Y alone for [`Rav1dPixelLayout::I400`].
    pub fn planes(&self) -> usize {
        if self.layout == Rav1dPixelLayout::I400 {
            1
        } else {
            3
        }
    }

    /// The bytes of plane `i` with rows `stride` apart: its height rounded up to a multiple of
    /// 128 luma rows.
    pub fn plane_len(&self, i: usize, stride: usize) -> Option<usize> {
        let ss_ver = if i > 0 { self.subsampling().1 } else { 0 };
        stride.checked_mul((self.height as usize).next_multiple_of(128) >> ss_ver)
    }

    /// The smallest stride of plane `i`: its width rounded up to a multiple of 128 luma samples.
    pub fn min_stride(&self, i: usize) -> usize {
        let ss_hor = if i > 0 { self.subsampling().0 } else { 0 };
        ((self.width as usize).next_multiple_of(128) << (self.bpc > 8) as usize) >> ss_hor
    }

    /// The planes laid out as rav1d's own allocator lays them out: each at its
    /// [`PictureParameters::min_stride`], plus [`RAV1D_PICTURE_ALIGNMENT`] where that is a multiple
    /// of 1024 bytes (for the caches' sake), Y, U and V back to back, and
    /// [`RAV1D_PICTURE_ALIGNMENT`] bytes after the last.
    pub fn plane_layout(&self) -> Option<PlaneLayout> {
        let pad = |stride: usize| {
            if stride % 1024 == 0 {
                stride + RAV1D_PICTURE_ALIGNMENT
            } else {
                stride
            }
        };
        let strides = [pad(self.min_stride(0)), pad(self.min_stride(1))];
        let y = self.plane_len(0, strides[0])?;
        let uv = if self.planes() > 1 {
            self.plane_len(1, strides[1])?
        } else {
            0
        };
        Some(PlaneLayout {
            offsets: [0, y, y.checked_add(uv)?],
            strides,
            len: y
                .checked_add(uv.checked_mul(2)?)?
                .checked_add(RAV1D_PICTURE_ALIGNMENT)?,
        })
    }
}

/// The memory of one picture, and where its planes lie in it.
pub struct PictureBuffer {
    /// Borrowed mutably once, to decode into, and then dropped once the last picture of it is,
    /// on whichever thread drops that picture.
    pub memory: PictureMemory,
    pub layout: PlaneLayout,
}

pub type PictureMemory = Box<dyn AsMut<[u8]> + Send + Sync>;

/// Allocates the memory of the pictures a decoder decodes into, in place of rav1d's own pool.
///
/// rav1d checks each buffer and fails the frame with [`Rav1dError::InvalidArgument`] unless each of
/// the [`PictureParameters::planes`] starts at an address that is a multiple of
/// [`RAV1D_PICTURE_ALIGNMENT`], has a stride that is a multiple of it and at least
/// [`PictureParameters::min_stride`], takes [`PictureParameters::plane_len`] bytes that overlap no
/// other plane's, and ends at least [`RAV1D_PICTURE_ALIGNMENT`] bytes before the memory does (SIMD
/// reads that far past it). [`PictureParameters::plane_layout`] is what rav1d's own allocator uses.
///
/// Called from the threads that decode; a panic fails the frame.
pub trait PictureAllocator: Send + Sync + 'static {
    fn alloc(&self, picture: &PictureParameters) -> Result<PictureBuffer, Rav1dError>;
}

/// Each decoder's own allocator unless the caller gave one: a pool of buffers of its pictures.
#[derive(Clone)]
pub struct Rav1dPicAllocator(pub Arc<dyn PictureAllocator>);

impl Default for Rav1dPicAllocator {
    fn default() -> Self {
        Self(Arc::new(PoolAllocator(Default::default())))
    }
}

struct PoolAllocator(Arc<MemPool<u8>>);

struct PoolBuf {
    pool: Arc<MemPool<u8>>,
    buf: Vec<u8>,
}

impl AsMut<[u8]> for PoolBuf {
    fn as_mut(&mut self) -> &mut [u8] {
        &mut self.buf
    }
}

impl Drop for PoolBuf {
    fn drop(&mut self) {
        self.pool.push(mem::take(&mut self.buf));
    }
}

impl PictureAllocator for PoolAllocator {
    fn alloc(&self, picture: &PictureParameters) -> Result<PictureBuffer, Rav1dError> {
        let mut layout = picture.plane_layout().ok_or(Rav1dError::OutOfMemory)?;
        let buf = self.0.pop_init(
            layout
                .len
                .checked_add(RAV1D_PICTURE_ALIGNMENT)
                .ok_or(Rav1dError::OutOfMemory)?,
            0,
        );
        let skip = buf.as_ptr().align_offset(RAV1D_PICTURE_ALIGNMENT);
        layout.offsets = layout.offsets.map(|o| o + skip);
        layout.len = buf.len();
        Ok(PictureBuffer {
            memory: Box::new(PoolBuf {
                pool: self.0.clone(),
                buf,
            }),
            layout,
        })
    }
}

/// The plane pointers and strides of `buffer`, checked against what [`PictureAllocator`] promises.
fn place(
    picture: &PictureParameters,
    buffer: &mut [u8],
    layout: &PlaneLayout,
) -> Result<([Option<NonNull<u8>>; 3], [isize; 2]), Rav1dError> {
    let invalid = Rav1dError::InvalidArgument;
    let base = buffer.as_mut_ptr();
    let mut planes = [(0, 0); 3];
    let mut data = [None; 3];
    for i in 0..picture.planes() {
        let (offset, stride) = (layout.offsets[i], layout.strides[i.min(1)]);
        let len = picture.plane_len(i, stride).ok_or(invalid)?;
        let end = offset.checked_add(len).ok_or(invalid)?;
        let aligned = (base as usize).wrapping_add(offset) % RAV1D_PICTURE_ALIGNMENT == 0;
        if !aligned
            || stride % RAV1D_PICTURE_ALIGNMENT != 0
            || stride < picture.min_stride(i)
            || stride > isize::MAX as usize
            || len == 0
            || end.checked_add(RAV1D_PICTURE_ALIGNMENT).ok_or(invalid)? > buffer.len()
            || planes[..i].iter().any(|&(s, e)| offset < e && s < end)
        {
            return Err(invalid);
        }
        planes[i] = (offset, end);
        // SAFETY: `offset < buffer.len()`.
        data[i] = NonNull::new(unsafe { base.add(offset) });
    }
    let chroma = if picture.planes() > 1 {
        layout.strides[1] as isize
    } else {
        0
    };
    Ok((data, [layout.strides[0] as isize, chroma]))
}

impl Rav1dPictureParameters {
    pub fn pic_len(&self, [y_stride, uv_stride]: [isize; 2]) -> [usize; 2] {
        let ss_ver = (self.layout == Rav1dPixelLayout::I420) as u8;
        let aligned_h = self.h as usize + 127 & !127;
        let y_sz = y_stride as usize * aligned_h;
        let uv_sz = uv_stride as usize * (aligned_h >> ss_ver);
        [y_sz, uv_sz]
    }
}

/// A picture of `w` by `h` under the headers, its planes allocated by `allocator`. A panic in the
/// allocator or in its memory is an allocation failure.
pub fn alloc_picture_data(
    allocator: &Rav1dPicAllocator,
    w: i32,
    h: i32,
    seq_hdr: Arc<Rav1dSequenceHeader>,
    frame_hdr: Option<Arc<Rav1dFrameHeader>>,
) -> Rav1dResult<Rav1dPicture> {
    let p = Rav1dPictureParameters {
        w,
        h,
        layout: seq_hdr.layout,
        bpc: 8 + 2 * seq_hdr.hbd,
    };
    let picture = PictureParameters {
        width: w as u32,
        height: h as u32,
        bpc: p.bpc as u8,
        layout: p.layout,
    };
    let Ok(Ok(PictureBuffer { memory, layout })) =
        catch_unwind(AssertUnwindSafe(|| allocator.0.alloc(&picture)))
    else {
        return Err(Rav1dError::OutOfMemory);
    };
    let memory = NonNull::from(Box::leak(Box::new(memory)));
    let held = HeldMemory(memory);
    // SAFETY: `memory` is valid and unaliased until `held` drops; nothing else reads it here.
    let Ok(bytes) = catch_unwind(AssertUnwindSafe(|| unsafe { (**memory.as_ptr()).as_mut() }))
    else {
        return Err(Rav1dError::OutOfMemory);
    };
    let (data, stride) = place(&picture, bytes, &layout)?;
    let len = p.pic_len(stride);
    let data = array::from_fn(|i| {
        let plane = (i != 0) as usize;
        // SAFETY: `place` checked the pointer, length and stride against the memory.
        let component =
            unsafe { Rav1dPictureDataComponentInner::new(data[i], len[plane], stride[plane]) };
        Rav1dPictureDataComponent(DisjointMut::new(component))
    });
    Ok(Rav1dPicture {
        p,
        seq_hdr: Some(seq_hdr),
        frame_hdr,
        stride,
        data: Some(Arc::new(Rav1dPictureData {
            data,
            memory: held,
            allocator: allocator.clone(),
        })),
        ..Default::default()
    })
}

impl Rav1dPicture {
    #[doc(hidden)]
    pub fn has_grain(&self) -> bool {
        self.frame_hdr.as_ref().unwrap().film_grain.data.has_grain()
    }
}
