use std::{
    collections::BTreeMap,
    ffi::OsString,
    io,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicU32, Ordering},
    },
    time::Duration,
};

use artifactize::{
    process::{self, ChildIdentity, Command},
    runtime::{self, Error, Outcome, Verdict},
};
use tokio::{sync::oneshot, time::timeout};
use tokio_util::sync::CancellationToken;

const TEST_TIMEOUT: Duration = Duration::from_secs(5);

fn command(program: &str, args: &[&str]) -> Command {
    Command {
        program: program.into(),
        args: args.iter().map(OsString::from).collect(),
        cwd: PathBuf::from("/"),
        env: BTreeMap::from([("PATH".into(), "/usr/bin:/bin".into())]),
        timeout: TEST_TIMEOUT,
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
    let result =
        completed(execute(command("/bin/sh", &["-c", "printf out; printf err >&2"])).await);
    assert_eq!(result.verdict, Verdict::Green);
    assert_eq!(result.exit_code, 0);
    assert_eq!(result.output.stdout, b"out");
    assert_eq!(result.output.stderr, b"err");
    assert!(!result.output.truncated);
    assert!(result.output.duration > Duration::ZERO);
}

#[tokio::test]
async fn ordinary_nonzero_exit_is_red() {
    let result = completed(execute(command("/bin/sh", &["-c", "exit 127"])).await);
    assert_eq!(result.verdict, Verdict::Red);
    assert_eq!(result.exit_code, 127);
}

#[tokio::test]
async fn missing_binary_is_an_operational_error_not_red() {
    assert!(matches!(
        execute(command("/artifactize/nonexistent-binary", &[])).await,
        Outcome::OperationalError(Error::Process(process::Error::Spawn(error)))
            if error.kind() == io::ErrorKind::NotFound
    ));
}

#[tokio::test]
async fn signal_is_an_operational_error() {
    assert!(matches!(
        execute(command("/bin/sh", &["-c", "kill -TERM $$"])).await,
        Outcome::OperationalError(Error::AbnormalExit {
            signal: Some(libc::SIGTERM),
            ..
        })
    ));
}

#[tokio::test]
async fn timeout_is_an_operational_error_and_reaps_the_leader() {
    let mut command = command("/bin/sleep", &["30"]);
    command.timeout = Duration::from_millis(100);
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
    let cancellation = CancellationToken::new();
    let (registered, child) = oneshot::channel();
    let running = tokio::spawn(runtime::execute(
        command("/bin/sleep", &["30"]),
        cancellation.clone(),
        |identity| async move {
            registered.send(identity).unwrap();
            Ok(())
        },
    ));
    let identity = child.await.unwrap();
    // Wait until the command has exec'd; cancellation must also stop active work.
    wait_for(|| {
        std::fs::read_to_string(format!("/proc/{}/comm", identity.pid))
            .is_ok_and(|comm| comm.trim() == "sleep")
    })
    .await;
    cancellation.cancel();
    assert!(matches!(
        timeout(TEST_TIMEOUT, running).await.unwrap().unwrap(),
        Outcome::OperationalError(Error::Process(process::Error::Cancelled))
    ));
    assert_gone(identity.pid).await;
}

#[tokio::test]
async fn pre_cancelled_commands_never_register() {
    let cancellation = CancellationToken::new();
    cancellation.cancel();
    let outcome = runtime::execute(command("/bin/true", &[]), cancellation, |_| async {
        panic!("pre-cancelled commands must not spawn")
    })
    .await;
    assert!(matches!(
        outcome,
        Outcome::OperationalError(Error::Process(process::Error::Cancelled))
    ));
}

#[tokio::test]
async fn explicit_environment_and_literal_argv_are_preserved() {
    let mut env = command("/usr/bin/env", &[]);
    env.env = BTreeMap::from([("ONLY_EXPLICIT".into(), "value".into())]);
    assert_eq!(
        completed(execute(env).await).output.stdout,
        b"ONLY_EXPLICIT=value\n"
    );
    let result = completed(
        execute(command(
            "/usr/bin/printf",
            &["%s\n", "$(id); $HOME * a b", "{artifact}/path"],
        ))
        .await,
    );
    assert_eq!(
        result.output.stdout,
        b"$(id); $HOME * a b\n{artifact}/path\n"
    );
}

#[tokio::test]
async fn registration_observes_inert_group_leader_before_exec() {
    let scratch = Scratch::new();
    let marker = scratch.0.join("started");
    let mut command = command("/bin/sh", &["-c", "printf started > started; pwd"]);
    command.cwd = scratch.0.clone();
    let expected_cwd = scratch.0.clone();
    let output = process::run(
        command,
        CancellationToken::new(),
        move |identity| async move {
            assert!(!marker.exists());
            let stat = std::fs::read_to_string(format!("/proc/{}/stat", identity.pid))?;
            let fields: Vec<_> = stat
                .rsplit_once(')')
                .unwrap()
                .1
                .split_whitespace()
                .collect();
            assert_eq!(fields[2].parse::<u32>().unwrap(), identity.pid);
            assert_eq!(fields[19].parse::<u64>().unwrap(), identity.start_time);
            tokio::time::sleep(Duration::from_millis(50)).await;
            assert!(
                !marker.exists(),
                "user code ran before registration finished"
            );
            Ok(())
        },
    )
    .await
    .unwrap();
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap().trim(),
        expected_cwd.display().to_string()
    );
    assert_eq!(
        std::fs::read(scratch.0.join("started")).unwrap(),
        b"started"
    );
}

#[tokio::test]
async fn failed_registration_never_executes_and_reaps_the_child() {
    let scratch = Scratch::new();
    let mut command = command("/bin/sh", &["-c", "touch started"]);
    command.cwd = scratch.0.clone();
    let pid = Arc::new(AtomicU32::new(0));
    let observed = pid.clone();
    let result = process::run(
        command,
        CancellationToken::new(),
        move |identity| async move {
            observed.store(identity.pid, Ordering::SeqCst);
            Err(io::Error::other("registration rejected"))
        },
    )
    .await;
    assert!(matches!(result, Err(process::Error::Registration(_))));
    assert!(!scratch.0.join("started").exists());
    assert_gone(pid.load(Ordering::SeqCst)).await;
}

#[tokio::test]
async fn dropping_the_caller_during_registration_does_not_release_the_gate() {
    let scratch = Scratch::new();
    let mut command = command("/bin/sh", &["-c", "touch started"]);
    command.cwd = scratch.0.clone();
    let (registered, child) = oneshot::channel::<ChildIdentity>();
    let running = tokio::spawn(process::run(
        command,
        CancellationToken::new(),
        |identity| async move {
            registered.send(identity).unwrap();
            std::future::pending().await
        },
    ));
    let identity = child.await.unwrap();
    running.abort();
    assert!(running.await.unwrap_err().is_cancelled());
    assert_gone(identity.pid).await;
    assert!(!scratch.0.join("started").exists());
}

#[tokio::test]
async fn registration_is_covered_by_the_deadline() {
    let scratch = Scratch::new();
    let mut command = command("/bin/sh", &["-c", "touch started"]);
    command.cwd = scratch.0.clone();
    command.timeout = Duration::from_millis(100);
    let pid = Arc::new(AtomicU32::new(0));
    let observed = pid.clone();
    let result = process::run(
        command,
        CancellationToken::new(),
        move |identity| async move {
            observed.store(identity.pid, Ordering::SeqCst);
            std::future::pending().await
        },
    )
    .await;
    assert!(matches!(result, Err(process::Error::Timeout)));
    assert!(!scratch.0.join("started").exists());
    assert_gone(pid.load(Ordering::SeqCst)).await;
}

#[tokio::test]
async fn invalid_cwd_is_a_spawn_error_before_registration() {
    let mut command = command("/bin/true", &[]);
    command.cwd = PathBuf::from("/artifactize/nonexistent-directory");
    let result = timeout(
        Duration::from_secs(2),
        process::run(command, CancellationToken::new(), |_| async {
            panic!("an invalid cwd must not reach registration")
        }),
    )
    .await
    .unwrap();
    assert!(
        matches!(result, Err(process::Error::Spawn(error)) if error.kind() == io::ErrorKind::NotFound)
    );
}

#[tokio::test]
async fn timeout_kills_a_grandchild_even_when_the_leader_ignores_term() {
    let scratch = Scratch::new();
    let mut command = command(
        "/bin/sh",
        &["-c", "trap '' TERM; sleep 30 & echo $! > grandchild; wait"],
    );
    command.cwd = scratch.0.clone();
    command.timeout = Duration::from_millis(500);
    let outcome = execute(command).await;
    assert!(matches!(
        outcome,
        Outcome::OperationalError(Error::Process(process::Error::Timeout))
    ));
    let pid = std::fs::read_to_string(scratch.0.join("grandchild"))
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    assert_gone(pid).await;
}

#[tokio::test]
async fn normal_exit_also_kills_a_background_descendant() {
    let result = completed(execute(command("/bin/sh", &["-c", "sleep 30 & echo $!"])).await);
    assert_eq!(result.verdict, Verdict::Green);
    let pid = String::from_utf8(result.output.stdout)
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    assert_gone(pid).await;
}

#[tokio::test]
async fn stdout_and_stderr_are_drained_concurrently_after_capture_limit() {
    let result = completed(
        execute(command(
            "/bin/sh",
            &[
                "-c",
                "head -c 262144 /dev/zero & head -c 262144 /dev/zero >&2 & wait",
            ],
        ))
        .await,
    );
    assert_eq!(result.verdict, Verdict::Green);
    assert_eq!(result.output.stdout.len(), 128 * 1024);
    assert_eq!(result.output.stderr.len(), 128 * 1024);
    assert!(result.output.truncated);
}

async fn assert_gone(pid: u32) {
    assert!(pid > 0, "must have observed a real process");
    wait_for(|| !PathBuf::from(format!("/proc/{pid}")).exists()).await;
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

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target")
            .join(format!(
                "runtime-test-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::SeqCst)
            ));
        std::fs::create_dir_all(&path).unwrap();
        Self(std::fs::canonicalize(path).unwrap())
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}
