//! The private publication entry point delegates cancellation and temporary-space
//! ownership to the same runtime used by its eval, on every operating system.
#[path = "../../artifactize-core/tests/support/mod.rs"]
mod support;

use artifactize_core::{process, runtime};
use std::{fs, path::PathBuf};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::TcpListener,
};
use tokio_util::sync::CancellationToken;

const CRATES: [&str; 4] = [
    "artifactize-tools",
    "artifactize-core",
    "artifactize-tui",
    "artifactize",
];

#[tokio::test]
async fn publication_cancellation_reaps_the_build_script_before_releasing_owned_tmp() {
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
            "use std::io::{{Read, Write}};\nfn main() {{ let mut s = std::net::TcpStream::connect(\"{address}\").unwrap(); writeln!(s, \"{{}}\", std::env::var(\"OUT_DIR\").unwrap()).unwrap(); let mut b = [0]; if s.read_exact(&mut b).is_ok() {{ s.write_all(b\"alive\").unwrap(); }} }}\n"
        ),
    )
    .unwrap();
    let mut command = runtime::Command::prepare(
        "node".into(),
        vec![helper.into_os_string()],
        &workspace,
        &root.path().join("runs"),
        None,
    )
    .unwrap();
    command.cwd = cli;
    let scratch = command.directory().join("tmp");
    let cancellation = CancellationToken::new();
    let _cancel_on_unwind = cancellation.clone().drop_guard();
    let mut running = support::spawn(runtime::execute(command, cancellation.clone(), |_| async {
        Ok(())
    }));
    let (socket, _) = tokio::select! {
        connection = listener.accept() => connection.unwrap(),
        outcome = &mut running => panic!("publication exited before the build script connected: {outcome:?}"),
    };
    let mut socket = BufReader::new(socket);
    let mut line = String::new();
    tokio::select! {
        ready = socket.read_line(&mut line) => { ready.unwrap(); },
        outcome = &mut running => panic!("publication exited before the build script was ready: {outcome:?}"),
    }
    let build_output = fs::canonicalize(PathBuf::from(line.trim()));
    let owned_tmp = fs::canonicalize(&scratch);
    cancellation.cancel();
    let outcome = running.await.unwrap();
    // Let an incorrectly surviving script answer and exit, so this negative check
    // owns its fixture even when the runtime's cancellation contract regresses.
    let _ = socket.get_mut().write_all(b"probe").await;
    let mut response = Vec::new();
    socket.read_to_end(&mut response).await.unwrap();
    assert!(
        matches!(
            outcome,
            runtime::Outcome::OperationalError(runtime::Error::Process(process::Error::Cancelled))
        ),
        "{outcome:?}"
    );
    assert!(build_output.unwrap().starts_with(owned_tmp.unwrap()));
    assert!(
        response.is_empty(),
        "the build script survived: {response:?}"
    );
    assert!(!scratch.exists(), "runtime-owned tmp must be released");
}
