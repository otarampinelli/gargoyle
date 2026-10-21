//! gargoyle is an deterministic review engine
//!
//! this crate owns evey step of a review that doesnt need a model
//! loading checks, building the review pla, validating findings,
//! fingerprinting and dedupe, it does no network i/o and never prints

mod error;
mod severity;

pub use error::{Error, Result};
pub use severity::Severity;
