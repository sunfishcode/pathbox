use std::io::Write;

#[derive(Debug, Hash, Eq, PartialEq, Clone, Copy)]
pub enum Level {
    Trace,
    Debug,
    Info,
    Warning,
    Error,
}

pub fn log<W: Write>(out: &mut W, level: Level, context: &str, message: &str) {
    // Do a very simple thing for now.
    writeln!(
        out,
        "[{} {}] {}",
        match level {
            Level::Trace => "TRACE",
            Level::Debug => "DEBUG",
            Level::Info => "INFO",
            Level::Warning => "WARN",
            Level::Error => "ERROR",
        },
        context,
        message
    )
    .unwrap();
}
