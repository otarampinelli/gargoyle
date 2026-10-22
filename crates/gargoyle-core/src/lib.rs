//! gargoyle is an deterministic review engine
//!
//! this crate owns evey step of a review that doesnt need a model
//! loading checks, building the review pla, validating findings,
//! fingerprinting and dedupe, it does no network i/o and never prints

mod confidence;
mod error;
mod finding;
mod path;
mod review;
pub mod schema;
mod severity;

pub use confidence::{Confidence, InvalidConfidence};
pub use error::{Error, Result};
pub use finding::{Finding, FindingKind, Fix, Location, Side};
pub use path::{InvalidRepoPath, RepoPath};
pub use review::{ReviewOutput, Violation};
pub use severity::Severity;
