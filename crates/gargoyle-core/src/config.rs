use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use globset::Glob;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{Confidence, RepoPath, Severity};

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
    pub context: ContextConfig,
    /// extra review instructions for the files a glob matches
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub instructions: Vec<PathInstruction>,
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
    /// how checks are split into review tasks
    pub tasks: TaskMode,
}

impl Default for ReviewConfig {
    fn default() -> Self {
        Self {
            base: None,
            fail_on: FailOn::default(),
            min_confidence: Confidence::DEFAULT_MIN,
            tasks: TaskMode::default(),
        }
    }
}

/// how checks are split into review tasks
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum TaskMode {
    /// one task per group of files, with every check that applies to them,
    /// the diff is read once, so it's the cheapest
    #[default]
    Grouped,
    /// one task per check, each check gets the reviewer's full attention,
    /// but the diff is read once per check
    PerCheck,
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

/// related code reviewers get next to the changes, found with the code index,
/// it's reference for the review, never reviewed itself,
/// a task gets up to its own size of it, the same fixed size that splits the work
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct ContextConfig {
    /// show where the definitions the diff changes are used
    pub callers: bool,
    /// show the tests that use the definitions the diff changes
    pub tests: bool,
    /// project docs every reviewer follows, like a style guide, relative to the repo root,
    /// none by default since every repo keeps them somewhere else
    pub guidelines: Vec<RepoPath>,
}

impl Default for ContextConfig {
    fn default() -> Self {
        Self {
            callers: true,
            tests: true,
            guidelines: Vec::new(),
        }
    }
}

/// extra review instructions for the files a glob matches
/// cheaper than writing a whole check
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PathInstruction {
    /// glob pattern of the files these instructions are for, like 'src/api/**'
    pub path: String,
    /// what reviewers should look for in those files
    pub text: String,
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
    #[error("invalid glob {glob:?} in {field}: {source}")]
    InvalidGlob {
        field: &'static str,
        glob: String,
        source: Box<globset::Error>,
    },
    #[error("[[instructions]] for {0:?} has no text")]
    EmptyInstruction(String),
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

        let globs = config
            .files
            .exclude
            .iter()
            .map(|glob| ("files.exclude", glob))
            .chain(
                config
                    .instructions
                    .iter()
                    .map(|i| ("instructions.path", &i.path)),
            );
        for (field, glob) in globs {
            if let Err(err) = Glob::new(glob) {
                let kind = ConfigErrorKind::InvalidGlob {
                    field,
                    glob: glob.clone(),
                    source: Box::new(err),
                };
                return Err(fail(kind));
            }
        }

        if let Some(empty) = config
            .instructions
            .iter()
            .find(|i| i.text.trim().is_empty())
        {
            return Err(fail(ConfigErrorKind::EmptyInstruction(empty.path.clone())));
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
    fn context_is_on_by_default() {
        let context = parse("").unwrap().context;
        assert!(context.callers && context.tests);
        assert!(
            context.guidelines.is_empty(),
            "guidelines are only what the config lists"
        );
    }

    #[test]
    fn reads_context_and_instructions() {
        let config = parse(
            r#"
            [context]
            callers = false
            guidelines = ["docs/STYLE.md"]

            [[instructions]]
            path = "src/api/**"
            text = "Every endpoint must validate its input."
            "#,
        )
        .unwrap();
        assert!(!config.context.callers);
        assert!(config.context.tests, "unset fields keep their default");
        assert_eq!(config.context.guidelines[0].as_str(), "docs/STYLE.md");
        assert_eq!(config.instructions[0].path, "src/api/**");
    }

    #[test]
    fn guidelines_cant_point_outside_the_repo() {
        for bad in ["../secrets.txt", "/etc/passwd", "~/.ssh/id_rsa/../x"] {
            let text = format!("[context]\nguidelines = [{bad:?}]\n");
            assert!(parse(&text).is_err(), "{bad}");
        }
    }

    #[test]
    fn rejects_invalid_instruction_globs() {
        let err = parse("[[instructions]]\npath = \"src/[\"\ntext = \"x\"\n").unwrap_err();
        assert!(err.to_string().contains("instructions.path"), "{err}");
    }

    #[test]
    fn rejects_empty_instructions() {
        let err = parse("[[instructions]]\npath = \"src/**\"\ntext = \"  \"\n").unwrap_err();
        assert!(
            matches!(err.kind, ConfigErrorKind::EmptyInstruction(_)),
            "{err}"
        );
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
