//! A5: CLI launch selection is independent of cwd and never narrows the available catalog.
use super::*;

#[test]
fn a5_monitor_starts_on_all_from_any_cwd_and_paths_select_only_initial_scope() {
    const PROBE: &str = "ARTIFACTIZE_MONITOR_PROBE";
    if let Some(input) = crate::platform::environment::var(PROBE) {
        let args: Vec<String> = serde_json::from_str(&input.to_string_lossy()).unwrap();
        let expected = args
            .iter()
            .position(|arg| arg == "--repo")
            .map(|index| PathBuf::from(&args[index + 1]))
            .or_else(|| {
                args.last()
                    .filter(|arg| *arg == "." || arg.contains("selected"))
                    .map(PathBuf::from)
            });
        let screens = Screens {
            monitor: |_, initial, _| {
                Box::pin(async move {
                    let expected = crate::platform::environment::var("ARTIFACTIZE_MONITOR_EXPECT")
                        .filter(|value| !value.is_empty())
                        .map(PathBuf::from);
                    assert_eq!(initial, expected);
                    Ok(())
                })
            },
            review: |_, _, _, _, _| unreachable!("monitor never routes to review"),
        };
        assert_eq!(
            expected,
            crate::platform::environment::var("ARTIFACTIZE_MONITOR_EXPECT")
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
        );
        let cli = Cli::try_parse_from(args).unwrap();
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
            vec!["monitor".to_owned(), crate::platform::path_text(&selected)],
            Some(selected.clone()),
        ),
        (
            &unrelated,
            vec![
                "--repo".to_owned(),
                crate::platform::path_text(&selected),
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
            crate::platform::path_text(&root.path().join("state")),
        ]
        .into_iter()
        .chain(tail)
        .collect::<Vec<_>>();
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "cli::monitor_tests::a5_monitor_starts_on_all_from_any_cwd_and_paths_select_only_initial_scope", "--nocapture"])
            .current_dir(cwd).env(PROBE, serde_json::to_string(&args).unwrap())
            .env("ARTIFACTIZE_MONITOR_EXPECT", expected.as_deref().map(crate::platform::path_text).unwrap_or_default())
            .output().unwrap();
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
        assert!(Cli::try_parse_from(std::iter::once("artifactize").chain(args)).is_err());
    }
}
