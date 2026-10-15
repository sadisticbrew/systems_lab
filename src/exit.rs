//! Exit codes shared by every command.
//!
//! | code | meaning |
//! |---|---|
//! | 0 | ok |
//! | 1 | `check-abi` findings (returned as `Ok(1)`, not an error) |
//! | 2 | usage, IO, or locator error (also any error that is not an `Exit`) |
//! | 3 | unsupported program behavior |
//! | 4 | `--strict` violation |

use std::fmt;

pub const FINDINGS: u8 = 1;
pub const USAGE: u8 = 2;
pub const UNSUPPORTED: u8 = 3;
pub const STRICT: u8 = 4;

/// An error that carries the process exit code. `main` prints `message` to
/// stderr and exits with `code`.
#[derive(Debug)]
pub struct Exit {
    pub code: u8,
    pub message: String,
}

impl fmt::Display for Exit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for Exit {}

pub fn error(code: u8, message: impl Into<String>) -> anyhow::Error {
    Exit { code, message: message.into() }.into()
}

pub fn usage(message: impl Into<String>) -> anyhow::Error {
    error(USAGE, message)
}

pub fn unsupported(message: impl Into<String>) -> anyhow::Error {
    error(UNSUPPORTED, message)
}

pub fn strict(message: impl Into<String>) -> anyhow::Error {
    error(STRICT, message)
}

/// The exit code for an error: its `Exit` code, else `USAGE`.
pub fn code_of(err: &anyhow::Error) -> u8 {
    err.downcast_ref::<Exit>().map_or(USAGE, |e| e.code)
}
