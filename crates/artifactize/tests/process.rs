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
        cwd: PathBuf::from("/"),
        env: BTreeMap::from([("PATH".into(), "/usr/bin:/bin".into())]),
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
    let output = process::run(command, CancellationToken::new(), move |child| async move {
        assert!(!marker.exists());
        // A Unix child leads its own process group; Windows holds it in a Job Object instead.
        #[cfg(unix)]
        assert!(support::os::leads_group(child.pid));
        assert_eq!(support::os::start_time(child.pid), child.start_time);
        // Windows holds the child suspended in its job; its identity is already final.

        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(
            !marker.exists(),
            "user code ran before registration finished"
        );
        Ok(())
    })
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

#[tokio::test]
async fn registration_is_covered_by_the_deadline() {
    let scratch = Scratch::new();
    let mut command = command("/bin/sh", &["-c", "touch started"]);
    command.cwd = scratch.0.clone();
    command.timeout = Duration::from_millis(100);
    let pid = Arc::new(AtomicU32::new(0));
    let observed = pid.clone();
    let result = process::run(command, CancellationToken::new(), move |child| async move {
        observed.store(child.pid, Ordering::SeqCst);
        std::future::pending().await
    })
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
    // Windows reports a missing working directory as ERROR_DIRECTORY.
    assert!(matches!(
        result,
        Err(process::Error::Spawn(error)) if error.kind() == io::ErrorKind::NotFound
            || (cfg!(windows) && error.kind() == io::ErrorKind::NotADirectory)
    ));
}

#[tokio::test]
async fn stdout_and_stderr_are_drained_concurrently_after_capture_limit() {
    let result = execute(command(
        "/bin/sh",
        &[
            "-c",
            "head -c 262144 /dev/zero & head -c 262144 /dev/zero >&2 & wait",
        ],
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
        Self(support::os::canonical(&path))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}
