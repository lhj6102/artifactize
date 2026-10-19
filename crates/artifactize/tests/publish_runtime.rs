//! The private publication entry point delegates cancellation and temporary-space
//! ownership to the same runtime used by its eval, on every operating system.
#[path = "../../artifactize-core/tests/support/mod.rs"]
mod support;

#[path = "support/run.rs"]
mod owned_run;

use artifactize_core::{process, runtime};
use futures_util::FutureExt;
use std::{fs, path::PathBuf, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::{TcpListener, TcpStream},
};
use tokio_util::sync::CancellationToken;

const CRATES: [&str; 5] = [
    "artifactize-tools",
    "artifactize-core",
    "artifactize-tui",
    "artifactize-mcp",
    "artifactize",
];

#[tokio::test]
async fn publication_cancellation_reaps_the_build_script_before_releasing_owned_tmp() {
    let mut released = false;
    publication_fixture(false, &mut released).await;
    assert!(released);
}

#[tokio::test]
async fn publication_unwind_joins_cancellation_before_releasing_the_fixture() {
    let mut released = false;
    let failed = std::panic::AssertUnwindSafe(publication_fixture(true, &mut released))
        .catch_unwind()
        .await;
    // Preserving the original assertion must not hide a failed teardown assertion.
    assert!(
        released,
        "publication teardown must also succeed during unwind"
    );
    assert_eq!(
        failed.unwrap_err().downcast_ref::<&str>(),
        Some(&"publication fixture assertion")
    );
}

async fn publication_fixture(unwind: bool, released: &mut bool) {
    let root = support::os::tempdir();
    let workspace = root.path().join("workspace");
    fs::create_dir(&workspace).unwrap();
    fs::write(
        workspace.join("Cargo.toml"),
        "[workspace]\nmembers = [\"crates/*\"]\nresolver = \"3\"\n",
    )
    .unwrap();
    let mut lock = "version = 4\n".to_owned();
    for (index, name) in CRATES.iter().enumerate() {
        let directory = workspace.join("crates").join(name);
        fs::create_dir_all(directory.join("src")).unwrap();
        let dependency = if index == 0 {
            String::new()
        } else {
            format!(
                "\n[dependencies]\n{} = {{ path = \"../{}\", version = \"0.0.0-dev\" }}\n",
                CRATES[index - 1],
                CRATES[index - 1]
            )
        };
        fs::write(
            directory.join("Cargo.toml"),
            format!(
                "[package]\nname = \"{name}\"\nversion = \"0.0.0-dev\"\nedition = \"2024\"\nlicense = \"Apache-2.0\"\ndescription = \"Publication cancellation fixture\"\n{dependency}"
            ),
        )
        .unwrap();
        fs::write(directory.join("src/lib.rs"), "pub fn fixture() {}\n").unwrap();
        lock.push_str(&format!(
            "\n[[package]]\nname = \"{name}\"\nversion = \"0.0.0-dev\"\n"
        ));
        if index > 0 {
            lock.push_str(&format!("dependencies = [\"{}\"]\n", CRATES[index - 1]));
        }
    }
    fs::write(workspace.join("Cargo.lock"), lock).unwrap();
    // Both Cargo directories must be invocation-owned even when the author's real
    // configuration names other locations. Do not replace HOME or CARGO_HOME.
    let author_target = root.path().join("author-target");
    let author_build = root.path().join("author-build");
    fs::create_dir(workspace.join(".cargo")).unwrap();
    fs::write(
        workspace.join(".cargo/config.toml"),
        format!(
            "[build]\ntarget-dir = {}\nbuild-dir = {}\n",
            serde_json::json!(support::os::path_text(&author_target)),
            serde_json::json!(support::os::path_text(&author_build)),
        ),
    )
    .unwrap();
    let cli = workspace.join("crates/artifactize");
    let helper = cli.join("publish.mjs");
    fs::write(&helper, include_str!("../publish.mjs")).unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    // OUT_DIR proves the real Cargo build script reached the owned staging tree.
    // A leaked script answers the post-cancellation probe instead of hanging the test.
    fs::write(
        workspace.join("crates/artifactize-tools/build.rs"),
        format!(
            "use std::io::{{Read, Write}};\nfn main() {{ let mut s = std::net::TcpStream::connect(\"{address}\").unwrap(); writeln!(s, \"{{}}\\t{{}}\", std::process::id(), std::env::var(\"OUT_DIR\").unwrap()).unwrap(); let mut b = [0]; if s.read_exact(&mut b).is_ok() {{ s.write_all(b\"alive\").unwrap(); }} }}\n"
        ),
    )
    .unwrap();
    let mut command = runtime::Command::prepare(
        support::os::node_program().into(),
        vec![helper.into_os_string()],
        &workspace,
        &root.path().join("runs"),
        None,
    )
    .unwrap();
    command.cwd = cli;
    let scratch = command.directory().join("tmp");
    let cancellation = CancellationToken::new();
    // Cancellation must run the runtime's teardown to completion, not abort its future.
    let running = runtime::execute(command, cancellation.clone(), |_| async { Ok(()) });
    tokio::pin!(running);
    let mut outcome = None;
    let mut socket = None;
    let mut processes = owned_run::Processes::default();
    let mut identities = Vec::new();
    // This bounds a broken readiness handshake, never the Cargo build's lifetime.
    const READY_BOUND: Duration = Duration::from_secs(30);
    let observed = std::panic::AssertUnwindSafe(async {
        tokio::time::timeout(support::os::patience(READY_BOUND), async {
            let (connected, _) = tokio::select! {
                connection = listener.accept() => connection.unwrap(),
                finished = &mut running => {
                    outcome = Some(finished);
                    panic!("publication exited before the build script connected: {outcome:?}");
                },
            };
            socket = Some(BufReader::new(connected));
            let mut line = String::new();
            tokio::select! {
                ready = socket.as_mut().unwrap().read_line(&mut line) => { ready.unwrap(); },
                finished = &mut running => {
                    outcome = Some(finished);
                    panic!("publication exited before the build script was ready: {outcome:?}");
                },
            }
            let (pid, output) = line
                .trim()
                .split_once('\t')
                .expect("build script identity and OUT_DIR");
            let script: u32 = pid.parse().unwrap();
            processes.own(script);
            let cargo = support::os::parent_pid(script)
                .expect("the real Cargo owns the blocked build script");
            processes.own(cargo);
            identities = [script, cargo]
                .map(|pid| (pid, support::os::start_time(pid)))
                .to_vec();
            let build_output = support::os::canonical(&PathBuf::from(output));
            let owned_tmp = support::os::canonical(&scratch);
            assert!(build_output.starts_with(owned_tmp));
            assert!(
                !author_target.exists(),
                "Cargo target-dir must not use the author's configuration"
            );
            assert!(
                !author_build.exists(),
                "Cargo build-dir must not use the author's configuration"
            );
            assert!(!unwind, "publication fixture assertion");
        })
        .await
        .expect("publication build script did not become ready");
    })
    .catch_unwind()
    .await;
    cancellation.cancel();
    let closed = std::panic::AssertUnwindSafe(async {
        let outcome = match outcome {
            Some(outcome) => outcome,
            None => running.await,
        };
        // A cancelled Outcome or closed socket is not proof that Cargo and its build
        // script exited. Observe both exact birth identities before the cleanup probe
        // could itself let an incorrectly surviving script finish.
        let survivors: Vec<_> = identities
            .iter()
            .copied()
            .filter(|&(pid, start)| support::os::same_process_running(pid, start))
            .collect();
        // Let an incorrectly surviving script answer and exit, so this negative check
        // owns its fixture even when the runtime's cancellation contract regresses.
        if let Some(socket) = &mut socket {
            let response = support::os::closed_tcp_probe(socket, b"probe")
                .await
                .unwrap();
            assert!(
                response.is_empty(),
                "the build script survived: {response:?}"
            );
        }
        assert!(
            matches!(
                outcome,
                runtime::Outcome::OperationalError(runtime::Error::Process(
                    process::Error::Cancelled
                ))
            ),
            "{outcome:?}"
        );
        assert!(
            survivors.is_empty(),
            "Cargo/build-script survived cancellation: {survivors:?}"
        );
        assert!(!scratch.exists(), "runtime-owned tmp must be released");
        assert!(!author_target.exists());
        assert!(!author_build.exists());
    })
    .catch_unwind()
    .await;
    drop(socket);
    // The fallback guard joins real birth identities even when an observation or
    // teardown assertion failed. Never delete live fixture inputs after a failed join.
    let joined = std::panic::catch_unwind(|| drop(processes));
    let closed = match closed.and(joined) {
        Ok(()) => {
            std::panic::catch_unwind(|| root.close().expect("remove joined publication inputs"))
        }
        Err(panic) => {
            let retained = root.keep();
            eprintln!(
                "Publication teardown failed; inputs retained at {}",
                support::os::path_text(&retained)
            );
            Err(panic)
        }
    };
    *released = closed.is_ok();
    match (observed, closed) {
        (Err(panic), _) | (_, Err(panic)) => std::panic::resume_unwind(panic),
        (Ok(()), Ok(())) => {}
    }
}

#[tokio::test]
async fn publication_closed_probe_preserves_a_live_peers_buffered_reply() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let peer = async {
        let (mut socket, _) = listener.accept().await?;
        socket.write_all(b"ready\nalive").await?;
        // Leaving the rest of the probe unread may reset TCP on close. The
        // already-observed application reply must still not disappear.
        let mut probe = [0];
        socket.read_exact(&mut probe).await?;
        Ok::<_, std::io::Error>(probe)
    };
    let observer = async {
        let mut socket = BufReader::new(TcpStream::connect(address).await?);
        let mut ready = String::new();
        socket.read_line(&mut ready).await?;
        // Observe at least one real reply byte before letting the peer terminate.
        // This is a protocol barrier, not an assumption about TCP packet grouping.
        let buffered = !socket.fill_buf().await?.is_empty();
        let response = support::os::closed_tcp_probe(&mut socket, b"probe").await?;
        Ok::<_, std::io::Error>((ready, buffered, response))
    };
    let (probe, (ready, buffered, response)) = tokio::try_join!(peer, observer).unwrap();
    assert_eq!(probe, [b'p']);
    assert_eq!(ready, "ready\n");
    assert!(buffered);
    assert!(
        !response.is_empty(),
        "a live reply must not become a successful closed observation"
    );
    assert_eq!(response[0], b'a');
}
