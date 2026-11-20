//! The CLI shortcuts forward the selected Desktop view and its arguments.
use super::*;

fn parse_desktop(tail: &[&str]) -> Result<Parsed, ParseError> {
    let args = std::iter::once("artifactize")
        .chain(tail.iter().copied())
        .map(Into::into)
        .collect::<Vec<_>>();
    parse_args(&args)
}

#[test]
fn monitor_forwards_the_initial_scope_and_native_paths() {
    for (tail, expected) in [
        (vec!["monitor"], vec!["monitor"]),
        (vec!["monitor", "--all"], vec!["monitor"]),
        (vec!["monitor", "."], vec!["monitor", "--repo=."]),
        (
            vec!["monitor", "work tree"],
            vec!["monitor", "--repo=work tree"],
        ),
        (
            vec!["--repo", "work tree", "monitor"],
            vec!["monitor", "--repo=work tree"],
        ),
    ] {
        let Parsed {
            state_dir,
            invocation: Invocation::Desktop(launch),
            ..
        } = parse_desktop(&tail).unwrap()
        else {
            panic!("expected a Monitor launch");
        };
        assert_eq!(launch.args(state_dir), expected);
    }
    for tail in [
        ["monitor", ".", "--all"].as_slice(),
        &["monitor", ".", "--repo", "."],
        &["monitor", "--all", "--repo", "."],
    ] {
        assert!(parse_desktop(tail).is_err());
    }
}

#[test]
fn review_forwards_the_request_reviewer_and_state_to_desktop() {
    let Parsed {
        state_dir,
        invocation: Invocation::Desktop(launch),
        ..
    } = parse_desktop(&[
        "--state-dir",
        "state directory",
        "review",
        "request-1",
        "--repo=.",
        "--reviewer",
        "alice",
    ])
    .unwrap()
    else {
        panic!("expected a Review launch");
    };
    assert_eq!(
        launch.args(state_dir),
        [
            "review",
            "request-1",
            "--reviewer=alice",
            "--repo=.",
            "--state-dir=state directory"
        ],
    );
}

#[test]
fn review_parses_one_scope_and_rejects_conflicts_with_either_repo_syntax() {
    use interactive::{Launch, Scope, Screen};

    let parse = |tail: &[&str]| parse_desktop(tail).map(|parsed| parsed.invocation);
    for args in [&["review"][..], &["review", "--all"]] {
        assert!(matches!(
            parse(args).unwrap(),
            Invocation::Desktop(Launch {
                scope: Scope::All,
                screen: Screen::Review { .. },
            })
        ));
    }
    for args in [
        &["--repo", "selected", "review"][..],
        &["review", "--repo=selected"],
    ] {
        assert!(
            matches!(parse(args).unwrap(), Invocation::Desktop(Launch { scope: Scope::Repository(path), screen: Screen::Review { .. } })
            if path == std::path::Path::new("selected"))
        );
    }
    for args in [
        &["review", "--all", "--repo", "selected"][..],
        &["--repo=selected", "review", "--all", "--json"],
    ] {
        assert!(
            matches!(parse(args), Err(ParseError::InvalidOptions { message, json })
            if message == "review accepts --repo or --all, not both." && json == args.contains(&"--json"))
        );
    }
}
