use std::fmt;
use std::io::{stderr, Write as _};

#[derive(Clone)]
pub(crate) enum Rav1dLogger {
    Stderr,
}

/// Whether `DEBUG` names `fast-avif`, read as Node's `debug` reads it. The environment is read
/// here only: a `getenv` is not safe against a `setenv` on another thread, so an application
/// reads it once on the thread that starts it and passes the answer to every decoder's settings.
pub(crate) fn logging_from_env() -> bool {
    std::env::var("DEBUG").is_ok_and(|debug| enabled(&debug, "fast-avif"))
}

/// Whether the comma or space separated `debug` names `name`, where `*` matches any run of
/// characters and an entry starting with `-` excludes what it matches.
fn enabled(debug: &str, name: &str) -> bool {
    let mut on = false;
    for entry in debug.split(|c: char| c == ',' || c.is_whitespace()) {
        match entry.strip_prefix('-') {
            Some(excluded) if glob(excluded, name) => return false,
            Some(_) => {}
            None => on |= !entry.is_empty() && glob(entry, name),
        }
    }
    on
}

fn glob(pattern: &str, name: &str) -> bool {
    match pattern.split_once('*') {
        None => pattern == name,
        Some((head, rest)) => name.strip_prefix(head).is_some_and(|tail| {
            tail.char_indices()
                .map(|(i, _)| i)
                .chain([tail.len()])
                .any(|i| glob(rest, &tail[i..]))
        }),
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_names_as_node_reads_them() {
        for on in [
            "fast-avif",
            "*",
            "fast-*",
            "app, fast-avif",
            "app fast-avif",
            "*,-app",
        ] {
            assert!(enabled(on, "fast-avif"), "{on}");
        }
        for off in [
            "",
            "app",
            "fast-avif:*",
            "fast-avif-x",
            "*,-fast-avif",
            "-fast-avif,fast-avif",
            "*,-fast*",
        ] {
            assert!(!enabled(off, "fast-avif"), "{off}");
        }
    }
}
