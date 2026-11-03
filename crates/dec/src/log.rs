use std::fmt;
use std::io::{stderr, stdout, Write as _};

#[derive(Clone, Default)]
pub(crate) enum Rav1dLogger {
    #[allow(dead_code)]
    Stdout,
    #[default]
    Stderr,
}

/// Any type implementing [`Rav1dLog`] can be used with [`write!`].
///
/// [`Rav1dLog`] is very similar to [`fmt::Write`] and [`io::Write`]
/// in that they can both be called by [`write!`] as `.write_fmt(format_args!(...))`.
///
/// The difference, and the reason we don't use either of those,
/// is that [`Rav1dLog::write_fmt`] takes `&self` instead of `&mut self`,
/// and this makes ownership and borrowing much simpler and more flexible.
/// Furthermore, this returns `()` instead of a [`Result`]
/// so that call sites don't have to propagate or `.unwrap()` it,
/// bloating call sites for non-essential logging code.
///
/// [`io::Write`]: std::io::Write
pub trait Rav1dLog {
    fn write_fmt(&self, args: fmt::Arguments);
}

impl Rav1dLog for Rav1dLogger {
    /// Logging doesn't have to be fast when it's on,
    /// but we don't want it slow things down when it's off,
    /// so ensure the logging code isn't inlined everywhere, bloating call sites.
    #[inline(never)]
    fn write_fmt(&self, args: fmt::Arguments) {
        match self {
            Self::Stdout => stdout().write_fmt(args).unwrap(),
            Self::Stderr => stderr().write_fmt(args).unwrap(),
        }
    }
}

impl Rav1dLog for Option<Rav1dLogger> {
    /// When a logger isn't set, we don't want to have to run any code here,
    /// so force this to be inlined so a [`None`] can be seen.
    #[inline(always)]
    fn write_fmt(&self, args: fmt::Arguments) {
        if let Some(logger) = self {
            logger.write_fmt(args);
        }
    }
}
