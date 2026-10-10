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

use artifactize::process::{self, ChildIdentity, Command};
use support::os::bin;
use tokio::{sync::oneshot, time::timeout};
use tokio_util::sync::CancellationToken;

mod support;

const TEST_TIMEOUT: Duration = Duration::from_secs(5);

fn command(program: &str, args: &[&str]) -> Command {
    Command {
        program: bin(program).into(),
        args: args.iter().map(OsString::from).collect(),
        cwd: support::os::temp_root(),
        env: BTreeMap::from([("PATH".into(), support::os::path())]),
        timeout: TEST_TIMEOUT,
    }
}

async fn execute(command: Command) -> process::Output {
    timeout(
        TEST_TIMEOUT + Duration::from_secs(2),
        process::run(command, CancellationToken::new(), |_| async { Ok(()) }),
    )
    .await
    .expect("runner must settle")
    .unwrap()
}

#[tokio::test]
async fn explicit_environment_and_literal_argv_are_preserved() {
    let mut env = command("/usr/bin/env", &[]);
    env.env = BTreeMap::from([("ONLY_EXPLICIT".into(), "value".into())]);
    assert_eq!(execute(env).await.stdout, b"ONLY_EXPLICIT=value\n");
    let result = execute(command(
        "/usr/bin/printf",
        &["%s\n", "$(id); $HOME * a b", "{artifact}/path"],
    ))
    .await;
    assert_eq!(result.stdout, b"$(id); $HOME * a b\n{artifact}/path\n");
}

#[tokio::test]
async fn registration_observes_inert_group_leader_before_exec() {
    let scratch = Scratch::new();
    let marker = scratch.0.join("started");
    let mut command = command("/bin/sh", &["-c", "printf started > started; pwd"]);
    command.cwd = scratch.0.clone();
    let expected_cwd = scratch.0.clone();
    let (registered, identity) = oneshot::channel();
    let (release, held) = oneshot::channel();
    let running = tokio::spawn(process::run(
        command,
        CancellationToken::new(),
        move |child| async move {
            assert!(!marker.exists());
            support::os::assert_group(child.pid);
            assert_eq!(support::os::start_time(child.pid), child.start_time);
            registered.send(child).unwrap();
            held.await.unwrap();
            assert!(
                !marker.exists(),
                "user code ran before registration finished"
            );
            Ok(())
        },
    ));
    let child = identity.await.unwrap();
    assert!(support::os::running(child.pid));
    assert!(!scratch.0.join("started").exists());
    release.send(()).unwrap();
    let output = running.await.unwrap().unwrap();
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap().trim(),
        support::os::canonical(&expected_cwd).display().to_string()
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
    let result = process::run(command, CancellationToken::new(), move |child| async move {
        observed.store(child.pid, Ordering::SeqCst);
        Err(io::Error::other("registration rejected"))
    })
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
        |child| async move {
            registered.send(child).unwrap();
            std::future::pending().await
        },
    ));
    let child = child.await.unwrap();
    running.abort();
    assert!(running.await.unwrap_err().is_cancelled());
    assert_gone(child.pid).await;
    assert!(!scratch.0.join("started").exists());
}

// The paused clock moves only when the test advances it: the held child keeps tokio from
// advancing it on its own, so the deadline passes once registration is underway.
#[tokio::test(start_paused = true)]
async fn registration_is_covered_by_the_deadline() {
    let scratch = Scratch::new();
    let mut command = command("/bin/sh", &["-c", "touch started"]);
    command.cwd = scratch.0.clone();
    command.timeout = Duration::from_secs(60);
    let (registered, pid) = oneshot::channel();
    let running = tokio::spawn(process::run(
        command,
        CancellationToken::new(),
        |child: ChildIdentity| async move {
            registered.send(child.pid).unwrap();
            std::future::pending().await
        },
    ));
    let pid = pid.await.unwrap();
    tokio::time::advance(Duration::from_secs(60)).await;
    assert!(matches!(
        running.await.unwrap(),
        Err(process::Error::Timeout)
    ));
    assert!(!scratch.0.join("started").exists());
    assert_gone(pid).await;
}

#[tokio::test]
async fn invalid_cwd_is_a_spawn_error_before_registration() {
    let mut command = command("/bin/true", &[]);
    let scratch = Scratch::new();
    command.cwd = scratch.0.join("nonexistent-directory");
    let result = timeout(
        Duration::from_secs(2),
        process::run(command, CancellationToken::new(), |_| async {
            panic!("an invalid cwd must not reach registration")
        }),
    )
    .await
    .unwrap();
    // Windows reports a missing working directory as ERROR_DIRECTORY.
    assert!(matches!(
        result,
        Err(process::Error::Spawn(error)) if support::os::missing_program(&error)
    ));
}

#[tokio::test]
async fn stdout_and_stderr_are_drained_concurrently_after_capture_limit() {
    let result = execute(command(
        "/bin/sh",
        &["-c", &support::os::zero_output(262144, 262144)],
    ))
    .await;
    assert!(result.status.success());
    assert_eq!(result.stdout.len(), 128 * 1024);
    assert_eq!(result.stderr.len(), 128 * 1024);
    assert!(result.truncated);
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

struct Scratch(
    PathBuf,
    #[expect(
        dead_code,
        reason = "owns the temporary fixture for the duration of the test"
    )]
    tempfile::TempDir,
);

impl Scratch {
    fn new() -> Self {
        let root = support::os::tempdir();
        Self(root.path().to_owned(), root)
    }
}
