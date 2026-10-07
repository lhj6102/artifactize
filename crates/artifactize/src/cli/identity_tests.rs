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
