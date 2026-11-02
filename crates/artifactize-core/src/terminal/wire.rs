//! Ordered, private terminal connections; no coalescing and no changes-hub messages.
use super::{RestoreChoice, TerminalFrame};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::io;
use tokio::io::{AsyncBufRead, AsyncWrite, AsyncWriteExt};

#[derive(Serialize, Deserialize)]
pub(super) enum Request {
    Shutdown {
        identity: crate::config::EndpointId,
    },
    Attach {
        identity: crate::config::EndpointId,
        cols: u16,
        rows: u16,
    },
    Input {
        data: Vec<u8>,
    },
    Resize {
        cols: u16,
        rows: u16,
    },
    Restore {
        choice: RestoreChoice,
    },
    Detach,
}

#[derive(Serialize, Deserialize)]
pub(super) enum Response {
    Frame(TerminalFrame),
    Accepted,
    Stopped { host: crate::process::ChildIdentity },
    Failed(String),
}

pub(super) async fn send<T: Serialize>(
    writer: &mut (impl AsyncWrite + Unpin),
    value: &T,
) -> io::Result<()> {
    let mut bytes = serde_json::to_vec(value)?;
    bytes.push(b'\n');
    writer.write_all(&bytes).await
}
pub(super) async fn receive<T: DeserializeOwned>(
    reader: &mut tokio::io::Lines<impl AsyncBufRead + Unpin>,
) -> io::Result<Option<T>> {
    let Some(line) = reader.next_line().await? else {
        return Ok(None);
    };
    serde_json::from_str(&line)
        .map(Some)
        .map_err(io::Error::other)
}
