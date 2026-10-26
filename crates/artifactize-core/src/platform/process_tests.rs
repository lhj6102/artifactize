//! The process layer's output contract on every operating system: a run's output reaches its
//! end however its siblings start, and however slowly its evidence reaches the disk.

use std::{collections::BTreeMap, future::Future, time::Duration};

use tokio::{sync::oneshot, time::timeout};
use tokio_util::sync::CancellationToken;

use crate::process::{self, Command};

/// Enough runs at once that many of them start while others are held before exec.
const RUNS: usize = 32;

// A stalled fixture fails within this watchdog; ordering uses handshakes, not sleeps.
const WATCHDOG: Duration = Duration::from_secs(30);

fn echo(text: &str) -> Command {
    Command {
        program: crate::test_os::echo_program().into(),
        args: vec![text.into()],
        cwd: crate::test_os::temp_root(),
        env: BTreeMap::from([("PATH".into(), crate::test_os::path())]),
        timeout: WATCHDOG,
    }
}

#[tokio::test]
async fn a_sibling_held_before_exec_does_not_keep_another_runs_output_open() {
    let (first_held, first_holding) = oneshot::channel();
    let (second_held, second_holding) = oneshot::channel::<()>();
    let (first_returned, first_over) = oneshot::channel::<()>();
    let first = tokio::spawn(process::run(
        echo("first"),
        CancellationToken::new(),
        move |_| async move {
            first_held.send(()).unwrap();
            // The second run starts while this one is still held before exec.
            let _ = second_holding.await;
            Ok(())
        },
    ));
    first_holding.await.unwrap();
    let second = tokio::spawn(process::run(
        echo("second"),
        CancellationToken::new(),
        move |_| async move {
            second_held.send(()).unwrap();
            // Held before exec until the first run has returned.
            let _ = first_over.await;
            Ok(())
        },
    ));
    let first = timeout(WATCHDOG, first).await.unwrap().unwrap().unwrap();
    first_returned.send(()).unwrap();
    let second = timeout(WATCHDOG, second).await.unwrap().unwrap().unwrap();
    assert!(first.status.success());
    assert_eq!(first.stdout, b"first\n");
    assert!(second.status.success());
    assert_eq!(second.stdout, b"second\n");
}

/// Await `future` while keeping the runtime busy, so that a paused clock moves only when the
/// test advances it, never by itself while real processes run.
async fn busy<F: Future>(future: F) -> F::Output {
    tokio::pin!(future);
    loop {
        tokio::select! {
            biased;
            output = &mut future => return output,
            () = tokio::task::yield_now() => {}
        }
    }
}

#[tokio::test(start_paused = true)]
async fn many_short_runs_keep_their_exit_and_output_while_their_evidence_waits_for_a_slow_disk() {
    let disk = process::slow_disk::hold();
    let scratch = crate::test_os::tempdir();
    let mut runs: Vec<_> = (0..RUNS)
        .map(|index| {
            let directory = scratch.path().join(index.to_string());
            std::fs::create_dir(&directory).unwrap();
            let (started, pid) = oneshot::channel();
            let run = tokio::spawn(process::run_logged(
                echo(&format!("run {index}")),
                directory.clone(),
                CancellationToken::new(),
                move |child| async move {
                    started.send(child.pid.get()).unwrap();
                    Ok(())
                },
            ));
            (directory, pid, run)
        })
        .collect();
    for (_, pid, _) in &mut runs {
        let pid = busy(pid).await.unwrap();
        busy(async {
            while !crate::test_os::gone(pid) {
                tokio::task::yield_now().await;
            }
        })
        .await;
    }
    // Every process has ended while none of its evidence is saved yet; twice as long passes
    // as any pipe may stay open, and less than the runs' deadline.
    tokio::time::advance(process::OUTPUT_CLOSE_GRACE * 2).await;
    disk.send(true).unwrap();
    for (index, (directory, _, run)) in runs.into_iter().enumerate() {
        let output = busy(run).await.unwrap().unwrap();
        let expected = format!("run {index}\n").into_bytes();
        assert!(output.status.success());
        assert_eq!(output.stdout, expected);
        assert_eq!(
            std::fs::read(directory.join("runtime.stdout")).unwrap(),
            expected
        );
        assert!(directory.join("runtime.stdout.complete").exists());
        assert!(directory.join("runtime.stderr.complete").exists());
    }
}

/// A process the test started outside the run's group, killed when the test ends.
#[cfg(unix)]
struct Left(u32);

#[cfg(unix)]
impl Drop for Left {
    fn drop(&mut self) {
        crate::test_os::kill(self.0);
    }
}

// Only on Unix can a descendant leave the run's process group: a Windows job that forbids
// breakaway, as the run's does, keeps every descendant in it.
#[cfg(unix)]
#[tokio::test(start_paused = true)]
async fn output_a_process_outside_the_group_still_holds_is_named_and_the_exit_is_kept() {
    let scratch = crate::test_os::tempdir();
    let record = scratch.path().join("left");
    let command = Command {
        program: crate::test_os::steps().into(),
        args: vec![
            "print".into(),
            "done".into(),
            "leave".into(),
            record.clone().into(),
        ],
        cwd: scratch.path().to_owned(),
        env: BTreeMap::from([("PATH".into(), crate::test_os::path())]),
        timeout: WATCHDOG,
    };
    let (started, pid) = oneshot::channel();
    let run = tokio::spawn(process::run(
        command,
        CancellationToken::new(),
        move |child| async move {
            started.send(child.pid.get()).unwrap();
            Ok(())
        },
    ));
    let leader = busy(pid).await.unwrap();
    busy(async {
        while !crate::test_os::gone(leader) {
            tokio::task::yield_now().await;
        }
    })
    .await;
    let _left = Left(
        std::fs::read_to_string(&record)
            .unwrap()
            .trim()
            .parse()
            .unwrap(),
    );
    // Time passes only here, a second at a time, until the run gives up on its pipes.
    let result = busy(async {
        while !run.is_finished() {
            tokio::time::advance(Duration::from_secs(1)).await;
        }
        run.await.unwrap()
    })
    .await;
    use crate::tools::human::log::Stream::{Stderr, Stdout};
    let Err(process::Error::Capture(error)) = result else {
        panic!("the held pipes must fail the capture: {result:?}");
    };
    assert!(error.output.status.success());
    assert_eq!(error.output.stdout, b"done");
    assert!(matches!(
        error.failures.as_slice(),
        [
            (Stdout, process::StreamFailure::Open),
            (Stderr, process::StreamFailure::Open)
        ]
    ));
}
