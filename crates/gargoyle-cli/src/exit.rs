//! process exit codes

use std::fmt;
use std::process::ExitCode;

#[derive(Debug, Clone, Copy)]
#[repr(u8)]
pub(crate) enum Exit {
    Clean = 0,
    /// the report has a finding at or above `review.fail_on`
    Findings = 1,
    Config = 2,
    Runtime = 3,
    /// a reviewer's answer didn't pass validation
    InvalidOutput = 4,
}

impl From<Exit> for ExitCode {
    fn from(exit: Exit) -> Self {
        ExitCode::from(exit as u8)
    }
}

/// an error you can fix in your setup, it exits with 2 instead of 3
#[derive(Debug)]
pub(crate) struct ConfigError(String);

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ConfigError {}

pub(crate) fn config_error(message: impl fmt::Display) -> anyhow::Error {
    ConfigError(message.to_string()).into()
}
