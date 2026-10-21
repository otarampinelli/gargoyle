use std::fmt;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// how serious a finding is
///
/// variant are declared from least to most severe, so the derived `Ord`
/// lets callers write `finding.seveirty >= confing.fail_on`
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Info,
    Warning,
    Error,
}

impl Severity {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Info => "info",
            Self::Warning => "warning",
            Self::Error => "error",
        }
    }
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn orders_from_least_to_most_severe() {
        assert!(Severity::Info < Severity::Warning);
        assert!(Severity::Warning < Severity::Error);
    }

    #[test]
    fn serialize_as_lowercase() {
        let json = serde_json::to_string(&Severity::Warning).unwrap();
        assert_eq!(json, "\"warning\"");
    }

    #[test]
    fn rejects_unkonwn_values() {
        assert!(serde_json::from_str::<Severity>("\"critical\"").is_err());
    }
}
