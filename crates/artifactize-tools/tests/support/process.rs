//! Owned native process-tree fixtures for integration tests.

use super::os::compile;
use artifactize_tools::launch::{Finished, Launch, LaunchError};
use std::{io::Read, net::TcpStream, path::Path, time::Duration};
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

type LaunchResult = Result<Finished, LaunchError>;

/// Expire the launcher only after both processes have connected, and check both exits
/// before teardown can interfere with the observation.
pub async fn timeout_tree(root: &Path) -> LaunchResult {
    let (mut tree, result) = NativeTree::start(root).await;
    tree.running.expire.take().unwrap().send(()).unwrap();
    let result = tokio::time::timeout(NATIVE_WATCHDOG, result)
        .await
        .expect("native launcher expiry watchdog")
        .unwrap();
    tree.connections.assert_terminated();
    result
}

/// Exercise fixture ownership when an async caller unwinds after the native handshake.
pub async fn unwind_tree(root: &Path) {
    let (tree, result) = NativeTree::start(root).await;
    let mut connections = tree.connections.clone_streams();
    let unwind = tokio::spawn(async move {
        let _tree = tree;
        panic!("intentional fixture unwind");
    })
    .await
    .unwrap_err();
    assert!(unwind.is_panic());
    assert!(matches!(result.await.unwrap(), Err(LaunchError::Cancelled)));
    connections.assert_terminated();
}

// These bound broken startup/cleanup, never decide expiry or success by elapsed time.
const NATIVE_WATCHDOG: Duration = Duration::from_secs(20);
// Expiry is requested explicitly only after both handshakes.
const TREE_EXPIRY: Duration = Duration::from_secs(3600);

struct NativeTree {
    running: NativeRun,
    connections: Connections,
}

impl NativeTree {
    async fn start(root: &Path) -> (Self, oneshot::Receiver<LaunchResult>) {
        let program = compile(
            root,
            "spawner",
            r#"use std::io::{Read, Write};
fn main() {
    let mut args = std::env::args().skip(1);
    let address = args.next().unwrap();
    let child = args.next().is_some();
    let mut stream = std::net::TcpStream::connect(&address).unwrap();
    stream.write_all(if child { b"C" } else { b"P" }).unwrap();
    let child_process = (!child).then(|| std::process::Command::new(std::env::current_exe().unwrap())
        .args([&address, "child"]).spawn().unwrap());
    // Do not exit when the other process dies. Closing the test side of this
    // socket independently releases each process if launcher cleanup fails.
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
        let (running, mut result) = NativeRun::start(Launch {
            program,
            args: vec![listener.local_addr().unwrap().to_string().into()],
            cwd: root.to_owned(),
            timeout: TREE_EXPIRY,
            output_limit: AUDIT_BYTES,
        });
        let connections = {
            // If the handshake fails, close the listening socket and all partial
            // connections before the worker's Drop waits for launcher cleanup.
            let listener = listener;
            tokio::select! {
                result = &mut result => panic!("help ended before the native tree handshake: {result:?}"),
                accepted = tokio::time::timeout(NATIVE_WATCHDOG, Connections::accept(&listener)) => {
                    accepted.expect("native tree startup watchdog")
                }
            }
        };
        (
            Self {
                running,
                connections,
            },
            result,
        )
    }
}

impl Drop for NativeTree {
    fn drop(&mut self) {
        // Let the product demonstrate cancellation before using the independent
        // socket shutdown, which also works when observers cloned the streams.
        if !self.running.cancel_and_wait() {
            self.connections.release();
            self.running.abort_and_join();
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
    cancellation: CancellationToken,
    abort: CancellationToken,
    expire: Option<oneshot::Sender<()>>,
    done: std::sync::mpsc::Receiver<()>,
    worker: Option<std::thread::JoinHandle<()>>,
}

impl NativeRun {
    fn start(launch: Launch) -> (Self, oneshot::Receiver<LaunchResult>) {
        use artifactize_tools::launch::{Launcher, Standalone};
        let cancellation = CancellationToken::new();
        let worker_cancellation = cancellation.clone();
        let abort = CancellationToken::new();
        let worker_abort = abort.clone();
        let (expire, expiry) = oneshot::channel();
        let (result_sender, result) = oneshot::channel();
        let (completed, done) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            let result = runtime.block_on(async {
                let run = async {
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
                };
                tokio::select! {
                    result = run => Some(result),
                    _ = worker_abort.cancelled() => None,
                }
            });
            if let Some(result) = result {
                let _ = result_sender.send(result);
            }
            drop(runtime);
            let _ = completed.send(());
        });
        (
            Self {
                cancellation,
                abort,
                expire: Some(expire),
                done,
                worker: Some(worker),
            },
            result,
        )
    }

    fn cancel_and_wait(&mut self) -> bool {
        self.cancellation.cancel();
        if self.worker.is_some()
            && self.done.recv_timeout(NATIVE_WATCHDOG)
                == Err(std::sync::mpsc::RecvTimeoutError::Timeout)
        {
            return false;
        }
        self.join();
        true
    }

    fn abort_and_join(&mut self) {
        self.abort.cancel();
        self.join();
    }

    fn join(&mut self) {
        if let Some(worker) = self.worker.take()
            && let Err(panic) = worker.join()
            && !std::thread::panicking()
        {
            std::panic::resume_unwind(panic);
        }
    }
}

impl Drop for NativeRun {
    fn drop(&mut self) {
        if !self.cancel_and_wait() {
            // The tree owner released its connections, or startup failed and the
            // listener and partial connections were already dropped.
            self.abort_and_join();
            assert!(std::thread::panicking(), "native launcher cleanup watchdog");
        }
    }
}
