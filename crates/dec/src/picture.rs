#![deny(unsafe_op_in_unsafe_fn)]
use std::sync::atomic::AtomicU32;
use std::sync::Arc;

use bitflags::bitflags;
use rav1d_shared::error::{Rav1dError, Rav1dResult};
use rav1d_shared::headers::{
    Rav1dContentLightLevel, Rav1dFrameHeader, Rav1dITUTT35, Rav1dMasteringDisplay,
    Rav1dSequenceHeader,
};
use rav1d_shared::picture::{alloc_picture_data, Rav1dPicAllocator, Rav1dPicture};
use rav1d_shared::props::Rav1dDataProps;

use crate::internal::{Rav1dFrameContext, Rav1dFrameData};
use crate::log::{Rav1dLog as _, Rav1dLogger};
use crate::settings::Rav1dEventFlags;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct PictureFlags(u8);

bitflags! {
    impl PictureFlags: u8 {
        const NEW_SEQUENCE = 1 << 0;
        const NEW_OP_PARAMS_INFO = 1 << 1;
        const NEW_TEMPORAL_UNIT = 1 << 2;
    }
}

impl From<PictureFlags> for Rav1dEventFlags {
    fn from(value: PictureFlags) -> Self {
        // [`Rav1dEventFlags`] just has one extra flag vs. [`PictureFlags`],
        // which this just truncates off.
        // Otherwise the values are the same so we can convert the bits.
        Self::from_bits_truncate(value.bits())
    }
}

#[derive(Clone, Default)]
#[repr(C)]
pub(crate) struct Rav1dThreadPicture {
    pub p: Rav1dPicture,
    pub visible: bool,
    /// This can be set for inter frames, non-key intra frames,
    /// or for invisible keyframes that have not yet been made visible
    /// using the show-existing-frame mechanism.
    pub showable: bool,
    pub flags: PictureFlags,
    /// `[0]`: block data (including segmentation map and motion vectors)
    /// `[1]`: pixel data
    pub progress: Option<Arc<[AtomicU32; 2]>>,
}

fn picture_alloc_with_edges(
    logger: &Option<Rav1dLogger>,
    p: &mut Rav1dPicture,
    w: i32,
    h: i32,
    seq_hdr: Option<Arc<Rav1dSequenceHeader>>,
    frame_hdr: Option<Arc<Rav1dFrameHeader>>,
    bpc: u8,
    p_allocator: &Rav1dPicAllocator,
) -> Rav1dResult {
    if p.data.is_some() {
        writeln!(logger, "Picture already allocated!",);
        return Err(Rav1dError::Other);
    }
    assert!(bpc > 0 && bpc <= 16);
    let pic = alloc_picture_data(p_allocator, w, h, seq_hdr.unwrap(), frame_hdr)?;
    *p = pic;

    Ok(())
}

pub fn rav1d_picture_copy_props(
    p: &mut Rav1dPicture,
    content_light: Option<Arc<Rav1dContentLightLevel>>,
    mastering_display: Option<Arc<Rav1dMasteringDisplay>>,
    itut_t35: Arc<Box<[Rav1dITUTT35]>>,
    props: Rav1dDataProps,
) {
    p.m = props;
    p.content_light = content_light;
    p.mastering_display = mastering_display;
    p.itut_t35 = itut_t35;
}

// itut_t35 was taken out of the c.itut_t35 originally, but that violates Rust
// borrowing rules so we need to pass it to this function explicitly.
pub(crate) fn rav1d_thread_picture_alloc(
    fc: &[Rav1dFrameContext],
    logger: &Option<Rav1dLogger>,
    allocator: &Rav1dPicAllocator,
    content_light: Option<Arc<Rav1dContentLightLevel>>,
    mastering_display: Option<Arc<Rav1dMasteringDisplay>>,
    output_invisible_frames: bool,
    max_spatial_id: u8,
    frame_flags: &mut PictureFlags,
    f: &mut Rav1dFrameData,
    bpc: u8,
    itut_t35: Vec<Rav1dITUTT35>,
) -> Rav1dResult {
    let p = &mut f.sr_cur;
    let have_frame_mt = fc.len() > 1;
    let frame_hdr = &**f.frame_hdr.as_ref().unwrap();
    picture_alloc_with_edges(
        logger,
        &mut p.p,
        frame_hdr.size.width[1],
        frame_hdr.size.height,
        f.seq_hdr.clone(),
        f.frame_hdr.clone(),
        bpc,
        allocator,
    )?;

    rav1d_picture_copy_props(
        &mut p.p,
        content_light,
        mastering_display,
        Rav1dITUTT35::to_immut(itut_t35),
        f.tiles[0].data.m.clone(),
    );

    // Don't clear these flags from `c.frame_flags` if the frame is not going to be output.
    // This way they will be added to the next visible frame too.
    let flags_mask = if (frame_hdr.show_frame != 0 || output_invisible_frames)
        && max_spatial_id == frame_hdr.spatial_id
    {
        PictureFlags::empty()
    } else {
        PictureFlags::NEW_SEQUENCE | PictureFlags::NEW_OP_PARAMS_INFO
    };
    p.flags = *frame_flags;
    *frame_flags &= flags_mask;
    p.visible = frame_hdr.show_frame != 0;
    p.showable = frame_hdr.showable_frame != 0;
    p.progress = if have_frame_mt {
        Some(Default::default())
    } else {
        None
    };
    Ok(())
}

pub(crate) fn rav1d_picture_alloc_copy(
    logger: &Option<Rav1dLogger>,
    dst: &mut Rav1dPicture,
    w: i32,
    src: &Rav1dPicture,
) -> Rav1dResult {
    picture_alloc_with_edges(
        logger,
        dst,
        w,
        src.p.h,
        src.seq_hdr.clone(),
        src.frame_hdr.clone(),
        src.p.bpc,
        &src.data.as_ref().unwrap().allocator,
    )?;

    rav1d_picture_copy_props(
        dst,
        src.content_light.clone(),
        src.mastering_display.clone(),
        src.itut_t35.clone(),
        src.m.clone(),
    );
    Ok(())
}
