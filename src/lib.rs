mod exit;
mod log;
mod preopener;
mod writer;

pub use crate::preopener::{Error, MagicLevel, Preopener};
pub use exit::{exit, Status};
pub use log::{log, Level};
pub use writer::Writer;
