//! Keeps the committed schemas in `schemas/` in sync with the Rust types.
//!
//! To regenerate after changing a type:
//! `UPDATE_SCHEMAS=1 cargo test -p gargoyle-core --test schemas`

use std::error::Error;
use std::fs;
use std::path::PathBuf;

type TestResult = Result<(), Box<dyn Error>>;

fn check(file: &str, schema: &schemars::Schema) -> TestResult {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../schemas");
    let path = dir.join(file);
    let generated = serde_json::to_string_pretty(schema)? + "\n";

    if std::env::var_os("UPDATE_SCHEMAS").is_some() {
        fs::create_dir_all(&dir)?;
        fs::write(&path, generated)?;
        return Ok(());
    }

    let committed = fs::read_to_string(&path).unwrap_or_default();
    assert!(
        committed == generated,
        "schemas/{file} is out of date. Run: UPDATE_SCHEMAS=1 cargo test -p gargoyle-core --test schemas"
    );
    Ok(())
}

#[test]
fn review_output_schema_is_up_to_date() -> TestResult {
    check(
        "review-output.v1.json",
        &gargoyle_core::schema::review_output(),
    )
}

#[test]
fn check_schema_is_up_to_date() -> TestResult {
    check("check.v1.json", &gargoyle_core::schema::check())
}
