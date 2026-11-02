//! Stop an existing host without ever starting one. Holding its election lock after shutdown
//! prevents another opener from racing Git's worktree removal.
use super::wire::{self, Request, Response};
use crate::{local_transport::Endpoint, platform};
use std::{fs::File, io, path::Path, time::Duration};
use tokio::io::{AsyncBufReadExt, BufReader};

/// Allow process-tree cleanup and a final frame drain, but never remove a still-owned tree.
const STOP_TIMEOUT: Duration = Duration::from_secs(45);
/// Poll the election only while shutdown completes, not during normal terminal operation.
const ELECTION_RETRY: Duration = Duration::from_millis(25);

/// End a worktree's terminal process tree and wait for its host to exit. A worktree with no
/// running host succeeds without starting one. This does not remove the worktree or branch.
pub async fn stop(worktree: &Path) -> io::Result<()> {
    drop(stop_for_removal(worktree).await?);
    Ok(())
}

pub(crate) async fn stop_for_removal(worktree: &Path) -> io::Result<File> {
    let identity = crate::repository::worktree_id(worktree).map_err(io::Error::other)?;
    let endpoint = Endpoint::namespaced(&identity, "terminal")?;
    tokio::time::timeout(STOP_TIMEOUT, async {
        let host = loop {
            if let Some(election) = endpoint.elect()? {
                return Ok(election);
            }
            match endpoint.connect().await {
                Ok(stream) => {
                    let (reader, mut writer) = tokio::io::split(stream);
                    let mut reader = BufReader::new(reader).lines();
                    wire::send(
                        &mut writer,
                        &Request::Shutdown {
                            identity: endpoint.identity.clone(),
                        },
                    )
                    .await?;
                    match wire::receive(&mut reader).await? {
                        Some(Response::Stopped { host }) => break host,
                        _ => return Err(io::Error::other("Terminal host did not finish shutdown")),
                    }
                }
                Err(error) if error.kind() == io::ErrorKind::PermissionDenied => return Err(error),
                Err(_) => tokio::time::sleep(ELECTION_RETRY).await,
            }
        };
        let election = loop {
            if let Some(election) = endpoint.elect()? {
                break election;
            }
            tokio::time::sleep(ELECTION_RETRY).await;
        };
        crate::task::joined(
            tokio::task::spawn_blocking(move || platform::terminal::wait_host(host)).await,
        )
        .map_err(io::Error::other)??;
        Ok(election)
    })
    .await
    .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "Terminal host is still stopping"))?
}
