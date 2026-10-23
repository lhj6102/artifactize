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

use artifactize_core::process::{self, ChildIdentity, Command};
use tokio::{sync::oneshot, time::timeout};
use tokio_util::sync::CancellationToken;

mod support;

// A stalled fixture fails within this watchdog; ordering and expiry use handshakes or paused time.
const TEST_TIMEOUT: Duration = Duration::from_secs(5);

fn command(program: &str, args: &[&str]) -> Command {
    Command {
        program: program.into(),
        args: args.iter().map(OsString::from).collect(),
        cwd: support::os::temp_root(),
        env: BTreeMap::from([("PATH".into(), support::os::path())]),
        timeout: TEST_TIMEOUT,
    }
}

async fn execute(command: Command) -> process::Output {
    let cancellation = CancellationToken::new();
    let execution = process::run(command, cancellation.clone(), |_| async { Ok(()) });
    tokio::pin!(execution);
    match timeout(
        support::os::patience(TEST_TIMEOUT + Duration::from_secs(2)),
        &mut execution,
    )
    .await
    {
        Ok(output) => output.unwrap(),
        Err(_) => {
            // Settle the process and its supervisor before the fixture folders go.
            cancellation.cancel();
            let _ = execution.await;
            panic!("runner must settle");
        }
    }
}

#[tokio::test]
async fn explicit_environment_and_literal_argv_are_preserved() {
    let mut env = command(&support::os::env_program(), &[]);
    env.env = BTreeMap::from([("ONLY_EXPLICIT".into(), "value".into())]);
    assert_eq!(execute(env).await.stdout, b"ONLY_EXPLICIT=value\n");
    let result = execute(command(
        &support::os::printf_program(),
        &["%s\n", "$(id); $AUTHOR_VALUE * a b", "{artifact}/path"],
    ))
    .await;
    assert_eq!(
        result.stdout,
        b"$(id); $AUTHOR_VALUE * a b\n{artifact}/path\n"
    );
}

#[tokio::test]
async fn registration_observes_inert_group_leader_before_exec() {
    let scratch = Scratch::new();
    let marker = scratch.0.join("started");
    let mut command = command(
        &support::os::steps(),
        &["write", "started", "started", "cwd"],
    );
    command.cwd = scratch.0.clone();
    let expected_cwd = scratch.0.clone();
    let (registered, identity) = oneshot::channel();
    let (release, held) = oneshot::channel();
    let cancellation = CancellationToken::new();
    let work = process::run(command, cancellation.clone(), move |child| async move {
        assert!(!marker.exists());
        support::os::assert_group(child.pid.get());
        assert_eq!(support::os::start_time(child.pid.get()), child.start_time);
        registered.send(child).unwrap();
        held.await.unwrap();
        assert!(
            !marker.exists(),
            "user code ran before registration finished"
        );
        Ok(())
    });
    support::os::scoped(cancellation, work, async |running| {
        let child = identity.await.unwrap();
        assert!(support::os::running(child.pid.get()));
        assert!(!scratch.0.join("started").exists());
        release.send(()).unwrap();
        let output = running.await.unwrap().unwrap();
        assert!(output.status.success());
        assert_eq!(
            String::from_utf8(output.stdout).unwrap().trim(),
            support::os::path_text(&support::os::canonical(&expected_cwd))
        );
        assert_eq!(
            std::fs::read(scratch.0.join("started")).unwrap(),
            b"started"
        );
    })
    .await;
}

#[tokio::test]
async fn failed_registration_never_executes_and_reaps_the_child() {
    let scratch = Scratch::new();
    let mut command = command(&support::os::steps(), &["touch", "started"]);
    command.cwd = scratch.0.clone();
    let pid = Arc::new(AtomicU32::new(0));
    let observed = pid.clone();
    let result = process::run(command, CancellationToken::new(), move |child| async move {
        observed.store(child.pid.get(), Ordering::SeqCst);
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
    let mut command = command(&support::os::steps(), &["touch", "started"]);
    command.cwd = scratch.0.clone();
    let (registered, child) = oneshot::channel::<ChildIdentity>();
    let cancellation = CancellationToken::new();
    let work = process::run(command, cancellation.clone(), |child| async move {
        registered.send(child).unwrap();
        std::future::pending().await
    });
    support::os::scoped(cancellation, work, async |running| {
        let child = child.await.unwrap();
        // Dropping the caller is what is under test: abort it rather than cancel it.
        running.abort();
        assert_gone(child.pid.get()).await;
        assert!(!scratch.0.join("started").exists());
    })
    .await;
}

// The paused clock moves only when the test advances it: the held child keeps tokio from
// advancing it on its own, so the deadline passes once registration is underway.
#[tokio::test(start_paused = true)]
async fn registration_is_covered_by_the_deadline() {
    let scratch = Scratch::new();
    let mut command = command(&support::os::steps(), &["touch", "started"]);
    command.cwd = scratch.0.clone();
    command.timeout = Duration::from_secs(60);
    let (registered, pid) = oneshot::channel();
    let cancellation = CancellationToken::new();
    let work = process::run(
        command,
        cancellation.clone(),
        |child: ChildIdentity| async move {
            registered.send(child.pid).unwrap();
            std::future::pending().await
        },
    );
    support::os::scoped(cancellation, work, async |running| {
        let pid = pid.await.unwrap();
        tokio::time::advance(Duration::from_secs(60)).await;
        assert!(matches!(
            running.await.unwrap(),
            Err(process::Error::Timeout)
        ));
        assert!(!scratch.0.join("started").exists());
        assert_gone(pid.get()).await;
    })
    .await;
}

#[tokio::test]
async fn invalid_cwd_is_a_spawn_error_before_registration() {
    let mut command = command(&support::os::true_program(), &[]);
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
        &support::os::steps(),
        &["fill", "262144", "0", "262144", "0"],
    ))
    .await;
    assert!(result.status.success());
    assert_eq!(result.stdout.len(), 128 * 1024);
    assert_eq!(result.stderr.len(), 128 * 1024);
    assert!(result.truncated);
}

#[tokio::test]
async fn owned_human_runs_exit_before_fixture_removal_on_success_and_unwind() {
    use artifactize_core::{
        project::{VerifyOptions, selection::Selection},
        runs::Worker,
        types::RequestStatus,
    };

    for unwinding in [false, true] {
        let root = support::os::tempdir();
        let directory = root.path().to_owned();
        let repo = root.path().join("repo");
        let state = root.path().join("state");
        std::fs::create_dir(&repo).unwrap();
        std::fs::write(
            repo.join("index.artf"),
            "name = \"app\"\nfingerprint = {}\n[evals.review]\ntitle = \"Review\"\n\
             profile = { kind = \"human\" }\n[evals.review.payload]\ninstruction = \"Inspect.\"\n",
        )
        .unwrap();
        let fixture = support::runs::RepoLease::new(root, &state, &repo);
        let run = support::runs::record(
            &repo,
            Some(&state),
            &Selection::All,
            &VerifyOptions::default(),
            CancellationToken::new(),
        )
        .await
        .unwrap();
        assert_eq!(run.requests[0].status(), RequestStatus::Deferred);
        let Worker::Admitted { owner } = run.run.control.unwrap().worker else {
            panic!("the fixture worker was admitted");
        };
        assert!(support::os::same_process_running(
            owner.pid.get(),
            owner.start_time
        ));
        fixture.release().unwrap();
        assert!(!support::os::same_process_running(
            owner.pid.get(),
            owner.start_time
        ));
        assert!(
            repo.join("index.artf").is_file(),
            "explicit release retains inputs"
        );
        assert!(fixture.path().is_dir());
        assert_eq!(
            artifactize_core::store::read_run(&state, &run.run.id)
                .await
                .unwrap()
                .run
                .status(),
            artifactize_core::types::RunStatus::Error
        );
        // Explicit teardown must not disarm ownership of later submissions.
        let later = support::runs::record(
            &repo,
            Some(&state),
            &Selection::All,
            &VerifyOptions::default(),
            CancellationToken::new(),
        )
        .await
        .unwrap();
        assert_eq!(later.requests[0].status(), RequestStatus::Deferred);
        assert_ne!(run.run.id, later.run.id);
        let Worker::Admitted { owner } = later.run.control.unwrap().worker else {
            panic!("the later fixture worker was admitted");
        };
        // Move ownership into the unwind boundary, so no fixture is reused after a panic.
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let fixture = fixture;
            if unwinding {
                panic!("scripted fixture unwind");
            }
            drop(fixture);
        }));
        if unwinding {
            assert_eq!(
                result.unwrap_err().downcast_ref::<&str>(),
                Some(&"scripted fixture unwind")
            );
        } else {
            result.unwrap();
        }
        assert!(!support::os::same_process_running(
            owner.pid.get(),
            owner.start_time
        ));
        assert!(
            !directory.exists(),
            "only released fixture state is removed"
        );
    }
}

#[test]
fn failed_run_cleanup_retains_inputs_and_preserves_an_existing_panic() {
    // This fixture deliberately contains no worker: its invalid SQLite input exercises
    // retention on cleanup failure without leaving a process for this test to recover.
    struct RetainedDirectory(PathBuf);
    impl Drop for RetainedDirectory {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    for unwinding in [false, true] {
        let root = support::os::tempdir();
        let directory = root.path().to_owned();
        let retained = RetainedDirectory(directory.clone());
        let repo = root.path().join("repo");
        let state = root.path().join("state");
        std::fs::create_dir(&repo).unwrap();
        std::fs::create_dir(&state).unwrap();
        std::fs::write(repo.join("input"), "owned input").unwrap();
        std::fs::write(state.join(artifactize_core::store::DATABASE), "not SQLite").unwrap();
        let fixture = support::runs::RepoLease::new(root, &state, &repo);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let fixture = fixture;
            if unwinding {
                panic!("scripted original panic");
            }
            drop(fixture);
        }));
        let panic = result.unwrap_err();
        if unwinding {
            assert_eq!(
                panic.downcast_ref::<&str>(),
                Some(&"scripted original panic")
            );
        } else {
            assert!(
                panic
                    .downcast_ref::<String>()
                    .unwrap()
                    .starts_with("Fixture Run cleanup failed; inputs retained at ")
            );
        }
        assert_eq!(std::fs::read(repo.join("input")).unwrap(), b"owned input");
        drop(retained);
        assert!(
            !directory.exists(),
            "the failure-test fixture also owns its retained files"
        );
    }
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
