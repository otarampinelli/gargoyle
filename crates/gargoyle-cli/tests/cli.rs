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
        ("config", "config.v1.json"),
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
        .args(["checks", "validate"])
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

#[test]
fn init_sets_up_a_repo_that_validates() {
    let dir = tempfile::tempdir().unwrap();
    gargoyle()
        .current_dir(dir.path())
        .arg("init")
        .assert()
        .success();
    assert!(dir.path().join("gargoyle.toml").is_file());

    let stdout = gargoyle()
        .current_dir(dir.path())
        .args(["checks", "validate"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let stdout = String::from_utf8(stdout).unwrap();
    assert!(
        stdout.starts_with("5 checks valid in .gargoyle/checks"),
        "{stdout}"
    );
}

#[test]
fn init_needs_force_to_overwrite_the_config() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("gargoyle.toml"), "# mine\n").unwrap();

    gargoyle()
        .current_dir(dir.path())
        .arg("init")
        .assert()
        .code(2);
    let kept = std::fs::read_to_string(dir.path().join("gargoyle.toml")).unwrap();
    assert_eq!(kept, "# mine\n");

    gargoyle()
        .current_dir(dir.path())
        .args(["init", "--force"])
        .assert()
        .success();
}

#[test]
fn init_never_overwrites_existing_checks() {
    let dir = tempfile::tempdir().unwrap();
    let checks = dir.path().join(".gargoyle/checks");
    std::fs::create_dir_all(&checks).unwrap();
    std::fs::write(checks.join("anti-slop.md"), "my edits").unwrap();

    gargoyle()
        .current_dir(dir.path())
        .arg("init")
        .assert()
        .success();
    let kept = std::fs::read_to_string(checks.join("anti-slop.md")).unwrap();
    assert_eq!(kept, "my edits");
}

#[test]
fn invalid_config_is_a_config_error() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("gargoyle.toml"),
        "[review]\nfail-on = \"warning\"\n",
    )
    .unwrap();

    let stderr = gargoyle()
        .current_dir(dir.path())
        .args(["checks", "list"])
        .assert()
        .code(2)
        .get_output()
        .stderr
        .clone();
    let stderr = String::from_utf8(stderr).unwrap();
    assert!(stderr.contains("gargoyle.toml"), "{stderr}");
}

/// runs git in `dir` with a fixed identity, so tests work with any global config
#[allow(clippy::unwrap_used)]
fn git(dir: &std::path::Path, args: &[&str]) {
    let status = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["-c", "user.name=test", "-c", "user.email=test@example.com"])
        .args(["-c", "commit.gpgsign=false"])
        .args(args)
        .status()
        .unwrap();
    assert!(status.success(), "git {args:?} failed");
}

/// a repo on `feature` that adds new.txt on top of `main`
#[allow(clippy::unwrap_used)]
fn feature_repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    git(dir.path(), &["init", "-q", "-b", "main"]);
    std::fs::write(dir.path().join("a.txt"), "one\n").unwrap();
    git(dir.path(), &["add", "-A"]);
    git(dir.path(), &["commit", "-q", "-m", "init"]);
    git(dir.path(), &["checkout", "-q", "-b", "feature"]);
    std::fs::write(dir.path().join("new.txt"), "hello\n").unwrap();
    git(dir.path(), &["add", "-A"]);
    git(dir.path(), &["commit", "-q", "-m", "feature work"]);
    dir
}

/// `gargoyle diff` without the CI env vars of whatever machine runs the tests
fn gargoyle_diff(dir: &std::path::Path) -> Command {
    let mut cmd = gargoyle();
    cmd.current_dir(dir)
        .env_remove("GITHUB_BASE_REF")
        .env_remove("CI_MERGE_REQUEST_TARGET_BRANCH_NAME")
        .arg("diff");
    cmd
}

#[test]
fn diff_prints_the_branch_changes() {
    let dir = feature_repo();
    let output = gargoyle_diff(dir.path())
        .assert()
        .success()
        .get_output()
        .clone();
    let stdout = String::from_utf8(output.stdout).unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stdout.contains("+++ b/new.txt"), "{stdout}");
    assert!(stderr.contains("comparing against main"), "{stderr}");
    assert!(stderr.contains("1 file changed, +1 -0"), "{stderr}");
}

#[test]
fn diff_includes_uncommitted_and_untracked_files() {
    let dir = feature_repo();
    std::fs::write(dir.path().join("a.txt"), "not committed\n").unwrap();
    std::fs::write(dir.path().join("brand-new.txt"), "untracked\n").unwrap();

    let stdout = gargoyle_diff(dir.path())
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let stdout = String::from_utf8(stdout).unwrap();
    for line in ["+++ b/new.txt", "+not committed", "+++ b/brand-new.txt"] {
        assert!(stdout.contains(line), "missing {line}:\n{stdout}");
    }
}

#[test]
fn diff_with_no_changes_says_so() {
    let dir = feature_repo();
    let output = gargoyle_diff(dir.path())
        .args(["--base", "HEAD"])
        .assert()
        .success()
        .get_output()
        .clone();
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "");
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("no changes to review"), "{stderr}");
}

#[test]
fn diff_uses_the_base_from_the_config() {
    let dir = feature_repo();
    std::fs::write(
        dir.path().join("gargoyle.toml"),
        "[review]\nbase = \"nope\"\n",
    )
    .unwrap();
    let stderr = gargoyle_diff(dir.path())
        .assert()
        .code(2)
        .get_output()
        .stderr
        .clone();
    let stderr = String::from_utf8(stderr).unwrap();
    assert!(
        stderr.contains("\"nope\" does not point to a commit"),
        "{stderr}"
    );
}

#[test]
fn diff_outside_a_repo_is_a_config_error() {
    let dir = tempfile::tempdir().unwrap();
    gargoyle_diff(dir.path()).assert().code(2);
}
