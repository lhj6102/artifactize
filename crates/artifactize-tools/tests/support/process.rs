//! Owned native process-tree fixtures for integration tests.

use super::os::compile;
use artifactize_tools::launch::{Finished, Launch, LaunchError};
use std::{io::Read, net::TcpStream, path::Path, time::Duration};

/// Expire the launcher only after both processes have connected, and check both exits
/// before teardown can interfere with the observation.
pub async fn timeout_tree(root: &Path) -> Result<Finished, LaunchError> {
    let mut tree = NativeTree::start(root).await;
    tree.running.expire.take().unwrap().send(()).unwrap();
    let result = tokio::time::timeout(NATIVE_WATCHDOG, &mut tree.running.result)
        .await
        .expect("native launcher expiry watchdog")
        .unwrap();
    tree.connections.assert_terminated();
    result
}

/// Exercise fixture ownership when an async caller unwinds after the native handshake.
pub async fn unwind_tree(root: &Path) {
    let tree = NativeTree::start(root).await;
    let mut connections = tree.connections.clone_streams();
    let unwind = tokio::spawn(async move {
        let _tree = tree;
        panic!("intentional fixture unwind");
    })
    .await
    .unwrap_err();
    assert!(unwind.is_panic());
    connections.assert_terminated();
}

// These bound broken startup/cleanup, never decide expiry or success by elapsed time.
const NATIVE_WATCHDOG: Duration = Duration::from_secs(20);
// Even if startup or cleanup stalls, the fixture processes exit without the launcher.
const FIXTURE_FAILSAFE: Duration = Duration::from_secs(120);
// Expiry is requested explicitly only after both handshakes.
const TREE_EXPIRY: Duration = Duration::from_secs(3600);

struct NativeTree {
    running: NativeRun,
    connections: Connections,
}

impl NativeTree {
    async fn start(root: &Path) -> Self {
        let program = compile(
            root,
            "spawner",
            r#"use std::io::{Read, Write};
fn main() {
    let mut args = std::env::args().skip(1);
    let address = args.next().unwrap();
    let watchdog = args.next().unwrap();
    let child = args.next().is_some();
    let mut stream = std::net::TcpStream::connect(&address).unwrap();
    stream.set_read_timeout(Some(std::time::Duration::from_secs(watchdog.parse().unwrap()))).unwrap();
    stream.write_all(if child { b"C" } else { b"P" }).unwrap();
    let child_process = (!child).then(|| std::process::Command::new(std::env::current_exe().unwrap())
        .args([&address, &watchdog, "child"]).spawn().unwrap());
    // Do not exit just because the other process died. Read blocks until the test
    // ends the connection or the independent watchdog expires.
    let _ = stream.read(&mut [0]);
    if let Some(mut child) = child_process {
        child.wait().unwrap();
    }
    // Let the operating system close our stream at process exit, not before.
    std::process::exit(0);
}"#,
        );
        let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        // This fixture writes no stdout/stderr; reserve a small bounded audit capture.
        const AUDIT_BYTES: usize = 1024;
        let mut running = NativeRun::start(Launch {
            program,
            args: vec![
                listener.local_addr().unwrap().to_string().into(),
                FIXTURE_FAILSAFE.as_secs().to_string().into(),
            ],
            cwd: root.to_owned(),
            timeout: TREE_EXPIRY,
            output_limit: AUDIT_BYTES,
        });
        let connections = tokio::select! {
            result = &mut running.result => panic!("help ended before the native tree handshake: {result:?}"),
            accepted = tokio::time::timeout(NATIVE_WATCHDOG, Connections::accept(&listener)) => {
                accepted.expect("native tree startup watchdog")
            }
        };
        Self {
            running,
            connections,
        }
    }
}

impl Drop for NativeTree {
    fn drop(&mut self) {
        // First give the product cancellation a chance to work with the observation
        // sockets still open. Then signal the fallback even if observers cloned them.
        self.running.cancellation.cancel();
        let stopped = self.running.wait(NATIVE_WATCHDOG);
        self.connections.release();
        if !stopped {
            // A launcher waiting on the live tree can now finish and be joined.
            self.running.wait(NATIVE_WATCHDOG);
            assert!(std::thread::panicking(), "native launcher cleanup watchdog");
        }
    }
}

struct Connections {
    parent: TcpStream,
    child: TcpStream,
}

impl Connections {
    async fn accept(listener: &tokio::net::TcpListener) -> Self {
        use tokio::io::AsyncReadExt;
        let (mut parent, mut child) = (None, None);
        for _ in 0..2 {
            let (mut stream, _) = listener.accept().await.unwrap();
            let slot = match stream.read_u8().await.unwrap() {
                b'P' => &mut parent,
                b'C' => &mut child,
                role => panic!("unknown fixture process role: {role}"),
            };
            let stream = stream.into_std().unwrap();
            stream.set_nonblocking(false).unwrap();
            assert!(
                slot.replace(stream).is_none(),
                "duplicate fixture process role"
            );
        }
        Self {
            parent: parent.unwrap(),
            child: child.unwrap(),
        }
    }

    fn clone_streams(&self) -> Self {
        Self {
            parent: self.parent.try_clone().unwrap(),
            child: self.child.try_clone().unwrap(),
        }
    }

    fn release(&self) {
        for stream in [&self.parent, &self.child] {
            let _ = stream.shutdown(std::net::Shutdown::Write);
        }
    }

    fn assert_terminated(&mut self) {
        for (role, stream) in [("parent", &mut self.parent), ("child", &mut self.child)] {
            stream.set_read_timeout(Some(NATIVE_WATCHDOG)).unwrap();
            match stream.read(&mut [0]) {
                Ok(read) => assert_eq!(read, 0, "the help {role} outlived tree cleanup"),
                Err(error) => assert!(
                    matches!(
                        error.kind(),
                        std::io::ErrorKind::ConnectionReset | std::io::ErrorKind::ConnectionAborted
                    ),
                    "the help {role} outlived tree cleanup: {error}"
                ),
            }
        }
    }
}

struct NativeRun {
    cancellation: tokio_util::sync::CancellationToken,
    expire: Option<tokio::sync::oneshot::Sender<()>>,
    result: tokio::sync::oneshot::Receiver<Result<Finished, LaunchError>>,
    done: std::sync::mpsc::Receiver<()>,
    worker: Option<std::thread::JoinHandle<()>>,
}

impl NativeRun {
    fn start(launch: Launch) -> Self {
        use artifactize_tools::launch::{Launcher, Standalone};
        let cancellation = tokio_util::sync::CancellationToken::new();
        let worker_cancellation = cancellation.clone();
        let (expire, expiry) = tokio::sync::oneshot::channel();
        let (result_sender, result) = tokio::sync::oneshot::channel();
        let (completed, done) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            let result = runtime.block_on(async {
                let timeout = launch.timeout;
                let mut running = Standalone.run(launch, &worker_cancellation);
                tokio::select! {
                    result = &mut running => result,
                    Ok(()) = expiry => {
                        tokio::time::pause();
                        tokio::time::advance(timeout).await;
                        running.await
                    }
                }
            });
            let _ = result_sender.send(result);
            drop(runtime);
            let _ = completed.send(());
        });
        Self {
            cancellation,
            expire: Some(expire),
            result,
            done,
            worker: Some(worker),
        }
    }

    fn wait(&mut self, limit: Duration) -> bool {
        if self.worker.is_none() {
            return true;
        }
        if self.done.recv_timeout(limit) == Err(std::sync::mpsc::RecvTimeoutError::Timeout) {
            // A launcher regression cannot strand test teardown in an unbounded join.
            // Closing the fixture connections independently releases the native processes.
            return false;
        }
        if let Err(panic) = self.worker.take().expect("worker is owned").join()
            && !std::thread::panicking()
        {
            std::panic::resume_unwind(panic);
        }
        true
    }
}

impl Drop for NativeRun {
    fn drop(&mut self) {
        // Startup can fail before NativeTree acquires the sockets. In that case they
        // have already been dropped or the processes' own watchdog will release them.
        self.cancellation.cancel();
        let stopped = self.wait(NATIVE_WATCHDOG);
        assert!(
            stopped || std::thread::panicking(),
            "native launcher cleanup watchdog"
        );
    }
}
