//! Owned native process-tree fixtures for integration tests.

use super::os::compile;
use std::path::Path;

/// The child's TCP connection proves startup; EOF proves the whole tree stopped.
/// The owned runtime thread keeps cancellation and reaping alive even when the calling
/// future is dropped or unwinds. There is no detached accept worker.
pub async fn timeout_tree(
    root: &Path,
) -> Result<artifactize_tools::launch::Finished, artifactize_tools::launch::LaunchError> {
    let mut tree = NativeTree::start(root).await;
    tree.running.expire.take().unwrap().send(()).unwrap();
    let result = (&mut tree.running.result).await.unwrap();
    tree.running.join();
    assert_closed(&mut tree.stream);
    result
}

/// Exercise fixture ownership when an async caller unwinds after the native handshake.
pub async fn unwind_tree(root: &Path) {
    let tree = NativeTree::start(root).await;
    let mut stream = tree.stream.try_clone().unwrap();
    let unwind = tokio::spawn(async move {
        let _tree = tree;
        panic!("intentional fixture unwind");
    })
    .await
    .unwrap_err();
    assert!(unwind.is_panic());
    assert_closed(&mut stream);
}

// These bound broken startup/cleanup, never decide expiry or success by elapsed time.
const NATIVE_WATCHDOG: std::time::Duration = std::time::Duration::from_secs(20);
// Startup has its own watchdog; expiry is requested explicitly only after the handshake.
const TREE_EXPIRY: std::time::Duration = std::time::Duration::from_secs(3600);

struct NativeTree {
    running: NativeRun,
    stream: std::net::TcpStream,
}

impl Drop for NativeTree {
    fn drop(&mut self) {
        // Keep the TCP handshake owned until the worker has cancelled and reaped
        // its process tree, even when the caller is aborted or unwinds.
        self.running.stop();
    }
}

impl NativeTree {
    async fn start(root: &Path) -> Self {
        let program = compile(
            root,
            "spawner",
            r#"fn main() {
            let args: Vec<String> = std::env::args().collect();
            if args.get(1).map(String::as_str) == Some("child") {
                let _stream = std::net::TcpStream::connect(&args[2]).unwrap();
                loop { std::thread::park(); }
            } else {
                let mut child = std::process::Command::new(std::env::current_exe().unwrap())
                    .arg("child").arg(&args[1]).spawn().unwrap();
                // Own the child while the process tree is alive, rather than detach it.
                child.wait().unwrap();
            }
        }"#,
        );
        let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        // This fixture writes no stdout/stderr; reserve a small bounded audit capture.
        const AUDIT_BYTES: usize = 1024;
        let mut running = NativeRun::start(artifactize_tools::launch::Launch {
            program,
            args: vec![listener.local_addr().unwrap().to_string().into()],
            cwd: root.to_owned(),
            timeout: TREE_EXPIRY,
            output_limit: AUDIT_BYTES,
        });
        let stream = tokio::select! {
            result = &mut running.result => panic!("help ended before its native child handshake: {result:?}"),
            accepted = tokio::time::timeout(NATIVE_WATCHDOG, listener.accept()) => {
                accepted.expect("native child startup watchdog").unwrap().0
            }
        };
        let stream = stream.into_std().unwrap();
        stream.set_nonblocking(false).unwrap();
        Self { running, stream }
    }
}

struct NativeRun {
    cancellation: tokio_util::sync::CancellationToken,
    expire: Option<tokio::sync::oneshot::Sender<()>>,
    result: tokio::sync::oneshot::Receiver<
        Result<artifactize_tools::launch::Finished, artifactize_tools::launch::LaunchError>,
    >,
    worker: Option<std::thread::JoinHandle<()>>,
}

impl NativeRun {
    fn start(launch: artifactize_tools::launch::Launch) -> Self {
        use artifactize_tools::launch::{Launcher, Standalone};
        let cancellation = tokio_util::sync::CancellationToken::new();
        let worker_cancellation = cancellation.clone();
        let (expire, expiry) = tokio::sync::oneshot::channel();
        let (result_sender, result) = tokio::sync::oneshot::channel();
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
                    _ = expiry => {
                        tokio::time::pause();
                        tokio::time::advance(timeout).await;
                        running.await
                    }
                }
            });
            // A dropped caller need not receive the result, but still owns this thread.
            let _ = result_sender.send(result);
        });
        Self {
            cancellation,
            expire: Some(expire),
            result,
            worker: Some(worker),
        }
    }

    fn stop(&mut self) {
        self.cancellation.cancel();
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
        // Startup can be cancelled before NativeTree acquires the TCP stream.
        self.stop();
    }
}

fn assert_closed(stream: &mut std::net::TcpStream) {
    use std::io::Read;
    stream.set_read_timeout(Some(NATIVE_WATCHDOG)).unwrap();
    match stream.read(&mut [0]) {
        Ok(read) => assert_eq!(read, 0, "the help child outlived tree cleanup"),
        Err(error) => assert!(
            matches!(
                error.kind(),
                std::io::ErrorKind::ConnectionReset | std::io::ErrorKind::ConnectionAborted
            ),
            "the help child outlived tree cleanup: {error}"
        ),
    }
}
