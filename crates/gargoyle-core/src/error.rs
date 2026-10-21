use std::path::PathBuf;

/// errors produced by the review engine
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// file the engine needed could not be read
    #[error("failed to read {}", .path.display())]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

/// `Result` with the engine error as the default error type
pub type Result<T, E = Error> = std::result::Result<T, E>;
