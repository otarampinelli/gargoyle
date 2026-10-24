//! git access for gargoyle, it shells out to the `git` binary
//!
//! this crate finds the repo, picks the base to compare against and
//! collects the raw diff, it never writes to the repo

mod base;
mod error;
mod files;
mod parse;
mod repo;

#[cfg(test)]
mod testing;

pub use base::{Base, BaseSource, CiEnv};
pub use error::GitError;
pub use files::TreeFile;
pub use parse::{DiffParseError, FileDiff, FileStatus, Hunk, Line, LineKind, parse_diff};
pub use repo::Repo;
