mod exit;
mod log;
mod pathbox;
mod writer;

pub use crate::pathbox::{Error, MagicLevel, Pathbox};
pub use exit::{exit, Status};
pub use log::{log, Level};
pub use writer::Writer;
