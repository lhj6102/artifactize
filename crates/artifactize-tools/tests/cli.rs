use std::process::Command;

#[test]
fn version_matches_the_shared_workspace_version() {
    let output = Command::new(env!("CARGO_BIN_EXE_artifactize-tools"))
        .arg("--version")
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        format!("artifactize-tools {}\n", env!("CARGO_PKG_VERSION"))
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn help_is_available_without_tool_subcommands() {
    let output = Command::new(env!("CARGO_BIN_EXE_artifactize-tools"))
        .arg("--help")
        .output()
        .unwrap();
    assert!(output.status.success());
    let help = String::from_utf8(output.stdout).unwrap();
    assert!(help.contains("--version"));
    assert!(help.contains("--help"));
    assert!(!help.contains("Commands:"));
}
