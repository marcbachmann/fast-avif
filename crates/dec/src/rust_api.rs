// This whole module was originally copied from https://github.com/rust-av/dav1d-rs/
// (specifically https://github.com/rust-av/dav1d-rs/blob/94b1deaa1e25bf29c77bb5cc8a08ddaf7663eede/src/lib.rs)
// with modifications.
// `dav1d-rs` is under the MIT license, replicated here:
//
// MIT License
//
// Copyright (c) 2018 Luca Barbato
//
// Permission is hereby granted, free of charge, to any person obtaining a copy
// of this software and associated documentation files (the "Software"), to deal
// in the Software without restriction, including without limitation the rights
// to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
// copies of the Software, and to permit persons to whom the Software is
// furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in all
// copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
// IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
// OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
// SOFTWARE.

use std::fmt;
use std::fmt::{Debug, Formatter};
use std::sync::Arc;

pub use av_data::pixel;
use rav1d_shared::bitdepth::BitDepth8;
use rav1d_shared::error::Rav1dError;
pub use rav1d_shared::headers::{
    Rav1dAdaptiveBoolean as AdaptiveBoolean, Rav1dContentLightLevel as ContentLightLevel,
    Rav1dMasteringDisplay as MasteringDisplay, Rav1dPixelLayout as PixelLayout,
    Rav1dSequenceHeader as SequenceHeader,
    Rav1dSequenceHeaderOperatingPoint as SequenceHeaderOperatingPoint,
};
use rav1d_shared::in_range::InRange;
pub use rav1d_shared::picture::{
    PictureAllocator, PictureBuffer, PictureParameters, PlaneLayout,
    RAV1D_PICTURE_ALIGNMENT as PICTURE_ALIGNMENT,
};
use rav1d_shared::picture::{Rav1dPicAllocator, Rav1dPicture};
use rav1d_shared::shared_bytes::SharedBytes;

use crate::decode::rav1d_decode_rows;
use crate::internal::Rav1dContext;
use crate::obu::rav1d_parse_sequence_header;
pub use crate::settings::{
    Rav1dDecodeFrameType as DecodeFrameType, Rav1dInloopFilterType as InloopFilterType,
};
pub use crate::Spawner;
use crate::{
    rav1d_close, rav1d_flush, rav1d_get_frame_delay, rav1d_get_picture, rav1d_open,
    rav1d_send_data, Rav1dData, Rav1dSettings,
};

/// Settings for creating a new [`Decoder`] instance.
#[derive(Default)]
pub struct Settings {
    pub(crate) inner: Rav1dSettings,
}

static_assertions::assert_impl_all!(Settings: Send, Sync);

impl Settings {
    /// Creates a new [`Settings`] instance with default settings.
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_n_threads(&mut self, n_threads: u32) {
        self.inner.n_threads = InRange::new(n_threads.try_into().unwrap()).unwrap();
    }

    pub fn get_n_threads(&self) -> u32 {
        self.inner.n_threads.get() as u32
    }

    pub fn set_max_frame_delay(&mut self, max_frame_delay: u32) {
        self.inner.max_frame_delay = InRange::new(max_frame_delay.try_into().unwrap()).unwrap();
    }

    pub fn get_max_frame_delay(&self) -> u32 {
        self.inner.max_frame_delay.get() as u32
    }

    pub fn set_apply_grain(&mut self, apply_grain: bool) {
        self.inner.apply_grain = apply_grain;
    }

    pub fn get_apply_grain(&self) -> bool {
        self.inner.apply_grain
    }

    pub fn set_operating_point(&mut self, operating_point: u8) {
        self.inner.operating_point = InRange::new(operating_point).unwrap();
    }

    pub fn get_operating_point(&self) -> u8 {
        self.inner.operating_point.get()
    }

    pub fn set_all_layers(&mut self, all_layers: bool) {
        self.inner.all_layers = all_layers;
    }

    pub fn get_all_layers(&self) -> bool {
        self.inner.all_layers
    }

    pub fn set_frame_size_limit(&mut self, frame_size_limit: u32) {
        self.inner.frame_size_limit = frame_size_limit;
    }

    pub fn get_frame_size_limit(&self) -> u32 {
        self.inner.frame_size_limit
    }

    pub fn set_strict_std_compliance(&mut self, strict_std_compliance: bool) {
        self.inner.strict_std_compliance = strict_std_compliance;
    }

    pub fn get_strict_std_compliance(&self) -> bool {
        self.inner.strict_std_compliance
    }

    pub fn set_output_invisible_frames(&mut self, output_invisible_frames: bool) {
        self.inner.output_invisible_frames = output_invisible_frames;
    }

    pub fn get_output_invisible_frames(&self) -> bool {
        self.inner.output_invisible_frames
    }

    pub fn set_inloop_filters(&mut self, inloop_filters: InloopFilterType) {
        self.inner.inloop_filters = inloop_filters;
    }

    pub fn get_inloop_filters(&self) -> InloopFilterType {
        self.inner.inloop_filters
    }

    pub fn set_decode_frame_type(&mut self, decode_frame_type: DecodeFrameType) {
        self.inner.decode_frame_type = decode_frame_type;
    }

    pub fn get_decode_frame_type(&self) -> DecodeFrameType {
        self.inner.decode_frame_type
    }

    /// Decodes into pictures from `allocator` instead of rav1d's own pool.
    pub fn set_allocator(&mut self, allocator: Arc<dyn PictureAllocator>) {
        self.inner.allocator = Some(Rav1dPicAllocator(allocator));
    }
}

/// A `rav1d` decoder instance.
pub struct Decoder {
    ctx: Arc<Rav1dContext>,
    pending_data: Option<Rav1dData>,
    n_threads: InRange<u16, 0, 256>,
    max_frame_delay: InRange<u16, 0, 256>,
}

impl Decoder {
    /// Creates a new [`Decoder`] instance with given [`Settings`].
    pub fn with_settings(settings: &Settings) -> Result<Self, Rav1dError> {
        Self::open(settings, None)
    }

    /// A [`Decoder`] with its `n_threads - 1` workers on threads of `spawner` instead of its own:
    /// those it takes when asked, while the thread decoding works too until its frame is done.
    /// Frames are decoded one at a time; with no worker taken it decodes alone. Dropping the
    /// decoder returns once every worker has returned.
    pub fn with_spawner(settings: &Settings, spawner: &Spawner) -> Result<Self, Rav1dError> {
        Self::open(settings, Some(spawner))
    }

    fn open(settings: &Settings, spawner: Option<&Spawner>) -> Result<Self, Rav1dError> {
        rav1d_open(&settings.inner, spawner).map(|ctx| Decoder {
            ctx,
            pending_data: None,
            n_threads: settings.inner.n_threads,
            max_frame_delay: settings.inner.max_frame_delay,
        })
    }

    /// Creates a new [`Decoder`] instance with the default settings.
    pub fn new() -> Result<Self, Rav1dError> {
        Self::with_settings(&Settings::default())
    }

    /// Flush the decoder.
    ///
    /// This drops all delayed frames and data still pending, and clears the internal decoder state.
    pub fn flush(&mut self) {
        self.pending_data = None;
        rav1d_flush(&self.ctx);
    }

    /// Send new AV1 data to the decoder.
    ///
    /// After this returned `Ok(())` or `Err(`[`Rav1dError::TryAgain`]`)` there might be decoded frames
    /// available via [`Decoder::get_picture`].
    ///
    /// # Panics
    ///
    /// If a previous call returned [`Rav1dError::TryAgain`] then this must not be called again until
    /// [`Decoder::send_pending_data`] has returned `Ok(())`.
    pub fn send_data(
        &mut self,
        buf: Box<[u8]>,
        offset: Option<i64>,
        timestamp: Option<i64>,
        duration: Option<i64>,
    ) -> Result<(), Rav1dError> {
        self.send(SharedBytes::new(buf), offset, timestamp, duration)
    }

    /// [`Decoder::send_data`] with the bytes of `owner`, read in place. The decoder drops `owner`
    /// once it no longer reads them, at the latest when it is dropped itself.
    pub fn send_data_owned<T: AsRef<[u8]> + Send + Sync + 'static>(
        &mut self,
        owner: T,
        offset: Option<i64>,
        timestamp: Option<i64>,
        duration: Option<i64>,
    ) -> Result<(), Rav1dError> {
        self.send(SharedBytes::new(owner), offset, timestamp, duration)
    }

    fn send(
        &mut self,
        buf: SharedBytes,
        offset: Option<i64>,
        timestamp: Option<i64>,
        duration: Option<i64>,
    ) -> Result<(), Rav1dError> {
        assert!(
            self.pending_data.is_none(),
            "Have pending data that needs to be handled first"
        );

        let mut data = Rav1dData::from(buf);
        if let Some(offset) = offset {
            data.m.offset = offset;
        }
        if let Some(timestamp) = timestamp {
            data.m.timestamp = timestamp;
        }
        if let Some(duration) = duration {
            data.m.duration = duration;
        }
        self.pending_data = Some(data);
        self.send_pending_data()
    }

    /// Sends any pending data to the decoder.
    ///
    /// This has to be called after [`Decoder::send_data`] has returned `Err(`[`Rav1dError::TryAgain`]`)` to
    /// consume any futher pending data.
    ///
    /// After this returned `Ok(())` or `Err(`[`Rav1dError::TryAgain`]`)` there might be decoded frames
    /// available via [`Decoder::get_picture`].
    pub fn send_pending_data(&mut self) -> Result<(), Rav1dError> {
        let Some(mut data) = self.pending_data.take() else {
            return Ok(());
        };

        if let Err(err) = rav1d_send_data(&self.ctx, &mut data) {
            if matches!(err, Rav1dError::TryAgain) {
                self.pending_data = Some(data);
            }
            return Err(err);
        }

        if data.data.as_ref().is_some_and(|d| !d.is_empty()) {
            self.pending_data = Some(data);
            return Err(Rav1dError::TryAgain);
        }

        Ok(())
    }

    /// Get the next decoded frame from the decoder.
    ///
    /// If this returns `Err(`[`Rav1dError::TryAgain`]`)` then further data has to be sent to the decoder
    /// before further decoded frames become available.
    ///
    /// To make most use of frame threading this function should only be called once per submitted
    /// input frame and not until it returns `Err(`[`Rav1dError::TryAgain`]`)`. Calling it in a loop should
    /// only be done to drain all pending frames at the end.
    pub fn get_picture(&mut self) -> Result<Picture, Rav1dError> {
        let mut pic = Rav1dPicture::default();
        rav1d_get_picture(&self.ctx, &mut pic)?;

        Ok(Picture {
            inner: Arc::new(pic),
            by_rows: false,
        })
    }

    /// Decodes frames that are decoded on the calling thread alone (one frame at a time, no worker
    /// taken) a superblock row at a time: [`Decoder::send_data`] or [`Decoder::get_picture`],
    /// whichever parses such a frame, only sets it up, each [`Decoder::decode_rows`] decodes one
    /// more row of superblocks, and [`Decoder::get_picture`] gives the picture once all are,
    /// [`Rav1dError::TryAgain`] before. A frame with film grain to apply, one copying blocks from
    /// rows above (intra block copy), or one not output, is decoded whole.
    pub fn set_decode_by_rows(&mut self, by_rows: bool) {
        self.ctx.state.try_lock().unwrap().by_rows = by_rows;
    }

    /// Decodes the tile columns of each superblock row decoded by rows on the threads `spawner`
    /// hands out as well as on the calling thread, which [`Decoder::decode_rows`] returns once all
    /// are decoded; `None` decodes them on the calling thread alone.
    pub fn set_rows_spawner(&mut self, spawner: Option<Spawner>) {
        self.ctx.state.try_lock().unwrap().rows_spawner = spawner;
    }

    /// Decodes the next superblock row of the frame set up to be decoded by rows: how many rows
    /// from the top of its picture no later row changes or reads, its height once the frame is
    /// done. [`Rav1dError::TryAgain`] where no frame is being decoded by rows; on an error the frame
    /// is gone, as where [`Decoder::send_data`] fails to decode it.
    pub fn decode_rows(&mut self) -> Result<u32, Rav1dError> {
        let state = &mut *self.ctx.state.try_lock().unwrap();
        rav1d_decode_rows(&self.ctx, state).map(|rows| rows as u32)
    }

    /// The picture of the frame being decoded by rows, to read its rows from as
    /// [`Decoder::decode_rows`] makes them final. [`Rav1dError::TryAgain`] where no frame is.
    ///
    /// Its planes are written while it is decoded: read them through [`Picture::plane_ptr`].
    pub fn rows_picture(&mut self) -> Result<Picture, Rav1dError> {
        let state = self.ctx.state.try_lock().unwrap();
        if state.rows.is_none() {
            return Err(Rav1dError::TryAgain);
        }
        Ok(Picture {
            inner: Arc::new(state.out.p.clone()),
            by_rows: true,
        })
    }

    /// Get the decoder delay.
    pub fn get_frame_delay(&self) -> u32 {
        // The only fields this actually needs from `Rav1dSettings`
        // are n_threads and max_frame_delay so we just pass these in directly

        rav1d_get_frame_delay(&Rav1dSettings {
            n_threads: self.n_threads,
            max_frame_delay: self.max_frame_delay,
            ..Default::default()
        }) as u32
    }
}

/// The encoder's filter trials, on a decoder of their own.
#[cfg(feature = "encoder")]
pub struct DecoderTrials;

#[cfg(feature = "encoder")]
impl rav1d_shared::filter_trials::FilterTrials for DecoderTrials {
    fn decoded(
        &self,
        data: Box<[u8]>,
        planes: usize,
        measure: &mut rav1d_shared::filter_trials::Measure,
    ) -> [u64; 3] {
        let mut s = Settings::new();
        s.set_inloop_filters(InloopFilterType::DEBLOCK | InloopFilterType::CDEF);
        s.set_n_threads(1);
        s.set_max_frame_delay(1);
        let mut d = Decoder::with_settings(&s).expect("a decoder");
        d.send_data(data, None, None, None).expect("its own stream");
        let pic = d
            .get_picture()
            .or_else(|_| d.get_picture())
            .expect("a picture");
        let comps = [
            PlanarImageComponent::Y,
            PlanarImageComponent::U,
            PlanarImageComponent::V,
        ];
        let (w, h) = (pic.width() as usize, pic.height() as usize);
        let mut out = [0; 3];
        for (p, &c) in comps[..planes].iter().enumerate() {
            out[p] = measure(p, pic.plane(c), pic.stride(c) as usize, w, h);
        }
        out
    }

    /// See `rav1d_filter_trials`: one thread, a frame delay of 1 and the in-loop filters off.
    fn trials(
        &self,
        data: Box<[u8]>,
        measure: &mut rav1d_shared::filter_trials::Measure,
        search: &mut dyn FnMut(&mut rav1d_shared::filter_trials::Trial),
    ) -> Result<(), Rav1dError> {
        let mut s = Settings::new();
        s.set_inloop_filters(InloopFilterType::empty());
        s.set_n_threads(1);
        s.set_max_frame_delay(1);
        let mut d = Decoder::with_settings(&s)?;
        d.ctx
            .keep_filter_data
            .store(true, std::sync::atomic::Ordering::Relaxed);
        d.ctx.state.try_lock().unwrap().by_rows = true;
        d.send_data(data, None, None, None)?;
        let state = &mut *d.ctx.state.try_lock().unwrap();
        crate::decode::rav1d_filter_trials(&d.ctx, state, measure, search)
    }
}

impl Drop for Decoder {
    fn drop(&mut self) {
        rav1d_close(&self.ctx);
    }
}

static_assertions::assert_impl_all!(Decoder: Send, Sync);

/// A decoded frame.
#[derive(Clone)]
pub struct Picture {
    inner: Arc<Rav1dPicture>,
    /// From [`Decoder::rows_picture`], so its planes may still be written.
    by_rows: bool,
}

// A picture from `rows_picture` lends no `&[u8]` while it is written: `plane` refuses it.
static_assertions::assert_impl_all!(Picture: Send, Sync);

/// Frame component.
#[derive(Eq, PartialEq, Copy, Clone, Debug)]
pub enum PlanarImageComponent {
    /// Y component (Luminance).
    Y,
    /// U component (Chrominance).
    U,
    /// V component (Chrominance).
    V,
}

impl TryFrom<usize> for PlanarImageComponent {
    type Error = Rav1dError;
    fn try_from(index: usize) -> Result<Self, Self::Error> {
        match index {
            0 => Ok(Self::Y),
            1 => Ok(Self::U),
            2 => Ok(Self::V),
            _ => Err(Rav1dError::InvalidArgument),
        }
    }
}

impl PlanarImageComponent {
    pub const fn as_index(&self) -> usize {
        match self {
            Self::Y => 0,
            Self::U => 1,
            Self::V => 2,
        }
    }
}

/// Number of bits per component.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct BitsPerComponent(pub u8);

impl BitsPerComponent {
    /// Get the number of bits per component from the high bit depth flag
    pub fn from_hbd(hbd: u8) -> Result<Self, Rav1dError> {
        match hbd {
            0 => Ok(Self(8)),
            1 => Ok(Self(10)),
            2 => Ok(Self(12)),
            _ => Err(Rav1dError::InvalidArgument),
        }
    }
}

impl Picture {
    /// Stride in pixels of the `component` for the decoded frame.
    pub fn stride(&self, component: PlanarImageComponent) -> u32 {
        let s = match component {
            PlanarImageComponent::Y => 0,
            _ => 1,
        };
        self.inner.stride[s].try_into().unwrap()
    }

    /// Plane data of the `component` for the decoded frame.
    ///
    /// # Panics
    ///
    /// On a picture from [`Decoder::rows_picture`], whose planes may still be written.
    pub fn plane<'a>(&'a self, component: PlanarImageComponent) -> &'a [u8] {
        assert!(
            !self.by_rows,
            "a picture decoded by rows is read through Picture::plane_ptr"
        );
        let data = &self.inner.data.as_ref().unwrap().data;
        let component = &data[component.as_index()];
        let guard = component.slice::<BitDepth8, _>(..);
        // SAFETY: [`Picture`] is only created after decoding is complete
        // (in [`Decoder::get_picture`], which calls [`rav1d_get_picture`]),
        // except by [`Decoder::rows_picture`], which `by_rows` refuses above.
        // Thus, the decoder no longer writes to this data,
        // and there are no other safe public ways to have a `&mut` to this data.
        unsafe { guard.get_unchecked() }
    }

    /// The first byte of the `component`'s plane, null where the frame has none; its rows are
    /// [`Picture::stride`] bytes apart. Valid for as long as `self`, but on a picture from
    /// [`Decoder::rows_picture`] only the rows [`Decoder::decode_rows`] calls final are no longer
    /// written.
    pub fn plane_ptr(&self, component: PlanarImageComponent) -> *const u8 {
        let data = &self.inner.data.as_ref().unwrap().data;
        data[component.as_index()].as_ptr()
    }

    /// Bit depth of the plane data.
    ///
    /// This returns 8 or 16 for the underlying integer type used for the plane data.
    ///
    /// Check [`Picture::bits_per_component`] for the number of bits that are used.
    pub fn bit_depth(&self) -> usize {
        self.inner.p.bpc.into()
    }

    /// Bits used per component of the plane data.
    ///
    /// Check [`Picture::bit_depth`] for the number of storage bits.
    pub fn bits_per_component(&self) -> Option<BitsPerComponent> {
        BitsPerComponent::from_hbd(self.inner.seq_hdr.as_ref().unwrap().hbd).ok()
    }

    /// Width of the frame.
    pub fn width(&self) -> u32 {
        self.inner.p.w.try_into().unwrap()
    }

    /// Height of the frame.
    pub fn height(&self) -> u32 {
        self.inner.p.h.try_into().unwrap()
    }

    /// Pixel layout of the frame.
    pub fn pixel_layout(&self) -> PixelLayout {
        self.inner.p.layout
    }

    /// Timestamp of the frame.
    ///
    /// This is the same timestamp as the one provided to [`Decoder::send_data`].
    pub fn timestamp(&self) -> Option<i64> {
        let ts = self.inner.m.timestamp;
        if ts == i64::MIN {
            None
        } else {
            Some(ts)
        }
    }

    /// Duration of the frame.
    ///
    /// This is the same duration as the one provided to [`Decoder::send_data`] or `0` if none was
    /// provided.
    pub fn duration(&self) -> i64 {
        self.inner.m.duration
    }

    /// Offset of the frame.
    ///
    /// This is the same offset as the one provided to [`Decoder::send_data`] or `-1` if none was
    /// provided.
    pub fn offset(&self) -> i64 {
        self.inner.m.offset
    }

    /// The sequence header the picture was decoded under, which may follow the first one in the data.
    pub fn sequence_header(&self) -> Option<&SequenceHeader> {
        self.inner.seq_hdr.as_deref()
    }

    /// Chromaticity coordinates of the source colour primaries.
    pub fn color_primaries(&self) -> pixel::ColorPrimaries {
        self.inner.seq_hdr.as_ref().unwrap().pri.into()
    }

    /// Transfer characteristics function.
    pub fn transfer_characteristic(&self) -> pixel::TransferCharacteristic {
        self.inner.seq_hdr.as_ref().unwrap().trc.into()
    }

    /// Matrix coefficients used in deriving luma and chroma signals from the
    /// green, blue and red or X, Y and Z primaries.
    pub fn matrix_coefficients(&self) -> pixel::MatrixCoefficients {
        self.inner.seq_hdr.as_ref().unwrap().mtrx.into()
    }

    /// YUV color range.
    pub fn color_range(&self) -> pixel::YUVRange {
        self.inner.seq_hdr.as_ref().unwrap().color_range.into()
    }

    /// Sample position for subsampled chroma.
    pub fn chroma_location(&self) -> Option<pixel::ChromaLocation> {
        self.inner.seq_hdr.as_ref().unwrap().chr.try_into().ok()
    }
}

impl Debug for Picture {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        f.debug_struct("Picture")
            .field("width", &self.width())
            .field("height", &self.height())
            .field("bit_depth", &self.bit_depth())
            .field("pixel_layout", &self.pixel_layout())
            .field("timestamp", &self.timestamp())
            .field("duration", &self.duration())
            .field("offset", &self.offset())
            .field("color_primaries", &self.color_primaries())
            .field("transfer_characteristic", &self.transfer_characteristic())
            .field("matrix_coefficients", &self.matrix_coefficients())
            .field("color_range", &self.color_range())
            .field("chroma_location", &self.chroma_location())
            .finish()
    }
}

/// Parses the first sequence header OBU in `data` without decoding anything.
pub fn parse_sequence_header(data: &[u8]) -> Result<SequenceHeader, Rav1dError> {
    if data.is_empty() {
        return Err(Rav1dError::InvalidArgument);
    }
    rav1d_parse_sequence_header(data)
}
