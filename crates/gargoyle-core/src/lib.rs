//! gargoyle is an deterministic review engine
//!
//! this crate owns evey step of a review that doesnt need a model
//! loading checks, building the review pla, validating findings,
//! fingerprinting and dedupe, it does no network i/o and never prints

mod check;
mod confidence;
mod config;
mod error;
mod finding;
mod id;
mod path;
mod review;
pub mod schema;
mod severity;

pub use check::{
    AppliesTo, Check, CheckError, CheckErrorKind, CheckSchema, CheckSpec, ContextMode, ContextSpec,
    load_checks,
};

pub use config::{
    CONFIG_FILE, ChecksConfig, Config, ConfigError, ConfigErrorKind, FailOn, FilesConfig,
    ReviewConfig,
};

pub use confidence::{Confidence, InvalidConfidence};
pub use error::{Error, Result};
pub use finding::{Finding, FindingKind, Fix, Location, Side};
pub use id::{CheckId, InvalidCheckId};
pub use path::{InvalidRepoPath, RepoPath};
pub use review::{ReviewOutput, Violation};
pub use severity::Severity;
