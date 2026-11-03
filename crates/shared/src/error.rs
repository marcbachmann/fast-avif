use std::error::Error;
use std::fmt::{self, Display, Formatter};

use strum::FromRepr;

/// Error enum return by various `rav1d` operations.
#[derive(Clone, Copy, PartialEq, Eq, FromRepr, Debug)]
#[repr(u8)]
#[non_exhaustive]
pub enum Rav1dError {
    /// This represents a generic `rav1d` error. The others carry the Linux errno values of their
    /// names, which consumers print.
    ///
    /// This forces `0` to be the niche of [`Rav1dResult`].
    Other = 1,

    /// No entity.
    ///
    /// No Sequence Header OBUs were found in the buffer.
    NoEntity = 2,

    /// Try again.
    ///
    /// If this is returned by [`rav1d_send_data`], then there
    /// are decoded frames pending that first have to be retrieved via [`rav1d_get_picture`]
    /// before processing any further pending data.
    ///
    /// If this is returned by [`rav1d_get_picture`], then no decoded frames are pending
    /// currently and more data needs to be sent to the decoder.
    TryAgain = 11,

    /// Out of memory.
    ///
    /// Not enough memory is currently available for performing this operation.
    OutOfMemory = 12,

    /// Invalid argument.
    ///
    /// One of the arguments passed to the function, including the bitstream, is invalid.
    InvalidArgument = 22,

    /// Out of range.
    ///
    /// The frame size is larger than the limit.
    OutOfRange = 34,

    /// Unsupported bitstream.
    ///
    /// The provided bitstream is not supported by `rav1d`.
    UnsupportedBitstream = 92,
}

impl Rav1dError {
    pub const fn as_str(&self) -> &'static str {
        match self {
            Rav1dError::TryAgain => "Try again",
            Rav1dError::InvalidArgument => "Invalid argument",
            Rav1dError::OutOfMemory => "Not enough memory available",
            Rav1dError::UnsupportedBitstream => "Unsupported bitstream",
            Rav1dError::Other => "Other error",
            Rav1dError::NoEntity => "No sequence header found",
            Rav1dError::OutOfRange => "Out of range",
        }
    }
}

impl Display for Rav1dError {
    fn fmt(&self, fmt: &mut Formatter) -> fmt::Result {
        write!(fmt, "{}", self.as_str())
    }
}

impl Error for Rav1dError {}

pub type Rav1dResult<T = ()> = Result<T, Rav1dError>;
