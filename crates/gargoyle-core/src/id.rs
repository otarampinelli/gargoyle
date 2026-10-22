use std::borrow::Cow;
use std::fmt;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Serialize};

const MAX_LEN: usize = 64;

/// Stable identifier of a check, e.g. `security-review`.
///
/// Lowercase letters, digits and single hyphens, at most 64 characters.
/// It is part of every finding's fingerprint, so renaming a check resets
/// its baseline.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct CheckId(String);

impl CheckId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

fn is_valid(id: &str) -> bool {
    id.len() <= MAX_LEN
        && id.split('-').all(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        })
}

/// Error returned when a string is not a valid [`CheckId`].
#[derive(Debug, thiserror::Error)]
#[error("invalid check id {0:?}: use lowercase letters, digits and single hyphens (max 64 chars)")]
pub struct InvalidCheckId(String);

impl TryFrom<String> for CheckId {
    type Error = InvalidCheckId;

    fn try_from(id: String) -> Result<Self, Self::Error> {
        if is_valid(&id) {
            Ok(Self(id))
        } else {
            Err(InvalidCheckId(id))
        }
    }
}

impl From<CheckId> for String {
    fn from(id: CheckId) -> Self {
        id.0
    }
}

impl fmt::Display for CheckId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl JsonSchema for CheckId {
    fn schema_name() -> Cow<'static, str> {
        "CheckId".into()
    }

    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "type": "string",
            "pattern": "^[a-z0-9]+(-[a-z0-9]+)*$",
            "maxLength": MAX_LEN,
            "description": "Stable check id: lowercase letters, digits and single hyphens. Must match the file name."
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(id: &str) -> Result<CheckId, InvalidCheckId> {
        CheckId::try_from(id.to_owned())
    }

    #[test]
    fn accepts_kebab_case() {
        for ok in ["security", "security-review", "owasp-top-10"] {
            assert!(parse(ok).is_ok(), "{ok:?} should be accepted");
        }
    }

    #[test]
    fn accepts_the_maximum_length() {
        assert!(parse(&"a".repeat(MAX_LEN)).is_ok());
    }

    #[test]
    fn rejects_everything_else() {
        let too_long = "a".repeat(MAX_LEN + 1);
        for bad in [
            "",
            "Security",
            "security_review",
            "-x",
            "x-",
            "a--b",
            "a b",
            &too_long,
        ] {
            assert!(parse(bad).is_err(), "{bad:?} should be rejected");
        }
    }
}
