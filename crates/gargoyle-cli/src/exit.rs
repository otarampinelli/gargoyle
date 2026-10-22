//! process exit code

use std::process::ExitCode;

#[derive(Debug, Clone, Copy)]
#[repr(u8)]
pub(crate) enum Exit {
    Clean = 0,
    Config = 2,
    Runtime = 3,
}

impl From<Exit> for ExitCode {
    fn from(exit: Exit) -> Self {
        ExitCode::from(exit as u8)
    }
}
