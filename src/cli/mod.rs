//! Analysis commands: `info`, `steps`, `stack`, `dot stack`, `check-abi`.

mod abi;
mod dot;
mod info;
mod stack;
mod steps;

use std::io::{BufWriter, Write};
use std::path::Path;

use anyhow::Result;

use crate::model::LoadedTrace;

pub use abi::check_abi;
pub use dot::dot_stack;
pub use info::info;
pub use stack::stack;
pub use steps::{StepsOptions, steps};

fn load(path: &Path) -> Result<LoadedTrace> {
    LoadedTrace::load(path)
}

/// Runs `f` against a buffered stdout and flushes it.
fn with_stdout<T>(f: impl FnOnce(&mut dyn Write) -> Result<T>) -> Result<T> {
    let stdout = std::io::stdout();
    let mut out = BufWriter::new(stdout.lock());
    let r = f(&mut out)?;
    out.flush()?;
    Ok(r)
}
