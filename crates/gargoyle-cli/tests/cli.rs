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
        ("plan", "plan.v1.json"),
        ("report", "report.v1.json"),
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
            .starts_with("6 checks valid")
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
            "correctness",
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
        .args(["init", "--no-skill"])
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
        stdout.starts_with("6 checks valid in .gargoyle/checks"),
        "{stdout}"
    );
}

#[test]
fn init_keeps_the_config_unless_forced() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("gargoyle.toml");
    std::fs::write(&config, "# mine\n").unwrap();

    // running init again is fine, it only fills in what's missing
    gargoyle()
        .current_dir(dir.path())
        .args(["init", "--no-skill"])
        .assert()
        .success();
    assert_eq!(std::fs::read_to_string(&config).unwrap(), "# mine\n");

    gargoyle()
        .current_dir(dir.path())
        .args(["init", "--force", "--no-skill"])
        .assert()
        .success();
    assert_ne!(std::fs::read_to_string(&config).unwrap(), "# mine\n");
}

#[test]
fn init_never_overwrites_existing_checks() {
    let dir = tempfile::tempdir().unwrap();
    let checks = dir.path().join(".gargoyle/checks");
    std::fs::create_dir_all(&checks).unwrap();
    std::fs::write(checks.join("anti-slop.md"), "my edits").unwrap();

    gargoyle()
        .current_dir(dir.path())
        .args(["init", "--no-skill"])
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

/// gargoyle in `dir`, without the CI env vars of whatever machine runs the tests
fn gargoyle_in(dir: &std::path::Path) -> Command {
    let mut cmd = gargoyle();
    cmd.current_dir(dir)
        .env_remove("GITHUB_BASE_REF")
        .env_remove("CI_MERGE_REQUEST_TARGET_BRANCH_NAME");
    cmd
}

fn gargoyle_diff(dir: &std::path::Path) -> Command {
    let mut cmd = gargoyle_in(dir);
    cmd.arg("diff");
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

/// a repo set up with `gargoyle init`, on a branch that adds a hardcoded password
#[allow(clippy::unwrap_used)]
fn review_repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    git(dir.path(), &["init", "-q", "-b", "main"]);
    gargoyle_in(dir.path())
        .args(["init", "--no-skill"])
        .assert()
        .success();
    git(dir.path(), &["add", "-A"]);
    git(dir.path(), &["commit", "-q", "-m", "init"]);
    git(dir.path(), &["checkout", "-q", "-b", "feature"]);
    std::fs::create_dir(dir.path().join("src")).unwrap();
    std::fs::write(
        dir.path().join("src/db.js"),
        "const db = {\n  password: \"hunter2\",\n};\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("package-lock.json"), "{}\n").unwrap();
    dir
}

#[allow(clippy::unwrap_used)]
fn prepare(dir: &std::path::Path) -> serde_json::Value {
    let stdout = gargoyle_in(dir)
        .args(["prepare", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    serde_json::from_slice(&stdout).unwrap()
}

const SECRET_ANSWER: &str = r#"{"summary": "A password is hardcoded.", "findings": [{
    "check": "security-review", "rule": "hardcoded-secret", "title": "Hardcoded database password",
    "severity": "error", "confidence": 0.95, "kind": "design",
    "location": {"path": "src/db.js", "start_line": 2, "end_line": 2},
    "message": "The password is committed in source.",
    "fix": {"description": "Read it from DB_PASSWORD."}}]}"#;

#[test]
fn review_from_prepare_to_report() {
    let dir = review_repo();
    let plan = prepare(dir.path());
    let run = plan["run_id"].as_str().unwrap();

    // one small file, so every check reviews it in one task
    let tasks = plan["tasks"].as_array().unwrap();
    assert_eq!(tasks.len(), 1, "{plan}");
    assert_eq!(tasks[0]["id"], "review-1");
    assert_eq!(tasks[0]["checks"].as_array().unwrap().len(), 6);
    assert_eq!(plan["skipped"][0]["path"], "package-lock.json");
    let prompt = dir.path().join(tasks[0]["prompt_path"].as_str().unwrap());
    assert!(std::fs::read_to_string(prompt).unwrap().contains("hunter2"));

    gargoyle_in(dir.path())
        .args(["ingest", run, "review-1"])
        .write_stdin(SECRET_ANSWER)
        .assert()
        .success();

    // an error finding fails the review with the default fail_on
    let stdout = gargoyle_in(dir.path())
        .args(["report", run, "--format", "json"])
        .assert()
        .code(1)
        .get_output()
        .stdout
        .clone();
    let report: serde_json::Value = serde_json::from_slice(&stdout).unwrap();
    assert_eq!(report["findings"].as_array().unwrap().len(), 1);
    assert_eq!(report["findings"][0]["check_id"], "security-review");
    assert_eq!(report["missing_tasks"].as_array().unwrap().len(), 0);

    let stdout = gargoyle_in(dir.path())
        .args(["report", run])
        .assert()
        .code(1)
        .get_output()
        .stdout
        .clone();
    let stdout = String::from_utf8(stdout).unwrap();
    assert!(
        stdout.contains("error    Hardcoded database password"),
        "{stdout}"
    );
    assert!(
        stdout.contains("passed: anti-slop, code-quality, correctness, simplicity, test-coverage"),
        "{stdout}"
    );
}

#[test]
fn per_check_tasks_still_work() {
    let dir = review_repo();
    let config = dir.path().join("gargoyle.toml");
    let text = std::fs::read_to_string(&config).unwrap();
    std::fs::write(
        &config,
        text.replace("tasks = \"grouped\"", "tasks = \"per-check\""),
    )
    .unwrap();

    let plan = prepare(dir.path());
    let run = plan["run_id"].as_str().unwrap();
    assert_eq!(plan["tasks"].as_array().unwrap().len(), 6, "{plan}");

    // a task with one check doesn't need `check` in its findings
    let answer = SECRET_ANSWER.replace(r#""check": "security-review", "#, "");
    gargoyle_in(dir.path())
        .args(["ingest", run, "security-review-1"])
        .write_stdin(answer)
        .assert()
        .success();
    let stdout = gargoyle_in(dir.path())
        .args(["report", run, "--format", "json"])
        .assert()
        .code(1)
        .get_output()
        .stdout
        .clone();
    let report: serde_json::Value = serde_json::from_slice(&stdout).unwrap();
    assert_eq!(report["findings"][0]["check_id"], "security-review");
    assert_eq!(report["missing_tasks"].as_array().unwrap().len(), 5);
}

#[test]
fn invalid_answers_exit_with_4() {
    let dir = review_repo();
    let plan = prepare(dir.path());
    let run = plan["run_id"].as_str().unwrap();

    let stderr = gargoyle_in(dir.path())
        .args(["ingest", run, "review-1"])
        .write_stdin("RESULT: PASS")
        .assert()
        .code(4)
        .get_output()
        .stderr
        .clone();
    let stderr = String::from_utf8(stderr).unwrap();
    assert!(stderr.contains("no JSON object found"), "{stderr}");
}

#[test]
fn runs_stay_out_of_git() {
    let dir = review_repo();
    prepare(dir.path());
    let status = std::process::Command::new("git")
        .arg("-C")
        .arg(dir.path())
        .args(["status", "--porcelain"])
        .output()
        .unwrap();
    let status = String::from_utf8(status.stdout).unwrap();
    assert!(!status.contains(".gargoyle/runs"), "{status}");
}

#[test]
fn prepare_only_runs_the_checks_you_name() {
    let dir = review_repo();
    let stdout = gargoyle_in(dir.path())
        .args([
            "prepare",
            "--json",
            "--checks",
            "simplicity,security-review",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let plan: serde_json::Value = serde_json::from_slice(&stdout).unwrap();
    let checks: Vec<_> = plan["tasks"][0]["checks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["id"].as_str().unwrap())
        .collect();
    assert_eq!(checks, ["security-review", "simplicity"]);

    gargoyle_in(dir.path())
        .args(["prepare", "--checks", "nope"])
        .assert()
        .code(2);
}

#[test]
fn prepare_with_no_changes_has_no_tasks() {
    let dir = tempfile::tempdir().unwrap();
    git(dir.path(), &["init", "-q", "-b", "main"]);
    gargoyle_in(dir.path())
        .args(["init", "--no-skill"])
        .assert()
        .success();
    git(dir.path(), &["add", "-A"]);
    git(dir.path(), &["commit", "-q", "-m", "init"]);

    let stdout = gargoyle_in(dir.path())
        .args(["prepare", "--base", "HEAD"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(String::from_utf8(stdout).unwrap(), "no changes to review\n");
}

#[test]
fn report_needs_a_real_run_id() {
    let dir = review_repo();
    for run in ["../../etc", "0123456789ab"] {
        gargoyle_in(dir.path())
            .args(["report", run])
            .assert()
            .code(2);
    }
}

#[test]
fn guide_prints_the_review_workflow() {
    let stdout = gargoyle()
        .arg("guide")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let stdout = String::from_utf8(stdout).unwrap();
    for step in [
        "gargoyle prepare --json",
        "gargoyle ingest",
        "gargoyle report",
    ] {
        assert!(stdout.contains(step), "missing {step}:\n{stdout}");
    }
}

#[test]
fn init_without_node_still_sets_up_the_repo() {
    let dir = tempfile::tempdir().unwrap();
    let stderr = gargoyle_in(dir.path())
        .arg("init")
        .env("PATH", dir.path())
        .assert()
        .success()
        .get_output()
        .stderr
        .clone();
    let stderr = String::from_utf8(stderr).unwrap();
    assert!(stderr.contains("no Node.js found"), "{stderr}");
    assert!(stderr.contains("run `gargoyle guide`"), "{stderr}");
    assert!(dir.path().join("gargoyle.toml").is_file());
}
