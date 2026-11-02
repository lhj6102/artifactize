//! Native PTY and process lifetime. The slave stays open so a restored argv and its following
//! fresh shell use the same terminal, including pending input, output and geometry.
use super::{Foreground, Modes, Processes, Size};
use crate::{platform, terminal::TerminalCommand};
use portable_pty::{Child, CommandBuilder, MasterPty, SlavePty};
use std::{
    io::{self, Read, Write},
    path::Path,
    sync::mpsc,
};

/// Small reads bound queued terminal output while keeping interactive delivery prompt.
const OUTPUT_CHUNK: usize = 8192;
/// A blocked foreground reader must not block the host; cap queued writes independently.
const INPUT_QUEUE: usize = 256;

pub(crate) enum Output {
    Data(Vec<u8>),
    Closed,
}

struct Running {
    processes: Processes,
    child: Box<dyn Child + Send + Sync>,
}
impl Running {
    fn stop(&mut self) -> io::Result<()> {
        self.processes.stop()?;
        // A process already reaped by try_wait needs no signal, but wait still returns its status.
        let _ = self.child.kill();
        self.child.wait()?;
        Ok(())
    }
}
impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

pub(crate) struct Pty {
    running: Option<Running>,
    slave: Box<dyn SlavePty + Send>,
    master: Box<dyn MasterPty + Send>,
    modes: Modes,
    input: mpsc::SyncSender<Vec<u8>>,
}
impl Pty {
    pub fn open(
        cwd: &Path,
        size: Size,
        output: impl Fn(Output) + Send + 'static,
    ) -> io::Result<Self> {
        let pair = portable_pty::native_pty_system()
            .openpty(size)
            .map_err(io::Error::other)?;
        let mut writer = pair.master.take_writer().map_err(io::Error::other)?;
        let mut reader = pair.master.try_clone_reader().map_err(io::Error::other)?;
        let (input, pending) = mpsc::sync_channel::<Vec<u8>>(INPUT_QUEUE);
        let modes = Modes::initial(&*pair.master)?;
        let mut pty = Self {
            running: None,
            slave: pair.slave,
            master: pair.master,
            modes,
            input,
        };
        pty.run(cwd, None)?;
        std::thread::Builder::new()
            .name("terminal-input".into())
            .spawn(move || {
                while let Ok(data) = pending.recv() {
                    if writer
                        .write_all(&data)
                        .and_then(|()| writer.flush())
                        .is_err()
                    {
                        break;
                    }
                }
            })?;
        std::thread::Builder::new()
            .name("terminal-output".into())
            .spawn(move || {
                let mut buffer = [0u8; OUTPUT_CHUNK];
                loop {
                    match reader.read(&mut buffer) {
                        Ok(0) | Err(_) => {
                            output(Output::Closed);
                            break;
                        }
                        Ok(count) => output(Output::Data(buffer[..count].to_vec())),
                    }
                }
            })?;
        Ok(pty)
    }
    pub fn run(&mut self, cwd: &Path, command: Option<&TerminalCommand>) -> io::Result<()> {
        self.stop()?;
        // Every program starts on the PTY's initial settings, not on what the last one left.
        self.modes.restore(&*self.master)?;
        let mut launch = match command {
            Some(command) => {
                let program = platform::program::resolve(command.program.as_ref(), cwd)?;
                let mut launch = CommandBuilder::new(program);
                launch.args(&command.args);
                launch
            }
            None => CommandBuilder::new_default_prog(),
        };
        launch.cwd(cwd);
        launch.env_clear();
        for (name, value) in platform::environment::for_command() {
            launch.env(name, value);
        }
        launch.env("TERM", "xterm-256color");
        let mut child = self.slave.spawn_command(launch).map_err(io::Error::other)?;
        let processes = match Processes::new(&*child) {
            Ok(processes) => processes,
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error);
            }
        };
        self.running = Some(Running { processes, child });
        Ok(())
    }
    pub fn stop(&mut self) -> io::Result<()> {
        if let Some(mut running) = self.running.take() {
            running.stop()?;
        }
        Ok(())
    }
    pub fn input(&self, data: Vec<u8>) -> io::Result<()> {
        self.input.try_send(data).map_err(io::Error::other)
    }
    pub fn resize(&self, size: Size) -> io::Result<()> {
        self.master.resize(size).map_err(io::Error::other)
    }
    pub fn foreground(&self) -> Option<Foreground> {
        super::foreground(&*self.master, self.running.as_ref()?.child.process_id())
    }
    pub fn try_wait(&mut self) -> io::Result<Option<u32>> {
        match &mut self.running {
            Some(running) => Ok(running.child.try_wait()?.map(|exit| exit.exit_code())),
            None => Ok(None),
        }
    }
}

// TEMPORARY diagnostic for #440 Windows: what ConPTY prints before and after a cursor answer.
#[cfg(all(test, windows))]
mod diagnostic {
    use super::*;
    use std::time::{Duration, Instant};
    fn collect(receiver: &mpsc::Receiver<Vec<u8>>, wait: Duration) -> Vec<u8> {
        let deadline = Instant::now() + wait;
        let mut bytes = Vec::new();
        while let Some(left) = deadline.checked_duration_since(Instant::now()) {
            match receiver.recv_timeout(left) {
                Ok(data) => bytes.extend(data),
                Err(_) => break,
            }
        }
        bytes
    }
    #[test]
    fn conpty_first_output() {
        let (sender, receiver) = mpsc::channel();
        let cwd = std::env::current_dir().unwrap();
        let size = Size {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        };
        let mut pty = Pty::open(&cwd, size, move |output| {
            if let Output::Data(data) = output {
                let _ = sender.send(data);
            }
        })
        .unwrap();
        let before = collect(&receiver, Duration::from_secs(8));
        pty.input(b"\x1b[1;1R".to_vec()).unwrap();
        let after = collect(&receiver, Duration::from_secs(8));
        pty.input(b"echo hello\r".to_vec()).unwrap();
        let echoed = collect(&receiver, Duration::from_secs(5));
        let _ = pty.stop();
        panic!(
            "DIAG before={:?}\nDIAG after={:?}\nDIAG echoed={:?}",
            String::from_utf8_lossy(&before),
            String::from_utf8_lossy(&after),
            String::from_utf8_lossy(&echoed)
        );
    }
}
