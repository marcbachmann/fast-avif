use bitflags::bitflags;
use fast_avif_shared::in_range::InRange;
use fast_avif_shared::picture::Rav1dPicAllocator;
use strum::FromRepr;

use crate::log::Rav1dLogger;

pub const RAV1D_MAX_THREADS: usize = 256;
pub const RAV1D_MAX_FRAME_DELAY: usize = 256;

bitflags! {
    #[derive(Clone, Copy, PartialEq, Eq, Hash, Default)]
    pub struct Rav1dInloopFilterType: u8 {
        const DEBLOCK = 1 << 1;
        const CDEF = 1 << 2;
        const RESTORATION = 1 << 3;
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, FromRepr, Default)]
pub enum Rav1dDecodeFrameType {
    /// decode and return all frames
    #[default]
    All = 0,
    /// decode and return frames referenced by other frames only
    Reference = 1,
    /// decode and return intra frames only (includes keyframes)
    Intra = 2,
    /// decode and return keyframes only
    Key = 3,
}

bitflags! {
    #[derive(Clone, Copy, PartialEq, Eq, Hash, Default)]
    pub(crate) struct Rav1dEventFlags: u8 {
        /// The last returned picture contains a reference
        /// to a new [`Rav1dSequenceHeader`],
        /// either because it's the start of a new coded sequence,
        /// or the decoder was flushed before it was generated.
        ///
        /// [`Rav1dSequenceHeader`]: fast_avif_shared::headers::Rav1dSequenceHeader
        const NEW_SEQUENCE = 1 << 0;

        /// The last returned picture contains a reference to a
        /// [`Rav1dSequenceHeader`] with new [`Rav1dSequenceHeaderOperatingParameterInfo`]
        /// for the current coded sequence.
        ///
        /// [`Rav1dSequenceHeader`]: fast_avif_shared::headers::Rav1dSequenceHeader
        /// [`Rav1dSequenceHeaderOperatingParameterInfo`]: fast_avif_shared::headers::Rav1dSequenceHeaderOperatingParameterInfo
        const NEW_OP_PARAMS_INFO = 1 << 1;
    }
}

#[repr(C)]
pub(crate) struct Rav1dSettings {
    pub n_threads: InRange<u16, 0, { RAV1D_MAX_THREADS as _ }>,
    pub max_frame_delay: InRange<u16, 0, { RAV1D_MAX_FRAME_DELAY as _ }>,
    pub apply_grain: bool,
    pub operating_point: InRange<u8, 0, 31>,
    pub all_layers: bool,
    pub frame_size_limit: u32,
    /// The decoder's own pool when `None`.
    pub allocator: Option<Rav1dPicAllocator>,
    pub logger: Option<Rav1dLogger>,
    pub strict_std_compliance: bool,
    pub output_invisible_frames: bool,
    pub inloop_filters: Rav1dInloopFilterType,
    pub decode_frame_type: Rav1dDecodeFrameType,
}
