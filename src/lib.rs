//! The AV1 still-image encoder and the decoder it is checked against: `rav1d_shared`, the
//! decoder `rav1d_dec` and the encoder, re-exported under the paths they always had.
#![deny(unsafe_op_in_unsafe_fn, clippy::missing_safety_doc)]

pub mod include {
    pub mod common {}
    pub mod dav1d {
        pub use rav1d_dec::include::dav1d::dav1d;
        pub use rav1d_shared::include::dav1d::{common, data, headers, picture};
    }
}

pub use rav1d_dec::rust_api;
pub use rav1d_dec::rust_api::*;
pub use rav1d_dec::{
    rav1d_version, DAV1D_API_VERSION_MAJOR, DAV1D_API_VERSION_MINOR, DAV1D_API_VERSION_PATCH,
};
pub use rav1d_shared::error::{Dav1dResult, Rav1dError, Rav1dResult};
pub use rav1d_shared::{align, cpu, send_sync_non_null, Spawner};

/// The encoder, its filter search decoding through `rav1d_dec`.
#[cfg(feature = "encoder")]
pub mod enc {
    use std::sync::Arc;

    pub use rav1d_enc::*;

    /// A still picture of 8-bit planes: one plane is monochrome, three are 4:4:4 YCbCr, BT.601 full
    /// range, sRGB.
    pub fn still(width: u32, height: u32, planes: &[&[u8]], options: &Options) -> frame::Coded {
        rav1d_enc::still(width, height, planes, options, &rav1d_dec::DecoderTrials)
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
        rav1d_enc::still_on(width, height, planes, options, spawner, &rav1d_dec::DecoderTrials)
    }
}
