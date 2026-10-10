use std::{
    collections::BTreeMap,
    ffi::OsString,
    io,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU32, Ordering},
    },
    time::Duration,
};

use artifactize::{
    process,
    runtime::{self, Command, Error, Outcome, Verdict},
};
use support::os::{bin, private_dir, symlink_dir};
use tokio::{sync::oneshot, time::timeout};
use tokio_util::sync::CancellationToken;

mod support;

const TEST_TIMEOUT: Duration = Duration::from_secs(5);

struct Scratch(tempfile::TempDir);

impl Scratch {
    fn new() -> Self {
        let scratch = Self(tempfile::tempdir().unwrap());
        std::fs::create_dir(scratch.workspace()).unwrap();
        scratch
    }

    fn workspace(&self) -> PathBuf {
        self.0.path().join("workspace")
    }

    fn run_dir(&self) -> PathBuf {
        self.0.path().join("runs")
    }

    fn command(&self, program: &str, args: &[&str], timeout_ms: Option<u32>) -> Command {
        Command::prepare(
            bin(program).into(),
            args.iter().map(OsString::from).collect(),
            &self.workspace(),
            &self.run_dir(),
            timeout_ms.map(|ms| Duration::from_millis(ms.into())),
        )
        .unwrap()
    }
}

async fn execute(command: Command) -> Outcome {
    timeout(
        TEST_TIMEOUT + Duration::from_secs(2),
        runtime::execute(command, CancellationToken::new(), |_| async { Ok(()) }),
    )
    .await
    .expect("runner must settle")
}

fn completed(outcome: Outcome) -> runtime::ReviewResult {
    match outcome {
        Outcome::Completed(result) => result,
        other => panic!("expected a verdict, got {other:?}"),
    }
}

#[tokio::test]
async fn ordinary_zero_exit_is_green_with_actual_output() {
    let scratch = Scratch::new();
    let result = completed(
        execute(scratch.command("/bin/sh", &["-c", "printf out; printf err >&2"], None)).await,
    );
    assert_eq!(result.verdict, Verdict::Green);
    assert_eq!(result.exit_code, 0);
    assert_eq!(result.output.stdout, b"out");
    assert_eq!(result.output.stderr, b"err");
    assert!(!result.output.truncated);
    assert!(result.output.duration > Duration::ZERO);
}

#[tokio::test]
async fn ordinary_nonzero_exit_is_red() {
    let scratch = Scratch::new();
    let result = completed(execute(scratch.command("/bin/sh", &["-c", "exit 127"], None)).await);
    assert_eq!(result.verdict, Verdict::Red);
    assert_eq!(result.exit_code, 127);
}

#[tokio::test]
async fn missing_binary_is_an_operational_error_not_red() {
    let scratch = Scratch::new();
    assert!(matches!(
        execute(scratch.command("/artifactize/nonexistent-binary", &[], None)).await,
        Outcome::OperationalError(Error::Process(process::Error::Spawn(error)))
            if error.kind() == io::ErrorKind::NotFound
    ));
}

// Only Unix ends a process with a signal; Windows reports exit codes.
#[cfg(unix)]
#[tokio::test]
async fn signal_is_an_operational_error() {
    let scratch = Scratch::new();
    assert!(matches!(
        execute(scratch.command("/bin/sh", &["-c", "kill -TERM $$"], None)).await,
        Outcome::OperationalError(Error::AbnormalExit {
            signal: Some(support::os::SIGTERM),
            ..
        })
    ));
}

/// Windows ends every process with an exit code, so a status that would mean a crash or
/// Ctrl-C elsewhere is still an ordinary RED verdict, never a signal.
#[cfg(windows)]
#[tokio::test]
async fn windows_exit_statuses_are_verdicts_not_signals() {
    let scratch = Scratch::new();
    // STATUS_CONTROL_C_EXIT, as a process ended by Ctrl-C reports it.
    let result =
        completed(execute(scratch.command("/bin/sh", &["-c", "exit -1073741510"], None)).await);
    assert_eq!(result.verdict, Verdict::Red);
    assert_eq!(result.exit_code, -1073741510);
}

#[tokio::test]
async fn timeout_is_an_operational_error_and_reaps_the_leader() {
    let scratch = Scratch::new();
    // The deadline also covers spawn and registration; leave room for both under load.
    let command = scratch.command("/bin/sleep", &["30"], Some(1000));
    let pid = Arc::new(AtomicU32::new(0));
    let observed = pid.clone();
    let outcome = runtime::execute(command, CancellationToken::new(), move |child| async move {
        observed.store(child.pid, Ordering::SeqCst);
        Ok(())
    })
    .await;
    assert!(matches!(
        outcome,
        Outcome::OperationalError(Error::Process(process::Error::Timeout))
    ));
    assert_gone(pid.load(Ordering::SeqCst)).await;
}

#[tokio::test]
async fn cancellation_is_an_operational_error_and_cleans_up() {
    let scratch = Scratch::new();
    let cancellation = CancellationToken::new();
    let (registered, child) = oneshot::channel();
    // exec keeps the observed PID stable while the final long-lived command runs.
    let script = if cfg!(windows) {
        "touch started; sleep 30"
    } else {
        "touch started; exec sleep 30"
    };
    let command = scratch.command("/bin/sh", &["-c", script], None);
    let running = tokio::spawn(runtime::execute(
        command,
        cancellation.clone(),
        |child| async move {
            registered.send(child).unwrap();
            Ok(())
        },
    ));
    let child = child.await.unwrap();
    wait_for(|| scratch.workspace().join("started").exists()).await;
    cancellation.cancel();
    let outcome = timeout(TEST_TIMEOUT, running).await.unwrap().unwrap();
    assert!(
        matches!(
            outcome,
            Outcome::OperationalError(Error::Process(process::Error::Cancelled))
        ),
        "{outcome:?}"
    );
    assert_gone(child.pid).await;
}

#[tokio::test]
async fn pre_cancelled_commands_never_register() {
    let scratch = Scratch::new();
    let cancellation = CancellationToken::new();
    cancellation.cancel();
    let outcome = runtime::execute(
        scratch.command("/bin/true", &[], None),
        cancellation,
        |_| async { panic!("pre-cancelled commands must not spawn") },
    )
    .await;
    assert!(matches!(
        outcome,
        Outcome::OperationalError(Error::Process(process::Error::Cancelled))
    ));
}

#[tokio::test]
async fn literal_argv_is_preserved() {
    let scratch = Scratch::new();
    let result = completed(
        execute(scratch.command(
            "/usr/bin/printf",
            &["%s\n", "$(id); $HOME * a b", "{artifact}/path"],
            None,
        ))
        .await,
    );
    assert_eq!(
        result.output.stdout,
        b"$(id); $HOME * a b\n{artifact}/path\n"
    );
}

#[test]
fn runtime_timeout_defaults_and_bounds_match_current_source() {
    let scratch = Scratch::new();
    assert_eq!(
        scratch.command("/bin/true", &[], None).timeout(),
        Duration::from_secs(30)
    );
    for ms in [1, 2_147_483_647] {
        assert_eq!(
            scratch.command("/bin/true", &[], Some(ms)).timeout(),
            Duration::from_millis(ms.into())
        );
    }
    for ms in [0, 2_147_483_648, u32::MAX] {
        let run_dir = scratch.0.path().join("invalid-timeout");
        assert!(matches!(
            Command::prepare(
                "/bin/true".into(),
                vec![],
                &scratch.workspace(),
                &run_dir,
                Some(Duration::from_millis(ms.into()))
            ),
            Err(Error::InvalidTimeout)
        ));
        assert!(!run_dir.exists());
    }
}

#[tokio::test]
async fn runtime_has_independent_private_external_directories() {
    let scratch = Scratch::new();
    let command = scratch.command("/usr/bin/env", &[], None);
    let directory = command.directory().to_owned();
    let other = scratch.command("/bin/true", &[], None);
    assert_ne!(directory, other.directory());
    let result = completed(execute(command).await);
    let text = String::from_utf8(result.output.stdout).unwrap();
    let environment: BTreeMap<_, _> = text
        .lines()
        .map(|line| line.split_once('=').unwrap())
        .collect();
    // Windows children also get their profile folders and the system variables.
    assert_eq!(environment.len(), if cfg!(windows) { 16 } else { 10 });
    assert_eq!(
        environment["ARTIFACTIZE_WORKSPACE_DIR"],
        support::os::canonical(&scratch.workspace())
            .to_str()
            .unwrap()
    );
    for (variable, name) in [
        ("ARTIFACTIZE_OUTPUT_DIR", "output"),
        ("ARTIFACTIZE_TMP_DIR", "tmp"),
        ("HOME", "home"),
        ("XDG_CACHE_HOME", "cache"),
        ("TMPDIR", "tmp"),
        ("TMP", "tmp"),
        ("TEMP", "tmp"),
    ] {
        let path = Path::new(environment[variable]);
        assert_eq!(path, directory.join(name));
        assert!(!path.starts_with(scratch.workspace()));
        assert!(path.is_dir());
        assert!(private_dir(path));
    }
    for path in [&directory, &scratch.run_dir()] {
        assert!(private_dir(path));
    }
    assert!(
        directory.exists(),
        "caller-owned runtime output persists after exit"
    );
}

#[test]
fn output_inside_workspace_is_rejected_before_creation_even_through_symlinks() {
    let scratch = Scratch::new();
    let alias = scratch.0.path().join("alias");
    let workspace_alias = scratch.0.path().join("workspace-alias");
    support::os::link_dir(&scratch.workspace(), &alias);
    support::os::link_dir(&scratch.workspace(), &workspace_alias);
    // A symbolic link, where creating one is allowed, is refused like the links above.
    let symlink = scratch.0.path().join("symlink");
    if symlink_dir(scratch.workspace(), &symlink).is_some() {
        let result = Command::prepare(
            bin("/bin/true").into(),
            vec![],
            &workspace_alias,
            &symlink.join("new/nested"),
            None,
        );
        assert!(
            matches!(result, Err(Error::OutputInsideWorkspace)),
            "{result:?}"
        );
    }
    for output in [
        scratch.workspace(),
        scratch.workspace().join("new/nested"),
        alias.join("new/nested"),
        scratch.0.path().join("missing/../workspace/new"),
    ] {
        let result = Command::prepare(
            bin("/bin/true").into(),
            vec![],
            &workspace_alias,
            &output,
            None,
        );
        assert!(
            matches!(result, Err(Error::OutputInsideWorkspace)),
            "{output:?}: {result:?}"
        );
    }
    assert!(!scratch.workspace().join("new").exists());
    assert!(!scratch.0.path().join("missing").exists());
}

#[tokio::test]
async fn external_symlinked_output_uses_canonical_existing_ancestors() {
    let scratch = Scratch::new();
    let external = scratch.0.path().join("external");
    std::fs::create_dir(&external).unwrap();
    let alias = scratch.0.path().join("alias");
    support::os::link_dir(&external, &alias);
    let command = Command::prepare(
        bin("/usr/bin/env").into(),
        vec![],
        &scratch.workspace(),
        &alias.join("new/nested"),
        None,
    )
    .unwrap();
    assert!(
        command
            .directory()
            .starts_with(support::os::canonical(&external.join("new/nested")))
    );
    assert_eq!(completed(execute(command).await).verdict, Verdict::Green);
}

#[test]
fn parent_secret_is_not_visible_to_runtime_child() {
    // The probe's PATH has no compiler, so build the Windows stand-ins first.
    bin("/usr/bin/env");
    // Set the parent environment in another process, never mutate this test runner's env.
    let result = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "environment_subprocess_probe", "--nocapture"])
        .env("ARTIFACTIZE_ENV_PROBE", "enabled")
        .env("PROVIDER_SECRET", "not-for-child")
        .env("NODE_OPTIONS", "--require=not-for-child")
        .env("PYTHONPATH", "/not-for-child")
        .env("PATH", "/usr/bin:/bin")
        .env("LANG", "C")
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
}

#[tokio::test]
async fn environment_subprocess_probe() {
    if std::env::var_os("ARTIFACTIZE_ENV_PROBE").is_none() {
        return;
    }
    assert_eq!(std::env::var("PROVIDER_SECRET").unwrap(), "not-for-child");
    let scratch = Scratch::new();
    let result = completed(execute(scratch.command("/usr/bin/env", &[], None)).await);
    let output = String::from_utf8(result.output.stdout).unwrap();
    assert!(output.lines().any(|line| line == "PATH=/usr/bin:/bin"));
    assert!(output.lines().any(|line| line == "LANG=C"));
    for forbidden in [
        "PROVIDER_SECRET",
        "NODE_OPTIONS",
        "PYTHONPATH",
        "ARTIFACTIZE_ENV_PROBE",
        "not-for-child",
        "NODE_NO_WARNINGS",
    ] {
        assert!(!output.contains(forbidden));
    }
}

#[tokio::test]
async fn runtime_cleans_ansi_and_controls_without_changing_whitespace_or_del() {
    let scratch = Scratch::new();
    let result = completed(
        execute(scratch.command(
            "/usr/bin/printf",
            &["\\033[31mred\\033[0m\\000\\001\\010\\013\\014\\016\\037\\t\\n\\r\\177\\377\\033[?25l\\033[1 qend"],
            None,
        ))
        .await,
    );
    assert_eq!(result.output.stdout, "red\t\n\r\x7f\u{fffd}end".as_bytes());
    assert!(!result.output.truncated);
    let stderr = completed(
        execute(scratch.command(
            "/bin/sh",
            &["-c", "printf '\\033[31merror\\033[0m\\001' >&2"],
            None,
        ))
        .await,
    );
    assert_eq!(stderr.output.stderr, b"error");
}

#[tokio::test]
async fn both_streams_are_bounded_before_cleaning_and_keep_truncation_metadata() {
    let scratch = Scratch::new();
    let result = completed(
        execute(scratch.command(
            "/bin/sh",
            &[
                "-c",
                "head -c 262144 /dev/zero | tr '\\000' x & head -c 262144 /dev/zero | tr '\\000' y >&2 & wait",
            ],
            None,
        ))
        .await,
    );
    assert_eq!(result.verdict, Verdict::Green);
    assert_eq!(result.output.stdout, vec![b'x'; 128 * 1024]);
    assert_eq!(result.output.stderr, vec![b'y'; 128 * 1024]);
    assert!(result.output.truncated);
    let result = completed(
        execute(scratch.command(
            "/bin/sh",
            &["-c", "head -c 131072 /dev/zero; printf discarded"],
            None,
        ))
        .await,
    );
    assert!(result.output.stdout.is_empty());
    assert!(result.output.truncated);
}

#[tokio::test]
async fn timeout_kills_a_grandchild_even_when_the_leader_ignores_term() {
    let scratch = Scratch::new();
    // Each Windows process start costs more, and the deadline covers three of them.
    let command = scratch.command(
        "/bin/sh",
        &[
            "-c",
            "trap '' TERM; sh -c 'sleep 30 & echo $! > \"$ARTIFACTIZE_OUTPUT_DIR/grandchild\"; wait' & wait",
        ],
        Some(if cfg!(windows) { 2000 } else { 500 }),
    );
    let marker = command.directory().join("output/grandchild");
    let outcome = execute(command).await;
    assert!(matches!(
        outcome,
        Outcome::OperationalError(Error::Process(process::Error::Timeout))
    ));
    let pid = std::fs::read_to_string(marker)
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    assert_gone(pid).await;
}

#[tokio::test]
async fn normal_exit_also_kills_a_background_descendant() {
    let scratch = Scratch::new();
    let result =
        completed(execute(scratch.command("/bin/sh", &["-c", "sleep 30 & echo $!"], None)).await);
    assert_eq!(result.verdict, Verdict::Green);
    let pid = String::from_utf8(result.output.stdout)
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    assert_gone(pid).await;
}

#[tokio::test]
async fn dropping_an_active_caller_cleans_its_grandchild() {
    let scratch = Scratch::new();
    let command = scratch.command(
        "/bin/sh",
        &[
            "-c",
            "trap '' TERM; sh -c 'sleep 30 & echo $! > \"$ARTIFACTIZE_OUTPUT_DIR/grandchild\"; wait' & wait",
        ],
        None,
    );
    let marker = command.directory().join("output/grandchild");
    let (registered, child) = oneshot::channel();
    let running = tokio::spawn(runtime::execute(
        command,
        CancellationToken::new(),
        |child| async move {
            registered.send(child).unwrap();
            Ok(())
        },
    ));
    let child = child.await.unwrap();
    wait_for(|| {
        std::fs::read_to_string(&marker).is_ok_and(|text| text.trim().parse::<u32>().is_ok())
    })
    .await;
    let pid = std::fs::read_to_string(marker)
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    running.abort();
    assert!(running.await.unwrap_err().is_cancelled());
    assert_gone(child.pid).await;
    assert_gone(pid).await;
}

#[tokio::test]
async fn deadline_is_not_reset_after_registration() {
    let scratch = Scratch::new();
    let command = scratch.command(
        "/bin/sh",
        &[
            "-c",
            "sleep 0.6; touch \"$ARTIFACTIZE_OUTPUT_DIR/too-late\"",
        ],
        Some(1000),
    );
    let marker = command.directory().join("output/too-late");
    let outcome = runtime::execute(command, CancellationToken::new(), |_| async {
        tokio::time::sleep(Duration::from_millis(600)).await;
        Ok(())
    })
    .await;
    assert!(
        matches!(
            outcome,
            Outcome::OperationalError(Error::Process(process::Error::Timeout))
        ),
        "{outcome:?}"
    );
    assert!(!marker.exists());
}

async fn assert_gone(pid: u32) {
    assert!(pid > 0, "must have observed a real process");
    wait_for(|| support::os::gone(pid)).await;
}

async fn wait_for(condition: impl Fn() -> bool) {
    timeout(TEST_TIMEOUT, async {
        while !condition() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("process did not reach the expected state");
}
