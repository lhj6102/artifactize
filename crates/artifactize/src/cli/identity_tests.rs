//! Clap parses identities at the public boundary; existing segment and mirror types stay unchanged.
use super::*;

#[test]
fn all_run_request_and_reuse_key_arguments_retain_valid_wire_strings() {
    let id = "run-legacy_1.request";
    let key = "a".repeat(64);
    let commands = [
        vec!["run", "show", id],
        vec!["request", "list", "--run", id],
        vec!["request", "show", id],
        vec!["request", "claim", id],
        vec!["request", "unclaim", id],
        vec!["request", "tool", id, "inspect"],
        vec!["request", "submit", id, "--verdict", "GREEN"],
        vec!["review", id],
        vec!["cache", "show", key.as_str()],
        vec!["cache", "rm", key.as_str()],
        vec!["server", "rm", key.as_str()],
    ];
    for command in commands {
        Cli::try_parse_from(std::iter::once("artifactize").chain(command)).unwrap();
    }
    let Some(Command::Run {
        command: RunCommand::Show { run_id, .. },
    }) = Cli::try_parse_from(["artifactize", "run", "show", id])
        .unwrap()
        .command
    else {
        panic!("Run show");
    };
    assert_eq!(run_id.as_str(), id);
    let Some(Command::Cache {
        command: CacheCommand::Show { key: parsed, .. },
    }) = Cli::try_parse_from(["artifactize", "cache", "show", key.as_str()])
        .unwrap()
        .command
    else {
        panic!("Cache show");
    };
    assert_eq!(parsed.as_str(), key);
    let legacy = "remote-".to_owned() + &"x".repeat(200);
    assert!(legacy.parse::<crate::types::ExecutionId>().is_ok());
    // The mirror allowance belongs only to ExecutionId, not RunId/RequestId.
    assert!(Cli::try_parse_from(["artifactize", "request", "show", legacy.as_str()]).is_err());
}

#[test]
fn malformed_identity_arguments_are_clap_usage_errors() {
    for id in [
        "",
        "..",
        "../escape",
        "a/b",
        "spaces here",
        "한글",
        &"x".repeat(201),
    ] {
        for command in [
            vec!["run", "show", id],
            vec!["request", "list", "--run", id],
            vec!["request", "show", id],
            vec!["request", "claim", id],
            vec!["request", "unclaim", id],
            vec!["request", "tool", id, "inspect"],
            vec!["request", "submit", id, "--verdict", "GREEN"],
            vec!["review", id],
        ] {
            let error =
                Cli::try_parse_from(std::iter::once("artifactize").chain(command)).unwrap_err();
            assert_eq!(error.kind(), ErrorKind::ValueValidation);
        }
    }
    for key in [
        "missing",
        &"a".repeat(63),
        &"A".repeat(64),
        &"g".repeat(64),
        &"0".repeat(65),
    ] {
        for command in [
            ["cache", "show", key],
            ["cache", "rm", key],
            ["server", "rm", key],
        ] {
            assert_eq!(
                Cli::try_parse_from(std::iter::once("artifactize").chain(command))
                    .unwrap_err()
                    .kind(),
                ErrorKind::ValueValidation
            );
        }
    }
}

#[test]
fn selection_duration_and_verdict_are_parsed_at_the_command_boundary() {
    for args in [
        vec!["verify", "app/check"],
        vec!["verify", "--evals", "app/check,lib/test"],
        vec!["verify", "--artifacts", "app,lib"],
        vec!["config", "graph", "app"],
    ] {
        Cli::try_parse_from(std::iter::once("artifactize").chain(args)).unwrap();
    }
    for args in [
        vec!["verify", "../escape"],
        vec!["verify", "--eval", "app"],
        vec!["verify", "--evals", "app/check,invalid"],
        vec!["verify", "--artifacts", "app,invalid/name"],
        vec!["config", "graph", "app/check"],
        vec!["verify", "--all", "--timeout-ms", "0"],
        vec!["verify", "--all", "--timeout-ms", "2147483648"],
        vec!["request", "submit", "r1", "--verdict", "green"],
    ] {
        assert!(Cli::try_parse_from(std::iter::once("artifactize").chain(args)).is_err());
    }
    let cli = Cli::try_parse_from([
        "artifactize",
        "verify",
        "--all",
        "--timeout-ms",
        "2147483647",
    ])
    .unwrap();
    let Some(Command::Verify { timeout_ms, .. }) = cli.command else {
        panic!("verify")
    };
    assert_eq!(
        timeout_ms,
        Some(std::time::Duration::from_millis(2_147_483_647))
    );
    let cli = Cli::try_parse_from([
        "artifactize",
        "request",
        "submit",
        "r1",
        "--verdict",
        "GREEN",
    ])
    .unwrap();
    let Some(Command::Request {
        command: RequestCommand::Submit { verdict, .. },
    }) = cli.command
    else {
        panic!("submit")
    };
    assert_eq!(verdict, crate::runtime::Verdict::Green);
}

#[test]
fn selection_files_validate_ids_before_entering_the_project() {
    let root = crate::test_os::tempdir();
    let path = root.path().join("selection.json");
    std::fs::write(&path, r#"["app/check","../escape"]"#).unwrap();
    assert!(selection_file::<crate::types::EvalId>(&path).is_err());
    std::fs::write(&path, r#"["app","lib"]"#).unwrap();
    assert_eq!(
        selection_file::<crate::types::ArtifactName>(&path).unwrap(),
        ["app", "lib"]
    );
}
