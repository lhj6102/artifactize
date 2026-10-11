use std::{
    collections::BTreeMap,
    ffi::OsString,
    io,
    path::{Path, PathBuf},
    time::Duration,
};

use artifactize::{
    process,
    runtime::{self, Command, Error, Outcome, Verdict},
};
use support::os::{private_dir, symlink_dir};
use tokio::{sync::oneshot, time::timeout};
use tokio_util::sync::CancellationToken;

mod support;

const TEST_TIMEOUT: Duration = Duration::from_secs(5);

struct Scratch(tempfile::TempDir);

impl Scratch {
    fn new() -> Self {
        let scratch = Self(support::os::tempdir());
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
            program.into(),
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
        support::os::patience(TEST_TIMEOUT + Duration::from_secs(2)),
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
        execute(scratch.command(
            &support::os::shell(),
            &["-c", "printf out; printf err >&2"],
            None,
        ))
        .await,
    );
    assert_eq!(result.verdict, Verdict::Green);
    assert_eq!(result.exit_code, 0);
    assert_eq!(result.output.stdout, b"out");
    assert_eq!(result.output.stderr, b"err");
    assert!(!result.output.truncated);
}

#[tokio::test]
async fn ordinary_nonzero_exit_is_red() {
    let scratch = Scratch::new();
    let result =
        completed(execute(scratch.command(&support::os::shell(), &["-c", "exit 127"], None)).await);
    assert_eq!(result.verdict, Verdict::Red);
    assert_eq!(result.exit_code, 127);
}

#[tokio::test]
async fn missing_binary_is_an_operational_error_not_red() {
    let scratch = Scratch::new();
    assert!(matches!(
        execute(scratch.command(scratch.workspace().join("nonexistent-binary").to_str().unwrap(), &[], None)).await,
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
        execute(scratch.command(
            &support::os::shell(),
            &["-c", support::os::SELF_TERMINATE],
            None
        ))
        .await,
        Outcome::OperationalError(Error::AbnormalExit {
            signal: Some(support::os::SIGTERM),
            ..
        })
    ));
}

// Windows ends every process with an exit code, so a status that would mean a crash or
// Ctrl-C elsewhere is still an ordinary RED verdict, never a signal.
#[cfg(windows)]
#[tokio::test]
async fn windows_exit_statuses_are_verdicts_not_signals() {
    let scratch = Scratch::new();
    // STATUS_CONTROL_C_EXIT, as a process ended by Ctrl-C reports it.
    let result = completed(
        execute(scratch.command(&support::os::shell(), &["-c", "exit -1073741510"], None)).await,
    );
    assert_eq!(result.verdict, Verdict::Red);
    assert_eq!(result.exit_code, -1073741510);
}

// The held child keeps the paused clock still until registration has handed over its PID;
// the test then advances the clock past the deadline.
#[tokio::test(start_paused = true)]
async fn timeout_is_an_operational_error_and_reaps_the_leader() {
    let scratch = Scratch::new();
    let command = scratch.command(
        &support::os::shell(),
        &["-c", support::os::LINGERING],
        Some(1000),
    );
    let (registered, child) = oneshot::channel();
    let running = tokio::spawn(runtime::execute(
        command,
        CancellationToken::new(),
        |child| async move {
            registered.send(child.pid).unwrap();
            Ok(())
        },
    ));
    let pid = child.await.unwrap();
    tokio::time::advance(Duration::from_millis(1000)).await;
    let outcome = running.await.unwrap();
    assert!(
        matches!(
            outcome,
            Outcome::OperationalError(Error::Process(process::Error::Timeout))
        ),
        "{outcome:?}"
    );
    assert_gone(pid.get()).await;
}

#[tokio::test]
async fn cancellation_is_an_operational_error_and_cleans_up() {
    let scratch = Scratch::new();
    let cancellation = CancellationToken::new();
    let (registered, child) = oneshot::channel();
    // exec keeps the observed PID stable while the final long-lived command runs.
    let script = support::os::cancellation_script();
    let command = scratch.command(&support::os::shell(), &["-c", script], None);
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
    let outcome = timeout(support::os::patience(TEST_TIMEOUT), running)
        .await
        .unwrap()
        .unwrap();
    assert!(
        matches!(
            outcome,
            Outcome::OperationalError(Error::Process(process::Error::Cancelled))
        ),
        "{outcome:?}"
    );
    assert_gone(child.pid.get()).await;
}

#[tokio::test]
async fn pre_cancelled_commands_never_register() {
    let scratch = Scratch::new();
    let cancellation = CancellationToken::new();
    cancellation.cancel();
    let outcome = runtime::execute(
        scratch.command(&support::os::true_program(), &[], None),
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
            &support::os::printf_program(),
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
        scratch
            .command(&support::os::true_program(), &[], None)
            .timeout(),
        Duration::from_secs(30)
    );
    for ms in [1, 2_147_483_647] {
        assert_eq!(
            scratch
                .command(&support::os::true_program(), &[], Some(ms))
                .timeout(),
            Duration::from_millis(ms.into())
        );
    }
    for ms in [0, 2_147_483_648, u32::MAX] {
        let run_dir = scratch.0.path().join("invalid-timeout");
        assert!(matches!(
            Command::prepare(
                support::os::true_program().into(),
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
    let command = scratch.command(&support::os::env_program(), &[], None);
    let directory = command.directory().to_owned();
    let other = scratch.command(&support::os::true_program(), &[], None);
    assert_ne!(directory, other.directory());
    let result = completed(execute(command).await);
    let text = String::from_utf8(result.output.stdout).unwrap();
    let environment: BTreeMap<_, _> = text
        .lines()
        .map(|line| line.split_once('=').unwrap())
        .collect();
    // Windows children also get their profile folders and the system variables.
    assert_eq!(environment.len(), support::os::runtime_environment_len());
    assert_eq!(
        environment["ARTIFACTIZE_WORKSPACE_DIR"],
        support::os::canonical(&scratch.workspace())
            .to_str()
            .unwrap()
    );
    for (variable, name) in support::os::runtime_directories() {
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
            support::os::true_program().into(),
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
            support::os::true_program().into(),
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
        support::os::env_program().into(),
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
    support::os::env_program();
    // Set the parent environment in another process, never mutate this test runner's env.
    let result = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "environment_subprocess_probe", "--nocapture"])
        .env("ARTIFACTIZE_ENV_PROBE", "enabled")
        .env("PROVIDER_SECRET", "not-for-child")
        .env("NODE_OPTIONS", "--require=not-for-child")
        .env("PYTHONPATH", "/not-for-child")
        .env("PATH", support::os::system_path())
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
    let result = completed(execute(scratch.command(&support::os::env_program(), &[], None)).await);
    let output = String::from_utf8(result.output.stdout).unwrap();
    assert!(
        output
            .lines()
            .any(|line| line == format!("PATH={}", support::os::system_path().to_str().unwrap()))
    );
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
            &support::os::printf_program(),
            &["\\033[31mred\\033[0m\\000\\001\\010\\013\\014\\016\\037\\t\\n\\r\\177\\377\\033[?25l\\033[1 qend"],
            None,
        ))
        .await,
    );
    assert_eq!(result.output.stdout, "red\t\n\r\x7f\u{fffd}end".as_bytes());
    assert!(!result.output.truncated);
    let stderr = completed(
        execute(scratch.command(
            &support::os::shell(),
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
            &support::os::shell(),
            &["-c", &support::os::letter_output(262144, 262144)],
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
            &support::os::shell(),
            &["-c", &support::os::capped_output(131072)],
            None,
        ))
        .await,
    );
    assert!(result.output.stdout.is_empty());
    assert!(result.output.truncated);
}

// The paused clock reaches the deadline only when the test advances it, once the
// grandchild has started.
#[tokio::test(start_paused = true)]
async fn timeout_kills_a_grandchild_even_when_the_leader_ignores_term() {
    let scratch = Scratch::new();
    let command = scratch.command(
        &support::os::shell(),
        &["-c", support::os::LINGERING_GRANDCHILD],
        Some(1000),
    );
    let marker = command.directory().join("output/grandchild");
    let running = tokio::spawn(runtime::execute(
        command,
        CancellationToken::new(),
        |_| async { Ok(()) },
    ));
    let pid = grandchild(&marker, &running).await;
    tokio::time::advance(Duration::from_millis(1000)).await;
    let outcome = running.await.unwrap();
    assert!(
        matches!(
            outcome,
            Outcome::OperationalError(Error::Process(process::Error::Timeout))
        ),
        "{outcome:?}"
    );
    assert_gone(pid).await;
}

/// The PID the lingering grandchild wrote to `marker`, once it has. The test yields rather
/// than sleeps while it waits, so that a paused clock stands still.
async fn grandchild(marker: &Path, running: &tokio::task::JoinHandle<Outcome>) -> u32 {
    let failure_deadline = std::time::Instant::now() + support::os::patience(TEST_TIMEOUT);
    loop {
        let written = std::fs::read_to_string(marker).ok();
        if let Some(pid) = written
            .as_deref()
            .and_then(|text| text.strip_suffix('\n'))
            .and_then(|text| text.trim().parse().ok())
        {
            return pid;
        }
        assert!(
            !running.is_finished(),
            "execution ended before its grandchild started"
        );
        assert!(
            std::time::Instant::now() < failure_deadline,
            "the grandchild did not start"
        );
        tokio::task::yield_now().await;
    }
}

#[tokio::test]
async fn normal_exit_also_kills_a_background_descendant() {
    let scratch = Scratch::new();
    let result = completed(
        execute(scratch.command(
            &support::os::shell(),
            &["-c", &format!("{} & echo $!", support::os::LINGERING)],
            None,
        ))
        .await,
    );
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
        &support::os::shell(),
        &["-c", support::os::LINGERING_GRANDCHILD],
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
    assert_gone(child.pid.get()).await;
    assert_gone(pid).await;
}

#[tokio::test(start_paused = true)]
async fn deadline_is_not_reset_after_registration() {
    let scratch = Scratch::new();
    let command = scratch.command(
        &support::os::shell(),
        &[
            "-c",
            "touch started; while [ ! -e release ]; do sleep 0.01; done",
        ],
        Some(1000),
    );
    let (registered, child) = oneshot::channel();
    let (release, held) = oneshot::channel();
    let running = tokio::spawn(runtime::execute(
        command,
        CancellationToken::new(),
        |child| async move {
            registered.send(child).unwrap();
            held.await.unwrap();
            Ok(())
        },
    ));
    let child = child.await.unwrap();
    // Consume most of the one deadline while user code is still behind registration.
    tokio::time::advance(Duration::from_millis(600)).await;
    release.send(()).unwrap();
    while !scratch.workspace().join("started").exists() {
        assert!(
            !running.is_finished(),
            "registration failed before execution"
        );
        tokio::task::yield_now().await;
    }
    // Only 400 ms remains. A reset at registration would leave this process running.
    tokio::time::advance(Duration::from_millis(401)).await;
    tokio::task::yield_now().await;
    let failure_deadline = std::time::Instant::now() + support::os::patience(TEST_TIMEOUT);
    while !running.is_finished() {
        assert!(
            std::time::Instant::now() < failure_deadline,
            "execution reset the registration deadline"
        );
        tokio::task::yield_now().await;
    }
    assert!(
        running.is_finished(),
        "execution reset the registration deadline"
    );
    let outcome = running.await.unwrap();
    assert!(
        matches!(
            outcome,
            Outcome::OperationalError(Error::Process(process::Error::Timeout))
        ),
        "{outcome:?}"
    );
    assert_gone(child.pid.get()).await;
}

async fn assert_gone(pid: u32) {
    assert!(pid > 0, "must have observed a real process");
    wait_for(|| support::os::gone(pid)).await;
}

async fn wait_for(condition: impl Fn() -> bool) {
    timeout(support::os::patience(TEST_TIMEOUT), async {
        while !condition() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("process did not reach the expected state");
}
