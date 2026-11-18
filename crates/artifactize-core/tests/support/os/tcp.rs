//! Local subprocess endpoints and TCP termination observations.

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

/// Reserve a private port on the local IPv4 loopback interface for the fixture's lifetime.
pub fn loopback_listener() -> std::net::TcpListener {
    std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap()
}

pub fn nonblocking_loopback_listener() -> std::net::TcpListener {
    let listener = loopback_listener();
    listener.set_nonblocking(true).unwrap();
    listener
}

pub fn async_loopback_listener() -> tokio::net::TcpListener {
    tokio::net::TcpListener::from_std(nonblocking_loopback_listener()).unwrap()
}

/// A Python subprocess optionally sends its label, then blocks until fixture release.
pub fn socket_gate_args(address: std::net::SocketAddr, label: Option<&str>) -> Vec<String> {
    [
        "-c".into(),
        "import socket,sys\nwith socket.create_connection((sys.argv[1],int(sys.argv[2]))) as stream:\n    if len(sys.argv) > 3: stream.sendall(sys.argv[3].encode())\n    assert stream.recv(1) == b'x'".into(),
        address.ip().to_string(),
        address.port().to_string(),
    ]
    .into_iter()
    .chain(label.map(str::to_owned))
    .collect()
}
