//! gargoyle is a deterministic review engine
//!
//! this crate owns every step of a review that doesn't need a model:
//! loading checks, building the review plan, validating findings,
//! fingerprinting and dedup, it does no network i/o and never prints

mod check;
mod confidence;
mod config;
mod error;
mod finding;
mod id;
mod ingest;
mod path;
mod plan;
mod planner;
mod prompt;
mod report;
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
pub use ingest::ingest;
pub use path::{InvalidRepoPath, RepoPath};
pub use plan::{PlanBase, PlanSchema, RUNS_DIR, ReviewPlan, RunId, Skipped, Task, TaskId};
pub use planner::{Plan, PlanError, PlanInput, build_plan};
pub use report::{CheckSummary, Report, ReportSchema, ReportedFinding, build_report};
pub use review::{ReviewOutput, Violation};
pub use severity::Severity;
