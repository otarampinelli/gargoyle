use std::borrow::Cow;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Serialize};

/// How sure the reviewer is that a finding is real, from 0.0 to 1.0.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(try_from = "f64", into = "f64")]
pub struct Confidence(f64);

impl Confidence {
    pub const fn get(self) -> f64 {
        self.0
    }
}

/// Error returned when a number is not a valid [`Confidence`].
#[derive(Debug, thiserror::Error)]
#[error("confidence must be between 0.0 and 1.0, got {0}")]
pub struct InvalidConfidence(f64);

impl TryFrom<f64> for Confidence {
    type Error = InvalidConfidence;

    fn try_from(value: f64) -> Result<Self, Self::Error> {
        // `contains` is false for NaN, so NaN is rejected too.
        if (0.0..=1.0).contains(&value) {
            Ok(Self(value))
        } else {
            Err(InvalidConfidence(value))
        }
    }
}

impl From<Confidence> for f64 {
    fn from(confidence: Confidence) -> Self {
        confidence.0
    }
}

impl JsonSchema for Confidence {
    fn schema_name() -> Cow<'static, str> {
        "Confidence".into()
    }

    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "type": "number",
            "minimum": 0.0,
            "maximum": 1.0,
            "description": "How sure you are this is a real problem, from 0.0 to 1.0."
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_the_closed_unit_range() {
        for ok in [0.0, 0.5, 1.0] {
            assert!(Confidence::try_from(ok).is_ok());
        }
    }

    #[test]
    fn rejects_out_of_range_and_nan() {
        for bad in [-0.1, 1.01, f64::NAN] {
            assert!(Confidence::try_from(bad).is_err());
        }
    }
}
