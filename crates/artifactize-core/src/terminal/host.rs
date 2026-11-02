//! The host serializes PTY state and attachments in one actor. Slow clients detach instead
//! of dropping output; their next attach receives a complete screen snapshot.
use super::{
    RestoreChoice, RestoreOffer, TerminalFrame, dimensions,
    metadata::Metadata,
    screen::Screen,
    wire::{self, Request, Response},
};
use crate::platform::terminal::{Output, Pty, Size};
use crate::{
    local_transport::{Endpoint, Stream, validate_peer},
    platform,
};
use std::{collections::BTreeMap, io, path::PathBuf, sync::mpsc, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    sync::mpsc as async_mpsc,
};

/// Bounded queues keep a hidden or stalled desktop from consuming the host's memory.
const CLIENT_QUEUE: usize = 256;
/// Worktree deletion and foreground state are sampled even with no attached desktop.
const OBSERVE_INTERVAL: Duration = Duration::from_secs(1);

/// One accepted connection of this host, numbered in accept order and never reused.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct ClientId(u64);
impl ClientId {
    /// Take this identity for a new connection and move on to the next one.
    fn advance(&mut self) -> Self {
        self.0 += 1;
        *self
    }
}

enum Event {
    Shutdown {
        completed: tokio::sync::oneshot::Sender<()>,
    },
    Attach {
        id: ClientId,
        cols: u16,
        rows: u16,
        sender: async_mpsc::Sender<Response>,
    },
    Request {
        id: ClientId,
        request: Request,
    },
    Detach(ClientId),
    Output(Vec<u8>),
    Eof,
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
    // The lock covers the actor's cleanup and the final connection drain.
    let _election = election;
    let mut actor = tokio::task::spawn_blocking(move || {
        Actor::new(worktree, identity, size, sender)?.run(incoming)
    });
    let mut next = ClientId::default();
    let mut connections = tokio::task::JoinSet::new();
    loop {
        tokio::select! {
            result = &mut actor => {
                // Let already queued output and the final ended frame reach connected clients.
                // A client that stopped reading cannot hold a finished host alive forever.
                const DRAIN_GRACE: Duration = Duration::from_secs(1);
                // A connection that panicked is a bug: its panic resumes here.
                let drained = tokio::time::timeout(DRAIN_GRACE, async {
                    while let Some(joined) = connections.join_next().await {
                        crate::task::joined(joined)?;
                    }
                    Ok::<(), tokio::task::JoinError>(())
                }).await;
                if let Ok(drained) = drained {
                    drained.map_err(|e| e.to_string())?;
                }
                return crate::task::joined(result).map_err(|e| e.to_string())?.map_err(|e| e.to_string());
            }
            Some(joined) = connections.join_next(), if !connections.is_empty() => {
                crate::task::joined(joined).map_err(|e| e.to_string())?;
            },
            stream = listener.accept() => {
                let stream = stream.map_err(|e| e.to_string())?;
                if validate_peer(&stream).is_err() { continue; }
                connections.spawn(connection(stream, endpoint.identity.clone(), next.advance(), events.clone()));
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
    id: ClientId,
    events: mpsc::SyncSender<Event>,
) {
    let _ = communicate(stream, identity, id, &events).await;
    let _ = post(&events, Event::Detach(id)).await;
}
async fn communicate(
    stream: Stream,
    identity: crate::config::EndpointId,
    id: ClientId,
    events: &mpsc::SyncSender<Event>,
) -> io::Result<()> {
    let (read, mut write) = tokio::io::split(stream);
    let mut read = BufReader::new(read).lines();
    let (cols, rows) = match wire::receive(&mut read).await? {
        Some(Request::Attach {
            identity: requested,
            cols,
            rows,
        }) if requested == identity => (cols, rows),
        Some(Request::Shutdown {
            identity: requested,
        }) if requested == identity => {
            let (completed, stopped) = tokio::sync::oneshot::channel();
            let host = platform::terminal::host_identity()?;
            post(events, Event::Shutdown { completed }).await?;
            stopped.await.map_err(io::Error::other)?;
            wire::send(&mut write, &Response::Stopped { host }).await?;
            return Ok(());
        }
        _ => return Err(io::ErrorKind::PermissionDenied.into()),
    };
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

enum Phase {
    Shell,
    Restored { cwd: PathBuf },
}

struct Actor {
    worktree: PathBuf,
    identity: PathBuf,
    pty: Pty,
    phase: Phase,
    size: Size,
    screen: Screen,
    clients: BTreeMap<ClientId, async_mpsc::Sender<Response>>,
    metadata: Metadata,
    offer: Option<RestoreOffer>,
    moved: Option<PathBuf>,
    persistent: bool,
}
impl Actor {
    fn new(
        worktree: PathBuf,
        identity: PathBuf,
        size: Size,
        events: mpsc::SyncSender<Event>,
    ) -> io::Result<Self> {
        let saved = Metadata::read(&identity)?;
        let offer = saved.as_ref().map(Metadata::offer);
        let metadata = saved.unwrap_or_else(|| Metadata {
            cwd: Some(worktree.clone()),
            ..Metadata::default()
        });
        metadata.save(&identity)?;
        let pty = Pty::open(&worktree, size, move |output| {
            let event = match output {
                Output::Data(data) => Event::Output(data),
                Output::Closed => Event::Eof,
            };
            let _ = events.send(event);
        })?;
        Ok(Self {
            worktree,
            identity,
            pty,
            phase: Phase::Shell,
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
    fn reply(&mut self, id: ClientId, response: Response) {
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
        self.pty.resize(size)?;
        self.size = size;
        self.screen.resize(cols, rows);
        Ok(())
    }
    fn run(mut self, incoming: mpsc::Receiver<Event>) -> io::Result<()> {
        let mut observed = std::time::Instant::now();
        loop {
            match incoming.recv_timeout(OBSERVE_INTERVAL.saturating_sub(observed.elapsed())) {
                Ok(Event::Shutdown { completed }) => {
                    self.pty.stop()?;
                    self.emit(TerminalFrame::Ended { exit_code: None });
                    let _ = completed.send(());
                    return Ok(());
                }
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
                Ok(Event::Output(data)) => {
                    self.screen.process(&data);
                    self.emit(TerminalFrame::Output { data });
                }
                Ok(Event::Eof) => {
                    self.emit(TerminalFrame::Ended { exit_code: None });
                    return Ok(());
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => return Ok(()),
            }
            if let Some(exit_code) = self.pty.try_wait()? {
                match &self.phase {
                    Phase::Restored { cwd } => {
                        self.pty.run(cwd, None)?;
                        self.phase = Phase::Shell;
                    }
                    Phase::Shell => {
                        self.emit(TerminalFrame::Ended {
                            exit_code: Some(exit_code),
                        });
                        return Ok(());
                    }
                }
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
            .foreground()
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
            Request::Input { data } => self.pty.input(data),
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
                if matches!(choice, RestoreChoice::FreshShell) {
                    self.offer = None;
                    return Ok(());
                }
                let (cwd, command) = offer.confirmed(choice).map_err(io::Error::other)?;
                // A chosen offer is used up before it starts: a launch that fails after
                // spawning must not leave the same command available to run again.
                self.offer = None;
                match self.pty.run(&cwd, Some(&command)) {
                    Ok(()) => {
                        self.phase = Phase::Restored { cwd };
                        Ok(())
                    }
                    Err(error) => {
                        self.pty.run(&self.worktree, None)?;
                        self.phase = Phase::Shell;
                        Err(error)
                    }
                }
            }
            Request::Detach => Ok(()),
            Request::Attach { .. } | Request::Shutdown { .. } => {
                Err(io::Error::other("Already attached"))
            }
        }
    }
}
