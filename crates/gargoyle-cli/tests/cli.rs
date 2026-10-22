use assert_cmd::Command;

const REPO_ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");

fn gargoyle() -> Command {
    Command::new(env!("CARGO_BIN_EXE_gargoyle"))
}

#[test]
fn prints_version() {
    let stdout = gargoyle()
        .arg("--version")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let stdout = String::from_utf8(stdout).unwrap();
    assert!(
        stdout.starts_with("gargoyle "),
        "unexpected output: {stdout}"
    );
}

#[test]
fn unknown_flag_is_a_usage_error() {
    gargoyle().arg("--nope").assert().code(2);
}

#[test]
fn prints_the_review_output_schema() {
    let stdout = gargoyle()
        .args(["schema", "review-output"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let schema: serde_json::Value = serde_json::from_slice(&stdout).unwrap();
    assert_eq!(schema["title"], "ReviewOutput");
}

#[test]
fn printed_schemas_match_the_committed_files() {
    for (name, file) in [
        ("review-output", "review-output.v1.json"),
        ("check", "check.v1.json"),
    ] {
        let stdout = gargoyle()
            .args(["schema", name])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone();
        let committed = std::fs::read(format!("{REPO_ROOT}/schemas/{file}")).unwrap();
        assert!(
            stdout == committed,
            "`gargoyle schema {name}` differs from schemas/{file}"
        );
    }
}

#[test]
fn built_in_checks_are_valid() {
    let stdout = gargoyle()
        .current_dir(REPO_ROOT)
        .args(["checks", "validate", "--dir", "checks"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert!(
        String::from_utf8(stdout)
            .unwrap()
            .starts_with("5 checks valid")
    );
}

#[test]
fn lists_checks_as_json_sorted_by_id() {
    let stdout = gargoyle()
        .current_dir(REPO_ROOT)
        .args(["checks", "list", "--json", "--dir", "checks"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let checks: Vec<serde_json::Value> = serde_json::from_slice(&stdout).unwrap();
    let ids: Vec<_> = checks.iter().map(|c| c["id"].as_str().unwrap()).collect();
    assert_eq!(
        ids,
        [
            "anti-slop",
            "code-quality",
            "security-review",
            "simplicity",
            "test-coverage"
        ]
    );
}

#[test]
fn invalid_checks_exit_with_a_config_error() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("broken.md"), "no frontmatter").unwrap();

    let output = gargoyle()
        .args(["checks", "validate", "--dir"])
        .arg(dir.path())
        .assert()
        .code(2)
        .get_output()
        .stderr
        .clone();
    let stderr = String::from_utf8(output).unwrap();
    assert!(
        stderr.contains("broken.md: missing frontmatter"),
        "{stderr}"
    );
}

#[test]
fn missing_checks_dir_is_a_config_error() {
    let dir = tempfile::tempdir().unwrap();
    gargoyle()
        .args(["checks", "list", "--dir"])
        .arg(dir.path().join("nope"))
        .assert()
        .code(2);
}
