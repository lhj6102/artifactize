//! The host serializes PTY state and attachments in one actor. Slow clients detach instead
//! of dropping output; their next attach receives a complete screen snapshot.
use super::{
    RestoreChoice, RestoreOffer, TerminalFrame, dimensions,
    metadata::Metadata,
    screen::Screen,
    wire::{self, Request, Response},
};
use crate::{
    local_transport::{Endpoint, Stream, validate_peer},
    platform,
};
use portable_pty::{Child, CommandBuilder, MasterPty, PtySize};
use std::{
    collections::BTreeMap,
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::mpsc,
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    sync::mpsc as async_mpsc,
};

/// Bounded queues keep a hidden or stalled desktop from consuming the host's memory.
const CLIENT_QUEUE: usize = 256;
/// Worktree deletion and foreground state are sampled even with no attached desktop.
const OBSERVE_INTERVAL: Duration = Duration::from_secs(1);
/// PTY reads are kept small enough for interactive delivery and bounded transport queues.
const OUTPUT_CHUNK: usize = 8192;

enum Event {
    Attach {
        id: u64,
        cols: u16,
        rows: u16,
        sender: async_mpsc::Sender<Response>,
    },
    Request {
        id: u64,
        request: Request,
    },
    Detach(u64),
    Output {
        generation: u64,
        data: Vec<u8>,
    },
    Eof {
        generation: u64,
    },
}
struct Pty {
    // Close the process tree before releasing the PTY's handles.
    _processes: platform::terminal::Processes,
    master: Box<dyn MasterPty + Send>,
    input: mpsc::SyncSender<Vec<u8>>,
    child: Box<dyn Child + Send + Sync>,
}
impl Pty {
    fn open(
        worktree: &Path,
        size: PtySize,
        command: Option<&super::TerminalCommand>,
        generation: u64,
        events: mpsc::SyncSender<Event>,
    ) -> io::Result<Self> {
        let pair = portable_pty::native_pty_system()
            .openpty(size)
            .map_err(io::Error::other)?;
        let mut launch = CommandBuilder::new_default_prog();
        launch.cwd(worktree);
        launch.env_clear();
        for (name, value) in platform::environment::for_command() {
            launch.env(name, value);
        }
        launch.env("TERM", "xterm-256color");
        let confirmed = command
            .map(|command| super::restore::input(&mut launch, command, worktree))
            .transpose()?;
        let mut child = pair.slave.spawn_command(launch).map_err(io::Error::other)?;
        let processes = match platform::terminal::Processes::new(&*child) {
            Ok(processes) => processes,
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error);
            }
        };
        drop(pair.slave);
        let mut writer = pair.master.take_writer().map_err(io::Error::other)?;
        let (input, pending) = mpsc::sync_channel::<Vec<u8>>(CLIENT_QUEUE);
        std::thread::spawn(move || {
            while let Ok(data) = pending.recv() {
                if writer
                    .write_all(&data)
                    .and_then(|()| writer.flush())
                    .is_err()
                {
                    break;
                }
            }
        });
        if let Some(confirmed) = confirmed {
            input.send(confirmed).map_err(io::Error::other)?;
        }
        let mut reader = pair.master.try_clone_reader().map_err(io::Error::other)?;
        std::thread::spawn(move || {
            let mut buffer = [0u8; OUTPUT_CHUNK];
            loop {
                match reader.read(&mut buffer) {
                    Ok(0) | Err(_) => {
                        let _ = events.send(Event::Eof { generation });
                        break;
                    }
                    Ok(count) => {
                        if events
                            .send(Event::Output {
                                generation,
                                data: buffer[..count].to_vec(),
                            })
                            .is_err()
                        {
                            break;
                        }
                    }
                }
            }
        });
        Ok(Self {
            _processes: processes,
            master: pair.master,
            input,
            child,
        })
    }
}
impl Drop for Pty {
    fn drop(&mut self) {
        // The platform owner kills descendants as well; kill/wait reaps the direct child.
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Hidden CLI entry point. A losing election exits without touching the winning host.
pub async fn serve(worktree: PathBuf, cols: u16, rows: u16) -> Result<(), String> {
    let worktree = platform::canonicalize(&worktree).map_err(|e| e.to_string())?;
    let identity = crate::repository::worktree_id(&worktree)?;
    let endpoint = Endpoint::namespaced(&identity, "terminal").map_err(|e| e.to_string())?;
    let Some(election) = endpoint.elect().map_err(|e| e.to_string())? else {
        return Ok(());
    };
    let size = dimensions(cols, rows).map_err(|e| e.to_string())?;
    let mut listener = endpoint.listen().map_err(|e| e.to_string())?;
    let (events, incoming) = mpsc::sync_channel(CLIENT_QUEUE);
    let sender = events.clone();
    let mut actor = tokio::task::spawn_blocking(move || {
        let _election = election;
        Actor::new(worktree, identity, size, sender)?.run(incoming)
    });
    let mut next = 0;
    let mut connections = tokio::task::JoinSet::new();
    loop {
        tokio::select! {
            result = &mut actor => {
                // Let already queued output and the final ended frame reach connected clients.
                // A client that stopped reading cannot hold a finished host alive forever.
                const DRAIN_GRACE: Duration = Duration::from_secs(1);
                let _ = tokio::time::timeout(DRAIN_GRACE, async {
                    while connections.join_next().await.is_some() {}
                }).await;
                return crate::task::joined(result).map_err(|e| e.to_string())?.map_err(|e| e.to_string());
            }
            _ = connections.join_next(), if !connections.is_empty() => {},
            stream = listener.accept() => {
                let stream = stream.map_err(|e| e.to_string())?;
                if validate_peer(&stream).is_err() { continue; }
                next += 1;
                connections.spawn(connection(stream, endpoint.identity.clone(), next, events.clone()));
            }
        }
    }
}

async fn post(events: &mpsc::SyncSender<Event>, event: Event) -> io::Result<()> {
    let events = events.clone();
    crate::task::joined(tokio::task::spawn_blocking(move || events.send(event)).await)
        .map_err(io::Error::other)?
        .map_err(io::Error::other)
}
async fn connection(
    stream: Stream,
    identity: crate::config::EndpointId,
    id: u64,
    events: mpsc::SyncSender<Event>,
) {
    let _ = communicate(stream, identity, id, &events).await;
    let _ = post(&events, Event::Detach(id)).await;
}
async fn communicate(
    stream: Stream,
    identity: crate::config::EndpointId,
    id: u64,
    events: &mpsc::SyncSender<Event>,
) -> io::Result<()> {
    let (read, mut write) = tokio::io::split(stream);
    let mut read = BufReader::new(read).lines();
    let Some(Request::Attach {
        identity: requested,
        cols,
        rows,
    }) = wire::receive(&mut read).await?
    else {
        return Ok(());
    };
    if requested != identity {
        return Err(io::ErrorKind::PermissionDenied.into());
    }
    let (sender, mut frames) = async_mpsc::channel(CLIENT_QUEUE);
    post(
        events,
        Event::Attach {
            id,
            cols,
            rows,
            sender,
        },
    )
    .await?;
    loop {
        tokio::select! {
            response = frames.recv() => match response {
                Some(response) => wire::send(&mut write, &response).await?,
                None => return Ok(()),
            },
            request = wire::receive(&mut read) => match request? {
                Some(request) => post(events, Event::Request { id, request }).await?,
                None => return Ok(()),
            },
        }
    }
}

struct Actor {
    worktree: PathBuf,
    identity: PathBuf,
    events: mpsc::SyncSender<Event>,
    pty: Option<Pty>,
    generation: u64,
    size: PtySize,
    screen: Screen,
    clients: BTreeMap<u64, async_mpsc::Sender<Response>>,
    metadata: Metadata,
    offer: Option<RestoreOffer>,
    moved: Option<PathBuf>,
    persistent: bool,
}
impl Actor {
    fn new(
        worktree: PathBuf,
        identity: PathBuf,
        size: PtySize,
        events: mpsc::SyncSender<Event>,
    ) -> io::Result<Self> {
        let saved = Metadata::read(&identity)?;
        let offer = saved.as_ref().map(Metadata::offer);
        let metadata = saved.unwrap_or_else(|| Metadata {
            cwd: Some(worktree.clone()),
            ..Metadata::default()
        });
        metadata.save(&identity)?;
        let pty = Pty::open(&worktree, size, None, 0, events.clone())?;
        Ok(Self {
            worktree,
            identity,
            events,
            pty: Some(pty),
            generation: 0,
            size,
            screen: Screen::new(size.cols, size.rows),
            clients: BTreeMap::new(),
            metadata,
            offer,
            moved: None,
            persistent: platform::environment::var("ARTIFACTIZE_TERMINAL_NOT_PERSISTENT").is_none(),
        })
    }
    fn emit(&mut self, frame: TerminalFrame) {
        self.clients
            .retain(|_, sender| sender.try_send(Response::Frame(frame.clone())).is_ok());
    }
    fn reply(&mut self, id: u64, response: Response) {
        if self
            .clients
            .get(&id)
            .is_some_and(|sender| sender.try_send(response).is_err())
        {
            self.clients.remove(&id);
        }
    }
    fn resize(&mut self, cols: u16, rows: u16) -> io::Result<()> {
        let size = dimensions(cols, rows)?;
        if let Some(pty) = &self.pty {
            pty.master.resize(size).map_err(io::Error::other)?;
        }
        self.size = size;
        self.screen.resize(cols, rows);
        Ok(())
    }
    fn run(mut self, incoming: mpsc::Receiver<Event>) -> io::Result<()> {
        let mut observed = std::time::Instant::now();
        loop {
            match incoming.recv_timeout(OBSERVE_INTERVAL.saturating_sub(observed.elapsed())) {
                Ok(Event::Attach {
                    id,
                    cols,
                    rows,
                    sender,
                }) => {
                    self.clients.insert(id, sender);
                    let resized = self.size.cols != cols || self.size.rows != rows;
                    match self.resize(cols, rows) {
                        Ok(()) => {
                            if resized {
                                self.emit(self.screen.snapshot());
                            } else {
                                self.reply(id, Response::Frame(self.screen.snapshot()));
                            }
                            if !self.persistent {
                                self.reply(id, Response::Frame(TerminalFrame::NotPersistent));
                            }
                            if let Some(offer) = self.offer.clone() {
                                self.reply(
                                    id,
                                    Response::Frame(TerminalFrame::RestoreOffer { offer }),
                                );
                            }
                            if let Some(worktree) = self.moved.clone() {
                                self.reply(
                                    id,
                                    Response::Frame(TerminalFrame::AgentMoved { worktree }),
                                );
                            }
                            self.reply(id, Response::Accepted);
                        }
                        Err(error) => self.reply(id, Response::Failed(error.to_string())),
                    }
                }
                Ok(Event::Request { id, request }) => {
                    let detached = matches!(request, Request::Detach);
                    let result = self.request(request);
                    self.reply(
                        id,
                        match result {
                            Ok(()) => Response::Accepted,
                            Err(e) => Response::Failed(e.to_string()),
                        },
                    );
                    if detached {
                        self.clients.remove(&id);
                    }
                }
                Ok(Event::Detach(id)) => {
                    self.clients.remove(&id);
                }
                Ok(Event::Output { generation, data }) if generation == self.generation => {
                    self.screen.process(&data);
                    self.emit(TerminalFrame::Output { data });
                }
                Ok(Event::Eof { generation }) if generation == self.generation => {
                    let exit_code = self
                        .pty
                        .as_mut()
                        .and_then(|pty| pty.child.try_wait().ok().flatten())
                        .map(|exit| exit.exit_code());
                    self.emit(TerminalFrame::Ended { exit_code });
                    return Ok(());
                }
                Ok(_) | Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => return Ok(()),
            }
            if observed.elapsed() >= OBSERVE_INTERVAL {
                if !self.worktree.exists() || !self.identity.exists() {
                    self.emit(TerminalFrame::Ended { exit_code: None });
                    return Ok(());
                }
                self.observe()?;
                observed = std::time::Instant::now();
            }
        }
    }
    fn observe(&mut self) -> io::Result<()> {
        let previous = self.metadata.clone();
        if let Some(cwd) = self.screen.take_cwd() {
            self.metadata.cwd = Some(cwd);
        }
        let agent = self
            .pty
            .as_ref()
            .and_then(|pty| platform::terminal::foreground(&*pty.master, pty.child.process_id()))
            .is_some_and(|foreground| self.metadata.observe(foreground));
        if previous.cwd != self.metadata.cwd || previous.command != self.metadata.command {
            self.metadata.certify();
        }
        if previous != self.metadata {
            self.metadata.save(&self.identity)?;
        }
        if agent && previous.cwd != self.metadata.cwd {
            let detected = self
                .metadata
                .cwd
                .as_deref()
                .and_then(|cwd| crate::repository::identify(cwd).worktree_path)
                .filter(|path| path != &self.worktree);
            if detected != self.moved {
                self.moved = detected;
                if let Some(worktree) = self.moved.clone() {
                    self.emit(TerminalFrame::AgentMoved { worktree });
                }
            }
        }
        Ok(())
    }
    fn request(&mut self, request: Request) -> io::Result<()> {
        match request {
            Request::Input { data } => {
                let pty = self
                    .pty
                    .as_mut()
                    .ok_or_else(|| io::Error::other("Terminal ended"))?;
                pty.input.try_send(data).map_err(io::Error::other)
            }
            Request::Resize { cols, rows } => {
                if self.size.cols != cols || self.size.rows != rows {
                    self.resize(cols, rows)?;
                    self.emit(self.screen.snapshot());
                }
                Ok(())
            }
            Request::Restore { choice } => {
                let offer = self
                    .offer
                    .as_ref()
                    .ok_or_else(|| io::Error::other("No restore offer remains"))?;
                let command = match choice {
                    RestoreChoice::FreshShell => {
                        self.offer = None;
                        return Ok(());
                    }
                    RestoreChoice::Rerun => offer.command.clone(),
                    RestoreChoice::Resume => offer.resume.clone(),
                }
                .ok_or_else(|| io::Error::other("That restore command is unknown"))?;
                let cwd = offer.cwd.clone().unwrap_or_else(|| self.worktree.clone());
                // A chosen offer is used up before it starts: a launch that fails after
                // spawning must not leave the same command available to run again.
                self.offer = None;
                self.replace(Some(&command), cwd)
            }
            Request::Detach => Ok(()),
            Request::Attach { .. } => Err(io::Error::other("Already attached")),
        }
    }
    fn replace(
        &mut self,
        command: Option<&super::TerminalCommand>,
        cwd: PathBuf,
    ) -> io::Result<()> {
        self.generation += 1;
        self.pty.take();
        let opened = Pty::open(
            &cwd,
            self.size,
            command,
            self.generation,
            self.events.clone(),
        );
        let error = match opened {
            Ok(pty) => {
                self.pty = Some(pty);
                None
            }
            Err(error) => {
                self.pty = Some(Pty::open(
                    &self.worktree,
                    self.size,
                    None,
                    self.generation,
                    self.events.clone(),
                )?);
                Some(error)
            }
        };
        self.emit(self.screen.snapshot());
        error.map_or(Ok(()), Err)
    }
}
