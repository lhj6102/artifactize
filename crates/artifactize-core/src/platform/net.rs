//! Listener options shared across operating systems.
use std::{io, net::SocketAddr};

use socket2::{Domain, Protocol, Socket, Type};
use tokio::net::TcpListener;

/// Open a timed connection to an IPC peer on the local machine.
pub(crate) fn connect_loopback(
    port: u16,
    timeout: std::time::Duration,
) -> io::Result<std::net::TcpStream> {
    std::net::TcpStream::connect_timeout(
        &SocketAddr::from((std::net::Ipv4Addr::LOCALHOST, port)),
        timeout,
    )
}

/// Bind without Unix's SO_REUSEADDR, and with Windows's SO_EXCLUSIVEADDRUSE.
/// IPv6 is v6-only so the two loopback addresses can be reserved independently.
pub(crate) fn exclusive_listener(address: SocketAddr) -> io::Result<TcpListener> {
    // Only a browser's short callback connects here; no large accept queue is needed.
    const CALLBACK_BACKLOG: i32 = 8;
    listen(address, Ipv6::Only, CALLBACK_BACKLOG)
}

/// The HTTP server owns its address on every OS and can serve IPv4 on an IPv6 wildcard.
pub(crate) fn http_listener(address: SocketAddr) -> io::Result<TcpListener> {
    // Keep Tokio's normal server accept queue size for bursts of requests.
    const HTTP_BACKLOG: i32 = 1024;
    listen(address, Ipv6::DualStack, HTTP_BACKLOG)
}

enum Ipv6 {
    Only,
    DualStack,
}

fn listen(address: SocketAddr, ipv6: Ipv6, backlog: i32) -> io::Result<TcpListener> {
    let socket = Socket::new(
        Domain::for_address(address),
        Type::STREAM,
        Some(Protocol::TCP),
    )?;
    if address.is_ipv6() {
        socket.set_only_v6(matches!(ipv6, Ipv6::Only))?;
    }
    #[cfg(windows)]
    exclusive(&socket)?;
    socket.bind(&address.into())?;
    socket.listen(backlog)?;
    socket.set_nonblocking(true)?;
    TcpListener::from_std(socket.into())
}

pub(crate) fn address_in_use(error: &io::Error) -> bool {
    if error.kind() == io::ErrorKind::AddrInUse {
        return true;
    }
    // Winsock reports an exclusive bind collision as WSAEACCES as well as WSAEADDRINUSE.
    #[cfg(windows)]
    if error.raw_os_error() == Some(windows_sys::Win32::Networking::WinSock::WSAEACCES) {
        return true;
    }
    false
}

/// An IPv4-only host need not reserve an address it cannot serve.
pub(crate) fn ipv6_unavailable(error: &io::Error) -> bool {
    if error.kind() == io::ErrorKind::AddrNotAvailable {
        return true;
    }
    #[cfg(unix)]
    let unsupported = [libc::EAFNOSUPPORT, libc::EPROTONOSUPPORT];
    #[cfg(windows)]
    let unsupported = {
        use windows_sys::Win32::Networking::WinSock::{WSAEAFNOSUPPORT, WSAEPROTONOSUPPORT};
        [WSAEAFNOSUPPORT, WSAEPROTONOSUPPORT]
    };
    error
        .raw_os_error()
        .is_some_and(|code| unsupported.contains(&code))
}

#[cfg(windows)]
fn exclusive(socket: &Socket) -> io::Result<()> {
    use std::os::windows::io::AsRawSocket;
    use windows_sys::Win32::Networking::WinSock::{
        SO_EXCLUSIVEADDRUSE, SOCKET_ERROR, SOL_SOCKET, WSAGetLastError, setsockopt,
    };
    let enabled: i32 = 1;
    // SAFETY: the socket is live; the pointer and length describe an initialized BOOL
    // for the duration of this synchronous Winsock call, which does not retain it.
    let result = unsafe {
        setsockopt(
            socket.as_raw_socket() as _,
            SOL_SOCKET,
            SO_EXCLUSIVEADDRUSE,
            (&enabled as *const i32).cast(),
            std::mem::size_of_val(&enabled) as i32,
        )
    };
    if result == SOCKET_ERROR {
        // SAFETY: WSAGetLastError has no preconditions; read immediately after failure.
        Err(io::Error::from_raw_os_error(unsafe { WSAGetLastError() }))
    } else {
        Ok(())
    }
}

#[cfg(test)]
pub(crate) mod fixtures;
