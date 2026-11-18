//! A5: CLI launch selection is independent of cwd and never narrows the available catalog.
use super::*;

#[test]
fn a5_monitor_starts_on_all_from_any_cwd_and_paths_select_only_initial_scope() {
    const PROBE: &str = "ARTIFACTIZE_MONITOR_PROBE";
    if let Some(input) = artifactize_core::platform::environment::var(PROBE) {
        let args: Vec<String> = serde_json::from_str(&input.to_string_lossy()).unwrap();
        let screens = Screens {
            monitor: |_, initial, _| {
                Box::pin(async move {
                    let expected =
                        artifactize_core::platform::environment::var("ARTIFACTIZE_MONITOR_EXPECT")
                            .filter(|value| !value.is_empty())
                            .map(PathBuf::from);
                    assert_eq!(initial, expected);
                    Ok(())
                })
            },
            review: |_, _, _, _, _| unreachable!("monitor never routes to review"),
        };
        let cli = parse_args(&args.into_iter().map(Into::into).collect::<Vec<_>>()).unwrap();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        assert_eq!(
            runtime.block_on(dispatch::execute(cli, screens)).unwrap(),
            0
        );
        return;
    }
    let root = crate::test_os::tempdir();
    let selected = root.path().join("selected");
    let unrelated = root.path().join("unrelated");
    std::fs::create_dir(&selected).unwrap();
    std::fs::create_dir(&unrelated).unwrap();
    for (cwd, tail, expected) in [
        (&selected, vec!["monitor".to_owned()], None),
        (&unrelated, vec!["monitor".to_owned()], None),
        (
            &selected,
            vec!["monitor".to_owned(), ".".to_owned()],
            Some(PathBuf::from(".")),
        ),
        (
            &unrelated,
            vec![
                "monitor".to_owned(),
                artifactize_core::platform::path_text(&selected),
            ],
            Some(selected.clone()),
        ),
        (
            &unrelated,
            vec![
                "--repo".to_owned(),
                artifactize_core::platform::path_text(&selected),
                "monitor".to_owned(),
            ],
            Some(selected.clone()),
        ),
        (
            &unrelated,
            vec!["monitor".to_owned(), "--all".to_owned()],
            None,
        ),
    ] {
        let args = vec![
            "artifactize".to_owned(),
            "--state-dir".to_owned(),
            artifactize_core::platform::path_text(&root.path().join("state")),
        ]
        .into_iter()
        .chain(tail)
        .collect::<Vec<_>>();
        let output = crate::test_os::run_test_in(
            "cli::monitor_tests::a5_monitor_starts_on_all_from_any_cwd_and_paths_select_only_initial_scope",
            cwd,
            &[
                (PROBE, serde_json::to_string(&args).unwrap().into()),
                (
                    "ARTIFACTIZE_MONITOR_EXPECT",
                    expected
                        .as_deref()
                        .map(artifactize_core::platform::path_text)
                        .unwrap_or_default()
                        .into(),
                ),
            ],
        );
        assert!(
            output.status.success(),
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    for args in [
        vec!["monitor", ".", "--repo", "."],
        vec!["monitor", ".", "--all"],
        vec!["monitor", "--all", "--repo", "."],
    ] {
        let args = std::iter::once("artifactize")
            .chain(args)
            .map(Into::into)
            .collect::<Vec<_>>();
        assert!(parse_args(&args).is_err());
    }
}

#[test]
fn review_parses_one_scope_and_rejects_conflicts_with_either_repo_syntax() {
    use interactive::Scope;

    let parse = |tail: &[&str]| {
        let args = std::iter::once("artifactize")
            .chain(tail.iter().copied())
            .map(Into::into)
            .collect::<Vec<_>>();
        parse_args(&args).map(|parsed| parsed.invocation)
    };
    for args in [&["review"][..], &["review", "--all"]] {
        assert!(matches!(
            parse(args).unwrap(),
            Invocation::Review {
                scope: Scope::All,
                ..
            }
        ));
    }
    for args in [
        &["--repo", "selected", "review"][..],
        &["review", "--repo=selected"],
    ] {
        assert!(
            matches!(parse(args).unwrap(), Invocation::Review { scope: Scope::Repository(path), .. }
            if path == std::path::Path::new("selected"))
        );
    }
    for args in [
        &["review", "--all", "--repo", "selected"][..],
        &["--repo=selected", "review", "--all", "--json"],
    ] {
        assert!(
            matches!(parse(args), Err(ParseError::ConflictingScope { message, json })
            if message == "review accepts --repo or --all, not both." && json == args.contains(&"--json"))
        );
    }
}
