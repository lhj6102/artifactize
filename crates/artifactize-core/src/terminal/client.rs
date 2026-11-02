//! Shared attachment client for desktop, CLI and TUI. Dropping it only closes the socket.
use super::{
    RestoreChoice, TerminalFrame, dimensions,
    wire::{self, Request, Response},
};
use crate::local_transport::{Endpoint, Stream};
use std::{io, path::Path, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    sync::{mpsc, oneshot},
};

/// Host startup includes loading the CLI and opening a native PTY.
const START_TIMEOUT: Duration = Duration::from_secs(15);
/// Retry only during startup; connected terminals use a continuous ordered stream.
const START_RETRY: Duration = Duration::from_millis(50);
/// Frames and requests in flight per attachment, matching the host's per-client queue: a
/// client that stops reading holds at most this many output chunks before the host drops it.
const CLIENT_QUEUE: usize = 256;

type Pending = (Request, oneshot::Sender<Result<(), String>>);
#[derive(Clone)]
pub struct Attachment {
    commands: mpsc::Sender<Pending>,
}
impl Attachment {
    async fn request(&self, request: Request) -> Result<(), String> {
        let (answer, result) = oneshot::channel();
        self.commands
            .send((request, answer))
            .await
            .map_err(|_| "Terminal detached".to_owned())?;
        result
            .await
            .map_err(|_| "Terminal disconnected".to_owned())?
    }
    pub async fn input(&self, data: Vec<u8>) -> Result<(), String> {
        self.request(Request::Input { data }).await
    }
    pub async fn resize(&self, cols: u16, rows: u16) -> Result<(), String> {
        self.request(Request::Resize { cols, rows }).await
    }
    pub async fn restore(&self, choice: RestoreChoice) -> Result<(), String> {
        self.request(Request::Restore { choice }).await
    }
    pub async fn detach(&self) -> Result<(), String> {
        self.request(Request::Detach).await
    }
}

pub async fn attach(
    worktree: &Path,
    cols: u16,
    rows: u16,
) -> Result<(Attachment, mpsc::Receiver<TerminalFrame>), String> {
    dimensions(cols, rows).map_err(|e| e.to_string())?;
    let identity = crate::repository::worktree_id(worktree)?;
    let endpoint = Endpoint::namespaced(&identity, "terminal").map_err(|e| e.to_string())?;
    let (stream, persistent) = match endpoint.connect().await {
        Ok(stream) => (stream, true),
        Err(error) if error.kind() == io::ErrorKind::PermissionDenied => {
            return Err(error.to_string());
        }
        Err(_) => {
            let persistent = crate::process::launch_terminal(worktree, cols, rows)?;
            let stream = tokio::time::timeout(START_TIMEOUT, async {
                loop {
                    match endpoint.connect().await {
                        Ok(stream) => return Ok(stream),
                        Err(error) if error.kind() == io::ErrorKind::PermissionDenied => {
                            return Err(error);
                        }
                        Err(_) => tokio::time::sleep(START_RETRY).await,
                    }
                }
            })
            .await
            .map_err(|_| "Terminal host did not start".to_owned())?
            .map_err(|e| e.to_string())?;
            (stream, persistent)
        }
    };
    let (commands, incoming) = mpsc::channel(CLIENT_QUEUE);
    let (frames, output) = mpsc::channel(CLIENT_QUEUE);
    let (ready, started) = oneshot::channel();
    tokio::spawn(async move {
        let result = connection(
            stream,
            Request::Attach {
                identity: endpoint.identity,
                cols,
                rows,
            },
            incoming,
            persistent,
            &frames,
            ready,
        )
        .await;
        if result.is_err() {
            let _ = frames.send(TerminalFrame::Ended { exit_code: None }).await;
        }
    });
    started
        .await
        .map_err(|_| "Terminal host disconnected".to_owned())??;
    Ok((Attachment { commands }, output))
}

async fn connection(
    stream: Stream,
    attach: Request,
    mut commands: mpsc::Receiver<Pending>,
    persistent: bool,
    frames: &mpsc::Sender<TerminalFrame>,
    ready: oneshot::Sender<Result<(), String>>,
) -> io::Result<()> {
    let (read, mut write) = tokio::io::split(stream);
    let mut read = BufReader::new(read).lines();
    wire::send(&mut write, &attach).await?;
    let mut warned = false;
    let result = acknowledge(&mut read, frames, &mut warned).await;
    let success = result.is_ok();
    if success && !persistent && !warned {
        frames
            .send(TerminalFrame::NotPersistent)
            .await
            .map_err(io::Error::other)?;
    }
    let _ = ready.send(result.map_err(|e| e.to_string()));
    if !success {
        return Ok(());
    }
    loop {
        tokio::select! {
            command = commands.recv() => {
                let Some((request, answer)) = command else { return Ok(()); };
                let detach = matches!(request, Request::Detach);
                wire::send(&mut write, &request).await?;
                let result = acknowledge(&mut read, frames, &mut warned).await;
                let disconnected = result.as_ref().is_err_and(|error| error.kind() != io::ErrorKind::Other);
                let _ = answer.send(result.map_err(|e| e.to_string()));
                if detach || disconnected { return Ok(()); }
            }
            response = wire::receive::<Response>(&mut read) => match response? {
                Some(Response::Frame(frame)) => {
                    let ended = matches!(frame, TerminalFrame::Ended { .. });
                    frames.send(frame).await.map_err(io::Error::other)?;
                    if ended { return Ok(()); }
                }
                Some(_) => return Err(io::Error::other("Unexpected terminal response")),
                None => return Err(io::ErrorKind::UnexpectedEof.into()),
            },
        }
    }
}
async fn acknowledge(
    reader: &mut tokio::io::Lines<impl tokio::io::AsyncBufRead + Unpin>,
    frames: &mpsc::Sender<TerminalFrame>,
    warned: &mut bool,
) -> io::Result<()> {
    loop {
        match wire::receive(reader).await? {
            Some(Response::Accepted) => return Ok(()),
            Some(Response::Failed(error)) => return Err(io::Error::other(error)),
            Some(Response::Frame(frame)) => {
                *warned |= matches!(frame, TerminalFrame::NotPersistent);
                frames.send(frame).await.map_err(io::Error::other)?;
            }
            None => return Err(io::ErrorKind::UnexpectedEof.into()),
        }
    }
}
