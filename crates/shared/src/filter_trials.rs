//! The encoder's search of the deblocking and CDEF strengths runs on a decoder: the decoder
//! implements the trials, the encoder takes them by reference.

use std::ops::Range;

use crate::error::Rav1dResult;
use crate::headers::Rav1dFrameHeader;

/// What a filter trial measures of rows of a plane: its index, the pixels from the first of the
/// rows, their stride and width, and the rows. The sums of a plane's rows make its measure.
pub type Measure<'a> = dyn FnMut(usize, &[u8], usize, usize, Range<usize>) -> u64 + 'a;

/// One filter trial: puts the unfiltered picture back, filters it under a copy of the frame
/// header as the edit leaves it, and measures each plane.
pub type Trial<'a> = dyn FnMut(&dyn Fn(&mut Rav1dFrameHeader)) -> [u64; 3] + 'a;

pub trait FilterTrials {
    /// Decodes `data`, one frame, with the in-loop filters its header has, and measures each of
    /// its `planes`.
    fn decoded(&self, data: Box<[u8]>, planes: usize, measure: &mut Measure) -> [u64; 3];

    /// Decodes `data`, one frame, once, and lets `search` filter it under headers of its choice,
    /// as many as it likes. `Err` where the frame cannot be tried so: nothing was run.
    fn trials(
        &self,
        data: Box<[u8]>,
        measure: &mut Measure,
        search: &mut dyn FnMut(&mut Trial),
    ) -> Rav1dResult;
}
