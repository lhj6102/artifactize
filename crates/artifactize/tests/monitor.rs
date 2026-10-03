use std::{
    fs::{self, File},
    io::{Read, Write},
    mem::MaybeUninit,
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::process::CommandExt,
    },
    path::Path,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

use serde_json::json;

fn command(state: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_artifactize"));
    command.arg("--state-dir").arg(state);
    command
}

fn wait_for(mut ready: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !ready() {
        assert!(Instant::now() < deadline, "timed out waiting for monitor");
        std::thread::sleep(Duration::from_millis(20));
    }
}

struct ChildGuard(Child);

impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

struct Terminal {
    master: File,
    slave: File,
    original: libc::termios,
    output: Vec<u8>,
}

impl Terminal {
    fn new() -> Self {
        let mut master = -1;
        let mut slave = -1;
        let size = libc::winsize {
            ws_row: 45,
            ws_col: 160,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        assert_eq!(
            unsafe {
                libc::openpty(
                    &mut master,
                    &mut slave,
                    std::ptr::null_mut(),
                    std::ptr::null(),
                    &size,
                )
            },
            0
        );
        let master = unsafe { File::from_raw_fd(master) };
        let slave = unsafe { File::from_raw_fd(slave) };
        let original = attributes(&slave);
        let flags = unsafe { libc::fcntl(master.as_raw_fd(), libc::F_GETFL) };
        assert_ne!(flags, -1);
        assert_ne!(
            unsafe { libc::fcntl(master.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) },
            -1
        );
        Self {
            master,
            slave,
            original,
            output: Vec::new(),
        }
    }

    fn spawn(&self, command: &mut Command) -> ChildGuard {
        command
            .env("TERM", "xterm-256color")
            .stdin(self.slave.try_clone().unwrap())
            .stdout(self.slave.try_clone().unwrap())
            .stderr(self.slave.try_clone().unwrap());
        unsafe {
            command.pre_exec(|| {
                if libc::setsid() == -1 || libc::ioctl(libc::STDIN_FILENO, libc::TIOCSCTTY, 0) == -1
                {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        ChildGuard(command.spawn().unwrap())
    }

    fn read(&mut self) {
        let mut buffer = [0; 8192];
        loop {
            match self.master.read(&mut buffer) {
                Ok(0) => break,
                Ok(count) => self.output.extend_from_slice(&buffer[..count]),
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(error) => panic!("PTY read failed: {error}"),
            }
        }
    }

    fn expect(&mut self, text: &str) {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            self.read();
            if String::from_utf8_lossy(&self.output).contains(text) {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "missing {text:?}: {}",
                String::from_utf8_lossy(&self.output)
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn restored(&mut self) {
        self.read();
        assert!(String::from_utf8_lossy(&self.output).contains("\u{1b}[?1049l"));
        let restored = attributes(&self.slave);
        assert_eq!(restored.c_lflag, self.original.c_lflag);
        assert_eq!(restored.c_iflag, self.original.c_iflag);
        assert_eq!(restored.c_oflag, self.original.c_oflag);
        assert_eq!(restored.c_cc, self.original.c_cc);
    }
}

fn attributes(file: &File) -> libc::termios {
    let mut attributes = MaybeUninit::uninit();
    assert_eq!(
        unsafe { libc::tcgetattr(file.as_raw_fd(), attributes.as_mut_ptr()) },
        0
    );
    unsafe { attributes.assume_init() }
}

#[test]
fn monitor_cli_validates_flags_and_requires_a_terminal_without_creating_state() {
    let root = tempfile::tempdir().unwrap();
    let state = root.path().join("absent");
    let help = command(&state)
        .args(["monitor", "--help"])
        .output()
        .unwrap();
    assert!(help.status.success());
    let text = String::from_utf8_lossy(&help.stdout);
    assert!(text.contains("--all"));
    assert!(text.contains("--repo <PATH>"));
    for args in [
        vec!["monitor", "--all", "--repo", "."],
        vec!["--repo", ".", "monitor", "--all"],
        vec!["monitor", "--json"],
        vec!["monitor", "--all"],
    ] {
        assert_eq!(
            command(&state).args(args).output().unwrap().status.code(),
            Some(2)
        );
    }
    let output = command(&state).args(["monitor", "--all"]).output().unwrap();
    assert!(String::from_utf8_lossy(&output.stderr).contains("requires an interactive terminal"));
    assert!(!state.exists());
}

#[test]
fn monitor_observes_a_live_verify_and_restores_terminal_on_quit_and_sigterm() {
    let root = tempfile::tempdir().unwrap();
    let state = root.path().join("state");
    let repo = root.path().join("repo");
    let release = repo.join("release");
    fs::create_dir(&repo).unwrap();
    fs::write(repo.join("artifactize.json"), json!({
        "name":"sample",
        "evals":[{
            "id":"slow", "title":"Slow review",
            "profile":{"kind":"runtime", "command":"/bin/sh", "args":["-c", "while [ ! -e release ]; do sleep 0.05; done"]},
            "payload":{"instruction":"Wait for the test."}
        }]
    }).to_string()).unwrap();
    let mut verify = ChildGuard(
        command(&state)
            .arg("--repo")
            .arg(&repo)
            .args(["verify", "--all"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let mut terminal = Terminal::new();
    let mut monitor = terminal.spawn(command(&state).args(["monitor", "--all"]));
    terminal.expect("RUNNING");
    terminal.master.write_all(b"\r").unwrap();
    terminal.expect("Running evals (elapsed)");
    terminal.expect("sample/slow");
    fs::write(&release, "done").unwrap();
    wait_for(|| verify.0.try_wait().unwrap().is_some());
    assert!(verify.0.wait().unwrap().success());
    // No keypress: the periodic read must see the verifier's committed verdict.
    terminal.expect("SATISFIED");
    terminal.master.write_all(b"q").unwrap();
    wait_for(|| monitor.0.try_wait().unwrap().is_some());
    assert!(monitor.0.wait().unwrap().success());
    terminal.restored();

    terminal.output.clear();
    let mut monitor = terminal.spawn(command(&state).args(["monitor", "--all"]));
    terminal.expect("GREEN=1");
    assert_eq!(
        unsafe { libc::kill(monitor.0.id() as i32, libc::SIGTERM) },
        0
    );
    wait_for(|| monitor.0.try_wait().unwrap().is_some());
    assert!(monitor.0.wait().unwrap().success());
    terminal.restored();
}
