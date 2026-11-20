//! Local subprocess endpoints and TCP termination observations.

use std::io;
use tokio::{
    io::{AsyncReadExt, BufReader},
    net::TcpStream,
};

/// Probe a peer after its owning operation has completed, preserving every reply byte.
/// EOF and a peer reset both terminate TCP; neither is evidence of process reaping by
/// itself. The caller separately checks the owner outcome and rejects any reply bytes.
/// Any error other than a peer reset remains an error.
pub async fn closed_probe(stream: &mut BufReader<TcpStream>) -> io::Result<Vec<u8>> {
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

/// A tool process announces its PID after optional JSON input and waits for its owner.
pub fn tool_ready_script(address: std::net::SocketAddr, read_input: bool) -> String {
    let input = if read_input {
        "json.load(sys.stdin)\n"
    } else {
        ""
    };
    let (ip, port) = (address.ip(), address.port());
    format!(
        "import json,os,socket,sys,threading\n{input}s = socket.create_connection(('{ip}', {port}))\ns.sendall(str(os.getpid()).encode())\ns.shutdown(socket.SHUT_WR)\nthreading.Event().wait()"
    )
}

/// Python's fixture-side half of the acknowledged IPC marker protocol.
pub const PYTHON_MARKERS: &str = r#"import socket, sys
host, port = sys.argv[2:4]
def record(kind):
    with socket.create_connection((host, int(port))) as stream:
        stream.sendall((kind + ' ' + sys.argv[1] + '\n').encode())
        with stream.makefile('rb') as response:
            assert response.readline() == b'ok\n'
"#;

/// A build-script callback that reports its identity and waits for the fixture owner.
pub fn cargo_build_callback(address: std::net::SocketAddr) -> String {
    let path_text = super::RUST_PATH_TEXT;
    format!(
        r#"use std::io::{{Read, Write}};
{path_text}
fn main() {{
    let mut stream = std::net::TcpStream::connect("{address}").unwrap();
    let out = std::env::var_os("OUT_DIR").unwrap();
    writeln!(stream, "{{}}\t{{}}", std::process::id(), path_text(std::path::Path::new(&out))).unwrap();
    let mut probe = [0];
    if stream.read_exact(&mut probe).is_ok() {{ stream.write_all(b"alive").unwrap(); }}
}}
"#
    )
}
