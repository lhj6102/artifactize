use std::{
    fs,
    io::{Read, Write},
    net::TcpListener,
    path::Path,
    process::{Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};

use serde_json::{Value, json};

mod support;

fn test_token() -> String {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).unwrap();
    let hex: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    format!("azt_test{hex}")
}

fn artifactize(state: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_artifactize"));
    command
        .arg("--state-dir")
        .arg(state)
        .env_remove("ARTIFACTIZE_REMOTE")
        .env_remove("ARTIFACTIZE_REMOTE_TOKEN")
        .env_remove("ARTIFACTIZE_REMOTE_SHARE");
    command
}

/// Run with `--json`; the output never contains `secret`.
fn run(command: &mut Command, stdin: &str, secret: &str, code: i32) -> Value {
    let mut child = command
        .arg("--json")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin.as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    let (stdout, stderr) = (
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    assert_eq!(output.status.code(), Some(code), "{stdout}{stderr}");
    assert!(!stdout.contains(secret) && !stderr.contains(secret));
    serde_json::from_slice(&output.stdout).unwrap()
}

struct Whoami {
    url: String,
    stop: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}

impl Whoami {
    fn start(token: &str) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!("http://{}/", listener.local_addr().unwrap());
        let stop = Arc::new(AtomicBool::new(false));
        let done = stop.clone();
        let expected = format!("authorization: Bearer {token}");
        let thread = thread::spawn(move || {
            while !done.load(Ordering::SeqCst) {
                let mut stream = match listener.accept() {
                    Ok((stream, _)) => stream,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(5));
                        continue;
                    }
                    Err(error) => panic!("{error}"),
                };
                stream.set_nonblocking(false).unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut request = Vec::new();
                let mut byte = [0];
                while !request.ends_with(b"\r\n\r\n") {
                    stream.read_exact(&mut byte).unwrap();
                    request.push(byte[0]);
                }
                let request = String::from_utf8(request).unwrap();
                assert!(
                    request.starts_with("GET /v1/whoami HTTP/1.1\r\n"),
                    "{request}"
                );
                let (status, body) = if request.lines().any(|line| line == expected) {
                    (
                        "200 OK",
                        json!({"principal":"alice-laptop","scopes":["read","publish"]}),
                    )
                } else {
                    (
                        "401 Unauthorized",
                        json!({"error":"Invalid or revoked token."}),
                    )
                };
                let body = body.to_string();
                write!(stream, "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
            }
        });
        Self {
            url,
            stop,
            thread: Some(thread),
        }
    }
}

impl Drop for Whoami {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        self.thread.take().unwrap().join().unwrap();
    }
}

#[test]
fn login_status_overrides_and_logout_against_a_test_server() {
    let root = tempfile::tempdir().unwrap();
    let state = root.path().join("state");
    let token = test_token();
    let server = Whoami::start(&token);

    let refused = run(
        artifactize(&state).args(["remote", "login", "http://reviews.example/"]),
        &token,
        &token,
        2,
    );
    assert!(refused["error"].as_str().unwrap().contains("https://"));
    let rejected = run(
        artifactize(&state).args(["remote", "login", &server.url]),
        &format!("{}\n", test_token()),
        &token,
        2,
    );
    assert!(rejected["error"].as_str().unwrap().contains("rejected"));
    assert!(!state.join("remote.json").exists());

    assert_eq!(
        run(
            artifactize(&state).args(["remote", "login", &server.url]),
            &format!("{token}\n"),
            &token,
            0
        ),
        json!({"url":server.url,"share":"summary","principal":"alice-laptop","scopes":["read","publish"]})
    );
    let saved = state.join("auth/remote-token.json");
    assert!(support::os::private_file(&saved));
    assert_eq!(
        serde_json::from_slice::<Value>(&fs::read(state.join("remote.json")).unwrap()).unwrap(),
        json!({"url":server.url,"share":"summary"})
    );

    assert_eq!(
        run(
            artifactize(&state).args(["remote", "status"]),
            "",
            &token,
            0
        ),
        json!({"configured":true,"url":server.url,"share":"summary","tokenSource":"file",
            "reachable":true,"principal":"alice-laptop","scopes":["read","publish"],"error":null})
    );
    let other = test_token();
    let overridden = run(
        artifactize(&state)
            .args(["remote", "status"])
            .env("ARTIFACTIZE_REMOTE_TOKEN", &other)
            .env("ARTIFACTIZE_REMOTE_SHARE", "full"),
        "",
        &other,
        1,
    );
    assert_eq!(
        (
            &overridden["tokenSource"],
            &overridden["share"],
            &overridden["reachable"],
            &overridden["principal"]
        ),
        (&json!("env"), &json!("full"), &json!(true), &Value::Null)
    );
    assert!(overridden["error"].as_str().unwrap().contains("rejected"));
    assert_eq!(
        run(
            artifactize(&state)
                .args(["remote", "status"])
                .env("ARTIFACTIZE_REMOTE", "off"),
            "",
            &token,
            1
        ),
        json!({"configured":false})
    );
    let closed = TcpListener::bind("127.0.0.1:0").unwrap();
    let closed_url = format!("http://{}/", closed.local_addr().unwrap());
    drop(closed);
    let elsewhere = run(
        artifactize(&state)
            .args(["remote", "status"])
            .env("ARTIFACTIZE_REMOTE", &closed_url),
        "",
        &token,
        2,
    );
    assert!(
        elsewhere["error"]
            .as_str()
            .unwrap()
            .contains("another store")
    );
    let unreachable = run(
        artifactize(&state)
            .args(["remote", "status"])
            .env("ARTIFACTIZE_REMOTE", &closed_url)
            .env("ARTIFACTIZE_REMOTE_TOKEN", &token),
        "",
        &token,
        1,
    );
    assert_eq!(unreachable["reachable"], false);
    assert!(
        unreachable["error"]
            .as_str()
            .unwrap()
            .contains("unreachable")
    );

    assert_eq!(
        run(
            artifactize(&state).args(["remote", "logout"]),
            "",
            &token,
            0
        ),
        json!({"signedIn":false})
    );
    assert!(!saved.exists() && !state.join("remote.json").exists());
}

#[test]
fn doctor_checks_remote_configuration_without_opening_a_socket() {
    let root = tempfile::tempdir().unwrap();
    let state = root.path().join("state");
    let bin = root.path().join("bin");
    support::os::create_private_dir_all(&state.join("auth"));
    fs::create_dir(&bin).unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let url = format!("http://{}/", listener.local_addr().unwrap());
    let token = test_token();
    let doctor = |code| {
        let report = run(
            artifactize(&state)
                .arg("doctor")
                .env("PATH", &bin)
                .env_remove("OPENAI_API_KEY")
                .env_remove("ANTHROPIC_API_KEY"),
            "",
            &token,
            code,
        );
        report["checks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|check| check["name"] == "remote")
            .unwrap()
            .clone()
    };
    assert_eq!(doctor(0)["status"], "PASS");

    fs::write(state.join("remote.json"), json!({"url":url}).to_string()).unwrap();
    let missing = doctor(0);
    assert_eq!(missing["status"], "WARN");
    assert_eq!(missing["details"]["tokenSource"], "none");

    let saved = state.join("auth/remote-token.json");
    let contents = json!({"url":url,"token":token}).to_string();
    support::os::write_private_file(&saved, &contents);
    support::os::grant_everyone_read(&saved);
    let unsafe_file = doctor(1);
    assert_eq!(unsafe_file["status"], "FAIL");
    assert!(unsafe_file["message"].as_str().unwrap().contains("0600"));
    // Owner-only again: mode 0600, or on Windows a new file with the folder's DACL.
    fs::remove_file(&saved).unwrap();
    support::os::write_private_file(&saved, &contents);
    let ready = doctor(0);
    assert_eq!(
        (&ready["status"], &ready["details"]["tokenSource"]),
        (&json!("PASS"), &json!("file"))
    );
    assert_eq!(
        listener.accept().unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock
    );

    fs::write(
        state.join("remote.json"),
        json!({"url":"http://reviews.example/"}).to_string(),
    )
    .unwrap();
    assert!(doctor(1)["message"].as_str().unwrap().contains("https://"));
}

/// Needs a pseudo-terminal from openpty(3); the Windows console has no such pair to drive.
#[cfg(unix)]
#[test]
fn login_on_a_terminal_does_not_echo_the_token() {
    use std::os::fd::{FromRawFd, OwnedFd};

    let root = tempfile::tempdir().unwrap();
    let token = test_token();
    let server = Whoami::start(&token);
    let (mut master, mut slave) = (0, 0);
    // SAFETY: openpty writes two new descriptors, owned below.
    let opened = unsafe {
        libc::openpty(
            &mut master,
            &mut slave,
            std::ptr::null_mut(),
            std::ptr::null(),
            std::ptr::null(),
        )
    };
    assert_eq!(opened, 0);
    // SAFETY: both descriptors were just opened and are owned exactly once.
    let (mut master, slave) =
        unsafe { (fs::File::from_raw_fd(master), OwnedFd::from_raw_fd(slave)) };
    let mut child = artifactize(&root.path().join("state"))
        .args(["remote", "login", &server.url])
        .stdin(slave.try_clone().unwrap())
        .stdout(slave.try_clone().unwrap())
        .stderr(slave)
        .spawn()
        .unwrap();
    // Reading the master fails with EIO once the child closed the terminal.
    fn read(master: &mut fs::File, output: &mut Vec<u8>, until: &str) {
        let mut buffer = [0; 1024];
        while !String::from_utf8_lossy(output).contains(until) {
            match master.read(&mut buffer) {
                Ok(0) | Err(_) => break,
                Ok(count) => output.extend_from_slice(&buffer[..count]),
            }
        }
    }
    let mut output = Vec::new();
    read(&mut master, &mut output, "Paste the remote token");
    master.write_all(format!("{token}\n").as_bytes()).unwrap();
    read(&mut master, &mut output, "Signed in");
    assert!(child.wait().unwrap().success());
    let output = String::from_utf8_lossy(&output);
    assert!(!output.contains(&token));
    assert!(output.contains("as alice-laptop"), "{output}");
}

/// Git Bash's mintty hands a program a pipe named like a pty: a terminal to `is_terminal`, but
/// no console whose echo can be turned off. Login then reads the token visibly and says so.
#[cfg(windows)]
#[test]
fn login_on_a_mintty_pipe_reads_the_token_visibly_with_a_warning() {
    use std::{os::windows::io::FromRawHandle, ptr};
    use windows_sys::Win32::{
        Foundation::{GENERIC_READ, INVALID_HANDLE_VALUE},
        Storage::FileSystem::{CreateFileW, OPEN_EXISTING, PIPE_ACCESS_OUTBOUND},
        System::Pipes::{CreateNamedPipeW, PIPE_TYPE_BYTE, PIPE_WAIT},
    };

    let root = tempfile::tempdir().unwrap();
    let token = test_token();
    let server = Whoami::start(&token);
    let name: Vec<u16> = format!(r"\\.\pipe\msys-{}-pty0-from-master", std::process::id())
        .encode_utf16()
        .chain([0])
        .collect();
    // SAFETY: a NUL-terminated name; each handle is checked, then owned by one File.
    let (mut writer, reader) = unsafe {
        let server_end = CreateNamedPipeW(
            name.as_ptr(),
            PIPE_ACCESS_OUTBOUND,
            PIPE_TYPE_BYTE | PIPE_WAIT,
            1,
            4096,
            4096,
            0,
            ptr::null(),
        );
        assert_ne!(server_end, INVALID_HANDLE_VALUE);
        let client_end = CreateFileW(
            name.as_ptr(),
            GENERIC_READ,
            0,
            ptr::null(),
            OPEN_EXISTING,
            0,
            ptr::null_mut(),
        );
        assert_ne!(client_end, INVALID_HANDLE_VALUE);
        (
            fs::File::from_raw_handle(server_end),
            fs::File::from_raw_handle(client_end),
        )
    };
    assert!(
        std::io::IsTerminal::is_terminal(&reader),
        "the pipe passes for a mintty terminal"
    );
    let child = artifactize(&root.path().join("state"))
        .args(["remote", "login", &server.url])
        .stdin(reader)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    // The pipe stays open meanwhile: a pty whose master closed is no longer a terminal.
    writer.write_all(format!("{token}\n").as_bytes()).unwrap();
    let output = child.wait_with_output().unwrap();
    drop(writer);
    let (stdout, stderr) = (
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    assert!(output.status.success(), "{stdout}{stderr}");
    assert!(stderr.contains("cannot hide input"), "{stdout}{stderr}");
    assert!(stderr.contains("Paste the remote token"), "{stderr}");
    assert!(stdout.contains("as alice-laptop"), "{stdout}");
    assert!(!stdout.contains(&token) && !stderr.contains(&token));
}
