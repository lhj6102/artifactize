//! Native PTY and process lifetime. Every program gets a PTY of its own: macOS revokes a
//! terminal once its session leader exits, so a restored argv and the fresh shell after it
//! cannot share one, and a new PTY starts from clean line settings wherever it runs. The
//! host's screen carries what was shown from one PTY to the next.
use super::{Foreground, Processes, Size};
use crate::{platform, terminal::TerminalCommand};
use portable_pty::{Child, CommandBuilder, MasterPty, SlavePty};
use std::{
    io::{self, Read, Write},
    path::Path,
    sync::{Arc, mpsc},
};

/// Small reads bound queued terminal output while keeping interactive delivery prompt.
const OUTPUT_CHUNK: usize = 8192;
/// A blocked foreground reader must not block the host; cap queued writes independently.
const INPUT_QUEUE: usize = 256;

pub(crate) enum Output {
    Data(Vec<u8>),
    /// A program's terminal closed; see [`Pty::is_current`].
    Closed(Generation),
}

/// Numbers the PTYs of one host in start order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Generation(u64);

type Sink = Arc<dyn Fn(Output) + Send + Sync>;

/// One program on its own PTY. Fields drop in order: the program is stopped before its
/// terminal closes.
struct Session {
    processes: Processes,
    child: Box<dyn Child + Send + Sync>,
    master: Box<dyn MasterPty + Send>,
    // Held open so a Linux PTY outlives its program's own descriptors until it is replaced.
    _slave: Box<dyn SlavePty + Send>,
    input: mpsc::SyncSender<Vec<u8>>,
}
impl Session {
    fn stop(&mut self) -> io::Result<()> {
        self.processes.stop()?;
        // A process already reaped by try_wait needs no signal, but wait still returns its status.
        let _ = self.child.kill();
        self.child.wait()?;
        Ok(())
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

pub(crate) struct Pty {
    session: Option<Session>,
    size: Size,
    output: Sink,
    generation: Generation,
}
impl Pty {
    pub fn open(
        cwd: &Path,
        size: Size,
        output: impl Fn(Output) + Send + Sync + 'static,
    ) -> io::Result<Self> {
        let mut pty = Self {
            session: None,
            size,
            output: Arc::new(output),
            generation: Generation(0),
        };
        pty.run(cwd, None)?;
        Ok(pty)
    }
    pub fn run(&mut self, cwd: &Path, command: Option<&TerminalCommand>) -> io::Result<()> {
        self.stop()?;
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

        let pair = portable_pty::native_pty_system()
            .openpty(self.size)
            .map_err(io::Error::other)?;
        let mut writer = pair.master.take_writer().map_err(io::Error::other)?;
        let mut reader = pair.master.try_clone_reader().map_err(io::Error::other)?;
        let mut child = pair.slave.spawn_command(launch).map_err(io::Error::other)?;
        let processes = match Processes::new(&*child) {
            Ok(processes) => processes,
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error);
            }
        };
        let (input, pending) = mpsc::sync_channel::<Vec<u8>>(INPUT_QUEUE);
        // From here the session owns the program, so a failed thread start still stops it.
        let session = Session {
            processes,
            child,
            master: pair.master,
            _slave: pair.slave,
            input,
        };
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
        let output = self.output.clone();
        let generation = Generation(self.generation.0 + 1);
        std::thread::Builder::new()
            .name("terminal-output".into())
            .spawn(move || {
                let mut buffer = [0u8; OUTPUT_CHUNK];
                loop {
                    match reader.read(&mut buffer) {
                        Ok(0) | Err(_) => {
                            output(Output::Closed(generation));
                            break;
                        }
                        // A replaced program's last output still belongs on the screen.
                        Ok(count) => output(Output::Data(buffer[..count].to_vec())),
                    }
                }
            })?;
        self.session = Some(session);
        self.generation = generation;
        Ok(())
    }
    /// Whether `generation` is the PTY of the program running now. A replaced PTY's close
    /// arrives late and says nothing about the current program.
    pub fn is_current(&self, generation: Generation) -> bool {
        self.session.is_some() && generation == self.generation
    }
    pub fn stop(&mut self) -> io::Result<()> {
        if let Some(mut session) = self.session.take() {
            session.stop()?;
        }
        Ok(())
    }
    pub fn input(&self, data: Vec<u8>) -> io::Result<()> {
        let session = self
            .session
            .as_ref()
            .ok_or_else(|| io::Error::other("No program is running"))?;
        session.input.try_send(data).map_err(io::Error::other)
    }
    pub fn resize(&mut self, size: Size) -> io::Result<()> {
        if let Some(session) = &self.session {
            session.master.resize(size).map_err(io::Error::other)?;
        }
        self.size = size;
        Ok(())
    }
    pub fn foreground(&self) -> Option<Foreground> {
        let session = self.session.as_ref()?;
        super::foreground(&*session.master, session.child.process_id())
    }
    pub fn try_wait(&mut self) -> io::Result<Option<u32>> {
        match &mut self.session {
            Some(session) => Ok(session.child.try_wait()?.map(|exit| exit.exit_code())),
            None => Ok(None),
        }
    }
}
