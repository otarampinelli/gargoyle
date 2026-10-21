use assert_cmd::Command;

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
