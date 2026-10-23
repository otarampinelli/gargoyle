use std::path::PathBuf;

/// a problem talking to git
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum GitError {
    #[error("git is not installed or not on PATH")]
    NotInstalled,
    #[error("could not run git: {0}")]
    Io(#[source] std::io::Error),
    #[error("{} is not inside a git repository", .0.display())]
    NotARepo(PathBuf),
    #[error("base {0:?} does not point to a commit")]
    UnknownBase(String),
    #[error("could not detect a base branch, set review.base in gargoyle.toml or pass --base")]
    NoBase,
    #[error("{0} and HEAD have no commit in common")]
    NoCommonHistory(String),
    #[error("`git {command}` failed: {stderr}")]
    Failed { command: String, stderr: String },
}

impl GitError {
    /// whether the user can fix this in their config or flags, not a broken setup
    pub fn is_config(&self) -> bool {
        matches!(
            self,
            Self::NotARepo(_) | Self::UnknownBase(_) | Self::NoBase | Self::NoCommonHistory(_)
        )
    }
}
