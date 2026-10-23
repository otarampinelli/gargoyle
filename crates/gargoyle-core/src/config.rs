use std::fs;
use std::io::ErrorKind;
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};

use globset::Glob;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{Confidence, Severity};

/// file name of the project config, at the repo root
pub const CONFIG_FILE: &str = "gargoyle.toml";

/// project settings from gargoyle.toml, every field has a default,
/// so an empty file is valid
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub review: ReviewConfig,
    pub files: FilesConfig,
    pub checks: ChecksConfig,
}

/// how a review runs and when it fails
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct ReviewConfig {
    /// branch or commit to compare against,
    /// detected from the remote default branch when unset
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base: Option<String>,
    /// lowest severity that makes a review exit with code 1
    pub fail_on: FailOn,
    /// drop findings below this confidence, a check can override it
    pub min_confidence: Confidence,
    /// token budget for a whole review run
    pub max_tokens: NonZeroU32,
}

impl Default for ReviewConfig {
    fn default() -> Self {
        Self {
            base: None,
            fail_on: FailOn::default(),
            min_confidence: Confidence::DEFAULT_MIN,
            max_tokens: NonZeroU32::new(120_000).unwrap_or(NonZeroU32::MIN),
        }
    }
}

/// lowest severity that fails a review
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum FailOn {
    Info,
    Warning,
    #[default]
    Error,
    /// never fail, only report
    Never,
}

impl FailOn {
    /// whether a finding of this severity fails the review
    pub fn fails(self, severity: Severity) -> bool {
        match self {
            Self::Info => true,
            Self::Warning => severity >= Severity::Warning,
            Self::Error => severity >= Severity::Error,
            Self::Never => false,
        }
    }
}

/// which files are reviewed at all
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct FilesConfig {
    /// glob patterns of files never to review, like generated code
    pub exclude: Vec<String>,
}

/// where checks come from
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct ChecksConfig {
    /// directory containing the check files, relative to the repo root
    pub dir: PathBuf,
}

impl Default for ChecksConfig {
    fn default() -> Self {
        Self {
            dir: PathBuf::from(".gargoyle/checks"),
        }
    }
}

/// a problem with the config file
#[derive(Debug, thiserror::Error)]
#[error("{}: {kind}", .path.display())]
pub struct ConfigError {
    pub path: PathBuf,
    pub kind: ConfigErrorKind,
}

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ConfigErrorKind {
    #[error("could not read: {0}")]
    Read(std::io::Error),
    #[error("invalid config: {0}")]
    Toml(Box<toml::de::Error>),
    #[error("invalid glob {glob:?} in files.exclude: {source}")]
    InvalidGlob {
        glob: String,
        source: Box<globset::Error>,
    },
}

impl Config {
    /// parses the text of a config file, path is only used in errors
    pub fn parse(path: &Path, text: &str) -> Result<Self, ConfigError> {
        let fail = |kind: ConfigErrorKind| ConfigError {
            path: path.to_owned(),
            kind,
        };

        let config: Self =
            toml::from_str(text).map_err(|err| fail(ConfigErrorKind::Toml(Box::new(err))))?;

        for glob in &config.files.exclude {
            if let Err(err) = Glob::new(glob) {
                let kind = ConfigErrorKind::InvalidGlob {
                    glob: glob.clone(),
                    source: Box::new(err),
                };
                return Err(fail(kind));
            }
        }
        Ok(config)
    }

    /// loads the config at path, or the default if there is no file
    pub fn load_or_default(path: &Path) -> Result<Self, ConfigError> {
        match fs::read_to_string(path) {
            Ok(text) => Self::parse(path, &text),
            Err(err) if err.kind() == ErrorKind::NotFound => Ok(Self::default()),
            Err(err) => Err(ConfigError {
                path: path.to_owned(),
                kind: ConfigErrorKind::Read(err),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> Result<Config, ConfigError> {
        Config::parse(Path::new(CONFIG_FILE), text)
    }

    #[test]
    fn empty_file_gives_the_defaults() {
        let config = parse("").unwrap();
        assert_eq!(config, Config::default());
        assert_eq!(config.review.fail_on, FailOn::Error);
        assert_eq!(config.checks.dir, Path::new(".gargoyle/checks"));
    }

    #[test]
    fn reads_every_section() {
        let config = parse(
            r#"
            [review]
            base = "develop"
            fail_on = "warning"
            min_confidence = 0.8
            max_tokens = 50000

            [files]
            exclude = ["dist/**"]

            [checks]
            dir = "review-checks"
            "#,
        )
        .unwrap();
        assert_eq!(config.review.base.as_deref(), Some("develop"));
        assert_eq!(config.review.fail_on, FailOn::Warning);
        assert!((config.review.min_confidence.get() - 0.8).abs() < f64::EPSILON);
        assert_eq!(config.review.max_tokens.get(), 50_000);
        assert_eq!(config.files.exclude, ["dist/**"]);
        assert_eq!(config.checks.dir, Path::new("review-checks"));
    }

    #[test]
    fn rejects_unknown_fields() {
        let err = parse("[review]\nfail-on = \"warning\"\n").unwrap_err();
        assert!(err.to_string().contains("fail-on"), "{err}");
    }

    #[test]
    fn rejects_out_of_range_confidence() {
        assert!(parse("[review]\nmin_confidence = 2.0\n").is_err());
    }

    #[test]
    fn rejects_invalid_globs() {
        let err = parse("[files]\nexclude = [\"src/[\"]\n").unwrap_err();
        assert!(matches!(err.kind, ConfigErrorKind::InvalidGlob { .. }));
    }

    #[test]
    fn missing_file_gives_the_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let config = Config::load_or_default(&dir.path().join(CONFIG_FILE)).unwrap();
        assert_eq!(config, Config::default());
    }

    #[test]
    fn fail_on_thresholds() {
        use Severity::{Error, Info, Warning};
        assert!(FailOn::Info.fails(Info));
        assert!(FailOn::Warning.fails(Warning) && !FailOn::Warning.fails(Info));
        assert!(FailOn::Error.fails(Error) && !FailOn::Error.fails(Warning));
        assert!(!FailOn::Never.fails(Error));
    }
}
