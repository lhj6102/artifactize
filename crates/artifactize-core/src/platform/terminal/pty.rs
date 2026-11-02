//! Native PTY and process lifetime. Where a PTY outlives the program leading it, a restored
//! argv and the fresh shell after it share one, including input typed ahead and geometry;
//! every program starts from the PTY's initial line settings. Where it does not (macOS),
//! each program gets a PTY of its own. The host's screen carries what was shown either way.
use super::{Foreground, Modes, PTY_PER_PROGRAM, Processes, Size};
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
    /// A PTY closed; see [`Pty::is_current`].
    Closed(Generation),
}

/// Numbers the PTYs of one host in open order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Generation(u64);

type Sink = Arc<dyn Fn(Output) + Send + Sync>;

/// One PTY with its input and output threads.
struct Terminal {
    master: Box<dyn MasterPty + Send>,
    // Held open so the PTY outlives each program's own descriptors, where the system allows.
    slave: Box<dyn SlavePty + Send>,
    modes: Modes,
    input: mpsc::SyncSender<Vec<u8>>,
    generation: Generation,
}
impl Terminal {
    fn open(size: Size, generation: Generation, output: Sink) -> io::Result<Self> {
        let pair = portable_pty::native_pty_system()
            .openpty(size)
            .map_err(io::Error::other)?;
        let mut writer = pair.master.take_writer().map_err(io::Error::other)?;
        let mut reader = pair.master.try_clone_reader().map_err(io::Error::other)?;
        let modes = Modes::initial(&*pair.master)?;
        let (input, pending) = mpsc::sync_channel::<Vec<u8>>(INPUT_QUEUE);
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
                            output(Output::Closed(generation));
                            break;
                        }
                        // A replaced PTY's last output still belongs on the screen.
                        Ok(count) => output(Output::Data(buffer[..count].to_vec())),
                    }
                }
            })?;
        Ok(Self {
            master: pair.master,
            slave: pair.slave,
            modes,
            input,
            generation,
        })
    }
}

/// The program running on the terminal now.
struct Program {
    processes: Processes,
    child: Box<dyn Child + Send + Sync>,
}
impl Program {
    fn stop(&mut self) -> io::Result<()> {
        self.processes.stop()?;
        // A process already reaped by try_wait needs no signal, but wait still returns its status.
        let _ = self.child.kill();
        self.child.wait()?;
        Ok(())
    }
}
impl Drop for Program {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

pub(crate) struct Pty {
    // Declared first: a program is stopped before its terminal closes.
    program: Option<Program>,
    terminal: Option<Terminal>,
    size: Size,
    output: Sink,
    opened: u64,
}
impl Pty {
    pub fn open(
        cwd: &Path,
        size: Size,
        output: impl Fn(Output) + Send + Sync + 'static,
    ) -> io::Result<Self> {
        let mut pty = Self {
            program: None,
            terminal: None,
            size,
            output: Arc::new(output),
            opened: 0,
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
        let terminal = match self.terminal.take() {
            Some(terminal) if !PTY_PER_PROGRAM => {
                // Every program starts on the PTY's initial settings, not on what the last
                // one left (a shell's line editor turns echo off while it waits).
                terminal.modes.restore(&*terminal.master)?;
                terminal
            }
            _ => {
                self.opened += 1;
                Terminal::open(self.size, Generation(self.opened), self.output.clone())?
            }
        };
        let terminal = self.terminal.insert(terminal);
        let mut child = terminal
            .slave
            .spawn_command(launch)
            .map_err(io::Error::other)?;
        let processes = match Processes::new(&*child) {
            Ok(processes) => processes,
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error);
            }
        };
        self.program = Some(Program { processes, child });
        Ok(())
    }
    /// Whether `generation` is the PTY in use now. A replaced PTY's close arrives late and
    /// says nothing about the program running now.
    pub fn is_current(&self, generation: Generation) -> bool {
        self.terminal
            .as_ref()
            .is_some_and(|terminal| terminal.generation == generation)
    }
    pub fn stop(&mut self) -> io::Result<()> {
        if let Some(mut program) = self.program.take() {
            program.stop()?;
        }
        Ok(())
    }
    pub fn input(&self, data: Vec<u8>) -> io::Result<()> {
        let terminal = self
            .terminal
            .as_ref()
            .ok_or_else(|| io::Error::other("The terminal is not open"))?;
        terminal.input.try_send(data).map_err(io::Error::other)
    }
    pub fn resize(&mut self, size: Size) -> io::Result<()> {
        if let Some(terminal) = &self.terminal {
            terminal.master.resize(size).map_err(io::Error::other)?;
        }
        self.size = size;
        Ok(())
    }
    pub fn foreground(&self) -> Option<Foreground> {
        super::foreground(
            &*self.terminal.as_ref()?.master,
            self.program.as_ref()?.child.process_id(),
        )
    }
    pub fn try_wait(&mut self) -> io::Result<Option<u32>> {
        match &mut self.program {
            Some(program) => Ok(program.child.try_wait()?.map(|exit| exit.exit_code())),
            None => Ok(None),
        }
    }
}
