use std::fmt;
use std::fmt::{Debug, Display, Formatter};
use std::ops::{BitAnd, Sub};
use std::sync::Arc;

use av_data::pixel;
use strum::{EnumCount, FromRepr};

use crate::align::ArrayDefault;
use crate::enum_map::EnumKey;
use crate::error::Rav1dError;
use crate::levels::SegmentId;
use crate::relaxed_atomic::RelaxedAtomic;

#[doc(hidden)]
pub const RAV1D_MAX_CDEF_STRENGTHS: usize = 8;
#[doc(hidden)]
pub const RAV1D_MAX_OPERATING_POINTS: usize = 32;
#[doc(hidden)]
pub const RAV1D_MAX_TILE_COLS: usize = 64;
#[doc(hidden)]
pub const RAV1D_MAX_TILE_ROWS: usize = 64;
#[doc(hidden)]
pub const RAV1D_NUM_REF_FRAMES: usize = 8;
#[doc(hidden)]
pub const RAV1D_PRIMARY_REF_NONE: u8 = 7;
#[doc(hidden)]
pub const RAV1D_REFS_PER_FRAME: usize = 7;
#[doc(hidden)]
pub const RAV1D_TOTAL_REFS_PER_FRAME: usize = RAV1D_REFS_PER_FRAME + 1;

#[derive(Clone, Copy, PartialEq, Eq, FromRepr)]
pub enum Rav1dObuType {
    SeqHdr = 1,
    Td = 2,
    FrameHdr = 3,
    TileGrp = 4,
    Metadata = 5,
    Frame = 6,
    RedundantFrameHdr = 7,
    Padding = 15,
}

#[derive(Clone, Copy, PartialEq, Eq, FromRepr, EnumCount, Default)]
pub enum Rav1dTxfmMode {
    #[default] // Not really a real default.
    Only4x4 = 0,
    Largest = 1,
    Switchable = 2,
}

#[derive(Clone, Copy, PartialEq, Eq, FromRepr, Default, Debug)]
pub enum Rav1dFilterMode {
    #[default] // Not really a real default.
    Regular8Tap = 0,
    Smooth8Tap = 1,
    Sharp8Tap = 2,
    Bilinear = 3,
    Switchable = 4,
}

impl ArrayDefault for Rav1dFilterMode {
    fn default() -> Self {
        Default::default()
    }
}

impl Rav1dFilterMode {
    pub const N_FILTERS: usize = 4;
    pub const N_SWITCHABLE_FILTERS: Self = Self::Bilinear;
}

#[derive(Clone, Copy, PartialEq, Eq, FromRepr)]
pub enum Rav1dAdaptiveBoolean {
    Off = 0,
    On = 1,
    Adaptive = 2,
}

impl From<bool> for Rav1dAdaptiveBoolean {
    fn from(value: bool) -> Self {
        match value {
            true => Self::On,
            false => Self::Off,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, FromRepr)]
pub enum SgrIdx {
    I0 = 0,
    I1 = 1,
    I2 = 2,
    I3 = 3,
    I4 = 4,
    I5 = 5,
    I6 = 6,
    I7 = 7,
    I8 = 8,
    I9 = 9,
    I10 = 10,
    I11 = 11,
    I12 = 12,
    I13 = 13,
    I14 = 14,
    I15 = 15,
}

impl Display for SgrIdx {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "{}", *self as u8)
    }
}

impl Debug for SgrIdx {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "{}", self)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Rav1dRestorationType {
    #[default]
    None,
    Switchable,
    Wiener,
    SgrProj(SgrIdx),
}

impl Rav1dRestorationType {
    #[inline]
    pub const fn to_repr(&self) -> u8 {
        match *self {
            Self::None => 0,
            Self::Switchable => 1,
            Self::Wiener => 2,
            Self::SgrProj(idx) => 3 + idx as u8,
        }
    }

    #[inline]
    pub const fn from_repr(repr: usize) -> Option<Self> {
        Some(match repr {
            0 => Self::None,
            1 => Self::Switchable,
            2 => Self::Wiener,
            3 => Self::SgrProj(SgrIdx::I0),
            _ => return None,
        })
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, FromRepr)]
pub enum Rav1dWarpedMotionType {
    Identity = 0,
    Translation = 1,
    RotZoom = 2,
    Affine = 3,
}

#[derive(Clone)]
pub struct Rav1dWarpedMotionParams {
    pub r#type: Rav1dWarpedMotionType,
    pub matrix: [i32; 6],
    pub abcd: RelaxedAtomic<[i16; 4]>,
}

impl Rav1dWarpedMotionParams {
    #[inline]
    pub fn alpha(&self) -> i16 {
        self.abcd.get()[0]
    }

    #[inline]
    pub fn beta(&self) -> i16 {
        self.abcd.get()[1]
    }

    #[inline]
    pub fn gamma(&self) -> i16 {
        self.abcd.get()[2]
    }

    #[inline]
    pub fn delta(&self) -> i16 {
        self.abcd.get()[3]
    }
}

// TODO(kkysen) Eventually the [`impl Default`] might not be needed.
/// Pixel layout of a frame.
#[derive(Clone, Copy, PartialEq, Eq, EnumCount, FromRepr, Default, Debug)]
pub enum Rav1dPixelLayout {
    /// Monochrome.
    #[default]
    I400 = 0,
    /// 4:2:0 planar.
    I420 = 1,
    /// 4:2:2 planar.
    I422 = 2,
    /// 4:4:4 planar.
    I444 = 3,
}

impl Sub for Rav1dPixelLayout {
    type Output = Rav1dPixelLayout;

    fn sub(self, rhs: Self) -> Self::Output {
        Self::from_repr((self as u8 - rhs as u8) as usize).unwrap()
    }
}

impl EnumKey<{ Self::COUNT }> for Rav1dPixelLayout {
    const VALUES: [Self; Self::COUNT] = [Self::I400, Self::I420, Self::I422, Self::I444];

    fn as_usize(self) -> usize {
        self as usize
    }
}

impl BitAnd for Rav1dPixelLayout {
    type Output = bool;

    fn bitand(self, rhs: Self) -> Self::Output {
        (self as usize & rhs as usize) != 0
    }
}

#[derive(Clone, Copy, PartialEq, Eq, EnumCount)]
#[doc(hidden)]
pub enum Rav1dPixelLayoutSubSampled {
    I420,
    I422,
    I444,
}

impl EnumKey<{ Self::COUNT }> for Rav1dPixelLayoutSubSampled {
    const VALUES: [Self; Self::COUNT] = [Self::I420, Self::I422, Self::I444];

    fn as_usize(self) -> usize {
        self as usize
    }
}

impl TryFrom<Rav1dPixelLayout> for Rav1dPixelLayoutSubSampled {
    type Error = ();

    fn try_from(value: Rav1dPixelLayout) -> Result<Self, Self::Error> {
        use Rav1dPixelLayout::*;
        Ok(match value {
            I400 => return Err(()),
            I420 => Self::I420,
            I422 => Self::I422,
            I444 => Self::I444,
        })
    }
}

impl From<Rav1dPixelLayoutSubSampled> for Rav1dPixelLayout {
    fn from(value: Rav1dPixelLayoutSubSampled) -> Self {
        use Rav1dPixelLayoutSubSampled::*;
        match value {
            I420 => Self::I420,
            I422 => Self::I422,
            I444 => Self::I444,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, FromRepr, Default)]
pub enum Rav1dFrameType {
    #[default] // Not really a real default.
    Key = 0,
    Inter = 1,
    Intra = 2,
    Switch = 3,
}

impl Rav1dFrameType {
    #[inline]
    pub const fn is_inter_or_switch(&self) -> bool {
        matches!(self, Self::Inter | Self::Switch)
    }

    #[inline]
    pub const fn is_key_or_intra(&self) -> bool {
        matches!(self, Self::Key | Self::Intra)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, FromRepr, Default)]
pub enum Rav1dColorPrimaries {
    BT709 = 1,
    #[default]
    Unspecified = 2,
    BT470M = 4,
    BT470BG = 5,
    BT601 = 6,
    SMPTE240 = 7,
    Film = 8,
    BT2020 = 9,
    XYZ = 10,
    SMPTE431 = 11,
    SMPTE432 = 12,
    EBU3213 = 22,
}

impl From<Rav1dColorPrimaries> for pixel::ColorPrimaries {
    fn from(val: Rav1dColorPrimaries) -> Self {
        use Rav1dColorPrimaries::*;
        match val {
            BT709 => Self::BT709,
            Unspecified => Self::Unspecified,
            BT470M => Self::BT470M,
            BT470BG => Self::BT470BG,
            BT601 => Self::BT470BG,
            SMPTE240 => Self::ST240M,
            Film => Self::Film,
            BT2020 => Self::BT2020,
            XYZ => Self::ST428,
            SMPTE431 => Self::P3DCI,
            SMPTE432 => Self::P3Display,
            EBU3213 => Self::Tech3213,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, FromRepr, Default)]
pub enum Rav1dTransferCharacteristics {
    // `TC_RESERVED_0` from spec.
    BT709 = 1,
    #[default]
    Unspecified = 2,
    // `TC_RESERVED_3` from spec.
    BT470M = 4,
    BT470BG = 5,
    BT601 = 6,
    SMPTE240 = 7,
    Linear = 8,
    Log100 = 9,
    Log100Sqrt10 = 10,
    IEC61966 = 11,
    BT1361 = 12,
    SRGB = 13,
    BT2020_10Bit = 14,
    BT2020_12Bit = 15,
    SMPTE2084 = 16,
    SMPTE428 = 17,
    HLG = 18,
}

impl From<Rav1dTransferCharacteristics> for pixel::TransferCharacteristic {
    fn from(val: Rav1dTransferCharacteristics) -> Self {
        use Rav1dTransferCharacteristics::*;
        match val {
            BT709 => Self::BT1886,
            Unspecified => Self::Unspecified,
            BT470M => Self::BT470M,
            BT470BG => Self::BT470BG,
            BT601 => Self::ST170M,
            SMPTE240 => Self::ST240M,
            Linear => Self::Linear,
            Log100 => Self::Logarithmic100,
            Log100Sqrt10 => Self::Logarithmic316,
            IEC61966 => Self::SRGB,
            BT1361 => Self::BT1886,
            SRGB => Self::SRGB,
            BT2020_10Bit => Self::BT2020Ten,
            BT2020_12Bit => Self::BT2020Twelve,
            SMPTE2084 => Self::PerceptualQuantizer,
            SMPTE428 => Self::ST428,
            HLG => Self::HybridLogGamma,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, FromRepr, Default)]
pub enum Rav1dMatrixCoefficients {
    Identity = 0,
    BT709 = 1,
    #[default]
    Unspecified = 2,
    // `MC_RESERVED_3` from spec.
    FCC = 4,
    BT470BG = 5,
    BT601 = 6,
    SMPTE240 = 7,
    #[allow(non_camel_case_types, reason = "inconsistency with other SMPTEs")]
    SMPTE_YCgCo = 8,
    BT2020NCL = 9,
    BT2020CL = 10,
    SMPTE2085 = 11,
    ChromatNCL = 12,
    ChromatCL = 13,
    ICtCp = 14,
}

impl From<Rav1dMatrixCoefficients> for pixel::MatrixCoefficients {
    fn from(val: Rav1dMatrixCoefficients) -> Self {
        use Rav1dMatrixCoefficients::*;
        match val {
            Identity => Self::Identity,
            BT709 => Self::BT709,
            Unspecified => Self::Unspecified,
            FCC => Self::BT470M,
            BT470BG => Self::BT470BG,
            BT601 => Self::BT470BG,
            SMPTE240 => Self::ST240M,
            SMPTE_YCgCo => Self::YCgCo,
            BT2020NCL => Self::BT2020NonConstantLuminance,
            BT2020CL => Self::BT2020ConstantLuminance,
            SMPTE2085 => Self::ST2085,
            ChromatNCL => Self::ChromaticityDerivedNonConstantLuminance,
            ChromatCL => Self::ChromaticityDerivedConstantLuminance,
            ICtCp => Self::ICtCp,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, FromRepr)]
pub enum Rav1dChromaSamplePosition {
    Unknown = 0,
    /// Horizontally co-located with (0, 0) luma sample, vertical position
    /// in the middle between two luma samples
    Vertical = 1,
    /// co-located with (0, 0) luma sample
    Colocated = 2,
    _Reserved = 3,
}

impl TryFrom<Rav1dChromaSamplePosition> for pixel::ChromaLocation {
    type Error = Rav1dError;

    fn try_from(value: Rav1dChromaSamplePosition) -> Result<pixel::ChromaLocation, Self::Error> {
        // According to y4m mapping declared in dav1d's output/y4m2.c and applied from FFmpeg's yuv4mpegdec.c
        use Rav1dChromaSamplePosition::*;
        match value {
            Unknown | Colocated => Ok(Self::Center),
            Vertical => Ok(Self::Left),
            _Reserved => Err(Rav1dError::InvalidArgument),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub enum Rav1dColorRange {
    Limited,
    Full,
}

impl Rav1dColorRange {
    #[inline]
    pub const fn is_full(&self) -> bool {
        matches!(self, Self::Full)
    }

    pub const fn to_dav1d(&self) -> u8 {
        self.is_full() as u8
    }
}

impl Rav1dColorRange {
    pub const fn from_is_full(is_full: bool) -> Self {
        if is_full {
            Self::Full
        } else {
            Self::Limited
        }
    }

    pub const fn from_dav1d(value: u8) -> Self {
        Self::from_is_full(value != 0)
    }
}

impl From<Rav1dColorRange> for pixel::YUVRange {
    fn from(value: Rav1dColorRange) -> Self {
        match value {
            Rav1dColorRange::Limited => Self::Limited,
            Rav1dColorRange::Full => Self::Full,
        }
    }
}

#[repr(C)]
pub struct Rav1dContentLightLevel {
    /// Maximum content light level (MaxCLL) in candela per square metre.
    pub max_content_light_level: u16,
    /// Maximum frame average light level (MaxFLL) in candela per square metre.
    pub max_frame_average_light_level: u16,
}

#[repr(C)]
pub struct Rav1dMasteringDisplay {
    /// Red/green/blue XY coordinates of primaries in CIE 1931 color space as 0.16 fixed-point number.
    pub primaries: [[u16; 2]; 3],
    /// XY coordinates of white point in CIE 1931 color space as 0.16 fixed-point number.
    pub white_point: [u16; 2],
    /// Maximum luminance in candela per square metre as 24.8 fixed-point number.
    pub max_luminance: u32,
    /// Minimum luminance in candela per square metre as 18.14 fixed-point number.
    pub min_luminance: u32,
}

#[repr(C)]
pub struct Rav1dITUTT35 {
    pub country_code: u8,
    pub country_code_extension_byte: u8,
    pub payload: Box<[u8]>,
}

impl Rav1dITUTT35 {
    pub fn to_immut(mutable: Vec<Rav1dITUTT35>) -> Arc<Box<[Rav1dITUTT35]>> {
        Arc::new(mutable.into_boxed_slice())
    }
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
#[repr(C)]
pub struct Rav1dSequenceHeaderOperatingPoint {
    pub major_level: u8,
    pub minor_level: u8,
    pub initial_display_delay: u8,
    pub idc: u16,
    pub tier: u8,
    pub decoder_model_param_present: u8,
    pub display_model_param_present: u8,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
#[repr(C)]
pub struct Rav1dSequenceHeaderOperatingParameterInfo {
    pub decoder_buffer_delay: u32,
    pub encoder_buffer_delay: u32,
    pub low_delay_mode: u8,
}

#[derive(Clone, Copy, PartialEq, Eq, FromRepr)]
pub enum Rav1dProfile {
    Main = 0,
    High = 1,
    Professional = 2,
}

impl From<Rav1dProfile> for u8 {
    fn from(value: Rav1dProfile) -> Self {
        value as u8
    }
}

impl TryFrom<u8> for Rav1dProfile {
    type Error = ();

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        Self::from_repr(value as usize).ok_or(())
    }
}

#[derive(Clone)]
#[repr(C)]
pub struct Rav1dSequenceHeader {
    pub profile: Rav1dProfile,
    pub max_width: i32,
    pub max_height: i32,
    pub layout: Rav1dPixelLayout,
    pub pri: Rav1dColorPrimaries,
    pub trc: Rav1dTransferCharacteristics,
    pub mtrx: Rav1dMatrixCoefficients,
    pub chr: Rav1dChromaSamplePosition,
    pub hbd: u8,
    pub color_range: Rav1dColorRange,
    pub num_operating_points: u8,
    pub operating_points: [Rav1dSequenceHeaderOperatingPoint; RAV1D_MAX_OPERATING_POINTS],
    pub still_picture: u8,
    pub reduced_still_picture_header: u8,
    pub timing_info_present: u8,
    /// > 0 if defined, 0 otherwise
    pub num_units_in_tick: u32,
    /// > 0 if defined, 0 otherwise
    pub time_scale: u32,
    pub equal_picture_interval: u8,
    pub num_ticks_per_picture: u32,
    pub decoder_model_info_present: u8,
    pub encoder_decoder_buffer_delay_length: u8,
    /// > 0 if defined, 0 otherwise
    pub num_units_in_decoding_tick: u32,
    pub buffer_removal_delay_length: u8,
    pub frame_presentation_delay_length: u8,
    pub display_model_info_present: u8,
    pub width_n_bits: u8,
    pub height_n_bits: u8,
    pub frame_id_numbers_present: u8,
    pub delta_frame_id_n_bits: u8,
    pub frame_id_n_bits: u8,
    pub sb128: u8,
    pub filter_intra: u8,
    pub intra_edge_filter: u8,
    pub inter_intra: u8,
    pub masked_compound: u8,
    pub warped_motion: u8,
    pub dual_filter: u8,
    pub order_hint: u8,
    pub jnt_comp: u8,
    pub ref_frame_mvs: u8,
    pub screen_content_tools: Rav1dAdaptiveBoolean,
    pub force_integer_mv: Rav1dAdaptiveBoolean,
    pub order_hint_n_bits: u8,
    pub super_res: u8,
    pub cdef: u8,
    pub restoration: u8,
    pub ss_hor: u8,
    pub ss_ver: u8,
    pub monochrome: u8,
    pub color_description_present: bool,
    pub separate_uv_delta_q: u8,
    pub film_grain_present: u8,
    pub operating_parameter_info:
        [Rav1dSequenceHeaderOperatingParameterInfo; RAV1D_MAX_OPERATING_POINTS],
}

impl Rav1dSequenceHeader {
    /// TODO(kkysen) We should split [`Rav1dSequenceHeader`] into an inner `struct`
    /// without the `operating_parameter_info` field
    /// so that we can just `#[derive(PartialEq, Eq)]` it.
    pub fn eq_without_operating_parameter_info(&self, other: &Self) -> bool {
        // Destructure so that there's a compile error
        // if we add fields and forget to update them here.
        let Self {
            profile,
            max_width,
            max_height,
            layout,
            pri,
            trc,
            mtrx,
            chr,
            hbd,
            color_range,
            num_operating_points,
            operating_points,
            still_picture,
            reduced_still_picture_header,
            timing_info_present,
            num_units_in_tick,
            time_scale,
            equal_picture_interval,
            num_ticks_per_picture,
            decoder_model_info_present,
            encoder_decoder_buffer_delay_length,
            num_units_in_decoding_tick,
            buffer_removal_delay_length,
            frame_presentation_delay_length,
            display_model_info_present,
            width_n_bits,
            height_n_bits,
            frame_id_numbers_present,
            delta_frame_id_n_bits,
            frame_id_n_bits,
            sb128,
            filter_intra,
            intra_edge_filter,
            inter_intra,
            masked_compound,
            warped_motion,
            dual_filter,
            order_hint,
            jnt_comp,
            ref_frame_mvs,
            screen_content_tools,
            force_integer_mv,
            order_hint_n_bits,
            super_res,
            cdef,
            restoration,
            ss_hor,
            ss_ver,
            monochrome,
            color_description_present,
            separate_uv_delta_q,
            film_grain_present,
            operating_parameter_info: _,
        } = self;
        true && *profile == other.profile
            && *max_width == other.max_width
            && *max_height == other.max_height
            && *layout == other.layout
            && *pri == other.pri
            && *trc == other.trc
            && *mtrx == other.mtrx
            && *chr == other.chr
            && *hbd == other.hbd
            && *color_range == other.color_range
            && *num_operating_points == other.num_operating_points
            && *operating_points == other.operating_points
            && *still_picture == other.still_picture
            && *reduced_still_picture_header == other.reduced_still_picture_header
            && *timing_info_present == other.timing_info_present
            && *num_units_in_tick == other.num_units_in_tick
            && *time_scale == other.time_scale
            && *equal_picture_interval == other.equal_picture_interval
            && *num_ticks_per_picture == other.num_ticks_per_picture
            && *decoder_model_info_present == other.decoder_model_info_present
            && *encoder_decoder_buffer_delay_length == other.encoder_decoder_buffer_delay_length
            && *num_units_in_decoding_tick == other.num_units_in_decoding_tick
            && *buffer_removal_delay_length == other.buffer_removal_delay_length
            && *frame_presentation_delay_length == other.frame_presentation_delay_length
            && *display_model_info_present == other.display_model_info_present
            && *width_n_bits == other.width_n_bits
            && *height_n_bits == other.height_n_bits
            && *frame_id_numbers_present == other.frame_id_numbers_present
            && *delta_frame_id_n_bits == other.delta_frame_id_n_bits
            && *frame_id_n_bits == other.frame_id_n_bits
            && *sb128 == other.sb128
            && *filter_intra == other.filter_intra
            && *intra_edge_filter == other.intra_edge_filter
            && *inter_intra == other.inter_intra
            && *masked_compound == other.masked_compound
            && *warped_motion == other.warped_motion
            && *dual_filter == other.dual_filter
            && *order_hint == other.order_hint
            && *jnt_comp == other.jnt_comp
            && *ref_frame_mvs == other.ref_frame_mvs
            && *screen_content_tools == other.screen_content_tools
            && *force_integer_mv == other.force_integer_mv
            && *order_hint_n_bits == other.order_hint_n_bits
            && *super_res == other.super_res
            && *cdef == other.cdef
            && *restoration == other.restoration
            && *ss_hor == other.ss_hor
            && *ss_ver == other.ss_ver
            && *monochrome == other.monochrome
            && *color_description_present == other.color_description_present
            && *separate_uv_delta_q == other.separate_uv_delta_q
            && *film_grain_present == other.film_grain_present
    }
}

#[derive(Clone, Default)]
#[repr(C)]
pub struct Rav1dSegmentationData {
    pub delta_q: i16,
    pub delta_lf_y_v: i8,
    pub delta_lf_y_h: i8,
    pub delta_lf_u: i8,
    pub delta_lf_v: i8,
    pub r#ref: i8,
    pub skip: u8,
    pub globalmv: u8,
}

#[derive(Clone, Default)]
#[repr(C)]
pub struct Rav1dSegmentationDataSet {
    pub d: [Rav1dSegmentationData; SegmentId::COUNT],
    pub preskip: u8,
    pub last_active_segid: i8,
}

#[derive(Clone, Default)]
#[repr(C)]
pub struct Rav1dLoopfilterModeRefDeltas {
    pub mode_delta: [i8; 2],
    pub ref_delta: [i8; RAV1D_TOTAL_REFS_PER_FRAME],
}

#[derive(Clone, Default)]
pub struct Rav1dFilmGrainData {
    pub seed: u32,
    pub num_y_points: i32,
    pub y_points: [[u8; 2]; 14],
    pub chroma_scaling_from_luma: bool,
    pub num_uv_points: [i32; 2],
    pub uv_points: [[[u8; 2]; 10]; 2],
    pub scaling_shift: u8,
    pub ar_coeff_lag: i32,
    pub ar_coeffs_y: [i8; 24],
    pub ar_coeffs_uv: [[i8; 28]; 2],
    pub ar_coeff_shift: u8,
    pub grain_scale_shift: u8,
    pub uv_mult: [i32; 2],
    pub uv_luma_mult: [i32; 2],
    pub uv_offset: [i32; 2],
    pub overlap_flag: bool,
    pub clip_to_restricted_range: bool,
}

#[derive(Clone, Default)]
#[repr(C)]
pub struct Rav1dFrameHeaderFilmGrain {
    pub data: Rav1dFilmGrainData,
    pub present: u8,
    pub update: u8,
}

#[derive(Clone, Copy, Default)]
#[repr(C)]
pub struct Rav1dFrameHeaderOperatingPoint {
    pub buffer_removal_time: u32,
}

#[derive(Clone, Default)]
#[repr(C)]
pub struct Rav1dFrameHeaderSuperRes {
    pub width_scale_denominator: u8,
    pub enabled: bool,
}

#[derive(Clone)]
#[repr(C)]
pub struct Rav1dFrameHeaderTiling {
    pub uniform: u8,
    pub n_bytes: u8,
    pub min_log2_cols: u8,
    pub max_log2_cols: u8,
    pub log2_cols: u8,
    pub cols: u8,
    pub min_log2_rows: u8,
    pub max_log2_rows: u8,
    pub log2_rows: u8,
    pub rows: u8,
    pub col_start_sb: [u16; RAV1D_MAX_TILE_COLS + 1],
    pub row_start_sb: [u16; RAV1D_MAX_TILE_ROWS + 1],
    pub update: u16,
}

impl Default for Rav1dFrameHeaderTiling {
    fn default() -> Self {
        Self {
            uniform: Default::default(),
            n_bytes: Default::default(),
            min_log2_cols: Default::default(),
            max_log2_cols: Default::default(),
            log2_cols: Default::default(),
            cols: Default::default(),
            min_log2_rows: Default::default(),
            max_log2_rows: Default::default(),
            log2_rows: Default::default(),
            rows: Default::default(),
            col_start_sb: [Default::default(); RAV1D_MAX_TILE_COLS + 1],
            row_start_sb: [Default::default(); RAV1D_MAX_TILE_ROWS + 1],
            update: Default::default(),
        }
    }
}

#[derive(Clone, Default)]
#[repr(C)]
pub struct Rav1dFrameHeaderQuant {
    pub yac: u8,
    pub ydc_delta: i8,
    pub udc_delta: i8,
    pub uac_delta: i8,
    pub vdc_delta: i8,
    pub vac_delta: i8,
    pub qm: u8,
    pub qm_y: u8,
    pub qm_u: u8,
    pub qm_v: u8,
}

#[derive(Clone, Default)]
#[repr(C)]
pub struct Rav1dFrameHeaderSegmentation {
    pub enabled: u8,
    pub update_map: u8,
    pub temporal: u8,
    pub update_data: u8,
    pub seg_data: Rav1dSegmentationDataSet,
    /// TODO compress `[bool; 8]` into `u8`.
    pub lossless: [bool; SegmentId::COUNT],
    pub qidx: [u8; SegmentId::COUNT],
}

#[derive(Clone, Default)]
#[repr(C)]
pub struct Rav1dFrameHeaderDeltaQ {
    pub present: u8,
    pub res_log2: u8,
}

#[derive(Clone, Default)]
#[repr(C)]
pub struct Rav1dFrameHeaderDeltaLF {
    pub present: u8,
    pub res_log2: u8,
    pub multi: u8,
}

#[derive(Clone, Default)]
#[repr(C)]
pub struct Rav1dFrameHeaderDelta {
    pub q: Rav1dFrameHeaderDeltaQ,
    pub lf: Rav1dFrameHeaderDeltaLF,
}

#[derive(Clone, Default)]
#[repr(C)]
pub struct Rav1dFrameHeaderLoopFilter {
    pub level_y: [u8; 2],
    pub level_u: u8,
    pub level_v: u8,
    pub mode_ref_delta_enabled: u8,
    pub mode_ref_delta_update: u8,
    pub mode_ref_deltas: Rav1dLoopfilterModeRefDeltas,
    pub sharpness: u8,
}

#[derive(Clone, Default)]
#[repr(C)]
pub struct Rav1dFrameHeaderCdef {
    pub damping: u8,
    pub n_bits: u8,
    pub y_strength: [u8; RAV1D_MAX_CDEF_STRENGTHS],
    pub uv_strength: [u8; RAV1D_MAX_CDEF_STRENGTHS],
}

#[derive(Clone, Default)]
#[repr(C)]
pub struct Rav1dFrameHeaderRestoration {
    pub r#type: [Rav1dRestorationType; 3],
    pub unit_size: [u8; 2],
}

#[derive(Clone, Default)]
#[repr(C)]
pub struct Rav1dFrameSize {
    pub width: [i32; 2],
    pub height: i32,
    pub render_width: i32,
    pub render_height: i32,
    pub super_res: Rav1dFrameHeaderSuperRes,
    pub have_render_size: u8,
}

#[derive(Clone, Default)]
#[repr(C)]
pub struct Rav1dFrameSkipMode {
    pub allowed: u8,
    pub enabled: u8,
    pub refs: [i8; 2],
}

#[derive(Clone, Default)]
#[repr(C)]
pub struct Rav1dFrameHeader {
    pub size: Rav1dFrameSize,
    pub film_grain: Rav1dFrameHeaderFilmGrain,
    pub frame_type: Rav1dFrameType,
    pub frame_offset: u8,
    pub temporal_id: u8,
    pub spatial_id: u8,
    pub show_existing_frame: u8,
    pub existing_frame_idx: u8,
    pub frame_id: u32,
    pub frame_presentation_delay: u32,
    pub show_frame: u8,
    pub showable_frame: u8,
    pub error_resilient_mode: u8,
    pub disable_cdf_update: u8,
    pub allow_screen_content_tools: bool,
    pub force_integer_mv: bool,
    pub frame_size_override: bool,
    pub primary_ref_frame: u8,
    pub buffer_removal_time_present: u8,
    pub operating_points: [Rav1dFrameHeaderOperatingPoint; RAV1D_MAX_OPERATING_POINTS],
    pub refresh_frame_flags: u8,
    pub allow_intrabc: bool,
    pub frame_ref_short_signaling: u8,
    pub refidx: [i8; RAV1D_REFS_PER_FRAME],
    pub hp: bool,
    pub subpel_filter_mode: Rav1dFilterMode,
    pub switchable_motion_mode: u8,
    pub use_ref_frame_mvs: u8,
    pub refresh_context: u8,
    pub tiling: Rav1dFrameHeaderTiling,
    pub quant: Rav1dFrameHeaderQuant,
    pub segmentation: Rav1dFrameHeaderSegmentation,
    pub delta: Rav1dFrameHeaderDelta,
    pub all_lossless: bool,
    pub loopfilter: Rav1dFrameHeaderLoopFilter,
    pub cdef: Rav1dFrameHeaderCdef,
    pub restoration: Rav1dFrameHeaderRestoration,
    pub txfm_mode: Rav1dTxfmMode,
    pub switchable_comp_refs: u8,
    pub skip_mode: Rav1dFrameSkipMode,
    pub warp_motion: u8,
    pub reduced_txtp_set: u8,
    pub gmv: [Rav1dWarpedMotionParams; RAV1D_REFS_PER_FRAME],
}

impl Rav1dFilmGrainData {
    #[doc(hidden)]
    pub fn has_grain(&self) -> bool {
        self.num_y_points != 0
            || self.num_uv_points[0] != 0
            || self.num_uv_points[1] != 0
            || self.clip_to_restricted_range && self.chroma_scaling_from_luma
    }
}
