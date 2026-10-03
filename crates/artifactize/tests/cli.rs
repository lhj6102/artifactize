use std::process::Command;

#[test]
fn version_matches_package() {
    let output = Command::new(env!("CARGO_BIN_EXE_artifactize"))
        .arg("--version")
        .output()
        .unwrap();

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        format!("artifactize {}\n", env!("CARGO_PKG_VERSION"))
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn help_is_displayed_without_arguments() {
    let output = Command::new(env!("CARGO_BIN_EXE_artifactize"))
        .output()
        .unwrap();
    let help = Command::new(env!("CARGO_BIN_EXE_artifactize"))
        .arg("--help")
        .output()
        .unwrap();

    assert!(output.status.success());
    assert!(help.status.success());
    assert!(output.stderr.is_empty());
    assert!(help.stderr.is_empty());
    assert_eq!(output.stdout, help.stdout);
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("Usage: artifactize"));
    assert!(text.contains("--repo <PATH>"));
    assert!(text.contains("--state-dir <PATH>"));
    assert!(text.contains("--json"));
}
