//! The AV1 still-image encoder and the decoder it is checked against, `fast_avif_enc` and `fast_avif_dec`.
#![deny(unsafe_op_in_unsafe_fn, clippy::missing_safety_doc)]

pub use fast_avif_dec::rust_api::*;
pub use fast_avif_dec::{rav1d_version, rust_api};
pub use fast_avif_shared::error::{Rav1dError, Rav1dResult};
pub use fast_avif_shared::{align, cpu, headers, Spawner};

/// The encoder, its filter search decoding through `fast_avif_dec`.
#[cfg(feature = "encoder")]
pub mod enc {
    use std::sync::Arc;

    pub use fast_avif_enc::*;

    /// A still picture of 8-bit planes: one plane is monochrome, three are 4:4:4 YCbCr, BT.601 full
    /// range, sRGB.
    pub fn still(width: u32, height: u32, planes: &[&[u8]], options: &Options) -> frame::Coded {
        fast_avif_enc::still(
            width,
            height,
            planes,
            options,
            &fast_avif_dec::DecoderTrials,
        )
    }

    /// `still` with the tile columns after the first coded on threads of `spawner`, as many as it
    /// takes; the output is the same.
    pub fn still_on(
        width: u32,
        height: u32,
        planes: Arc<Vec<Vec<u8>>>,
        options: &Options,
        spawner: Option<&crate::Spawner>,
    ) -> frame::Coded {
        fast_avif_enc::still_on(
            width,
            height,
            planes,
            options,
            spawner,
            &fast_avif_dec::DecoderTrials,
        )
    }
}
