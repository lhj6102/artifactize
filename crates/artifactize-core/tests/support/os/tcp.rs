//! TCP termination observations for fixtures whose owner has already completed.

use std::io;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt, BufReader},
    net::TcpStream,
};

/// Probe a peer after its owning operation has completed, preserving every reply byte.
/// EOF and a peer reset both terminate TCP; neither is evidence of process reaping by
/// itself. The caller separately checks the owner outcome and rejects any reply bytes.
/// A broken/reset send is a closed write side, but the buffered read is still drained.
/// Every other send/read error remains an error, never a successful observation.
pub async fn closed_probe(stream: &mut BufReader<TcpStream>, probe: &[u8]) -> io::Result<Vec<u8>> {
    match stream.get_mut().write_all(probe).await {
        Ok(()) => {}
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::BrokenPipe | io::ErrorKind::ConnectionReset
            ) => {}
        Err(error) => return Err(error),
    }
    let mut response = Vec::new();
    match stream.read_to_end(&mut response).await {
        Ok(_) => Ok(response),
        Err(error) if error.kind() == io::ErrorKind::ConnectionReset => Ok(response),
        Err(error) => Err(error),
    }
}
