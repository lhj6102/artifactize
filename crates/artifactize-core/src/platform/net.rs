//! Exclusive loopback listeners: do not let an OAuth callback reach a different process.
use std::{io, net::SocketAddr};

use socket2::{Domain, Protocol, Socket, Type};
use tokio::net::TcpListener;

/// Bind without Unix's SO_REUSEADDR, and with Windows's SO_EXCLUSIVEADDRUSE.
/// IPv6 is v6-only so the two loopback addresses can be reserved independently.
pub(crate) fn exclusive_listener(address: SocketAddr) -> io::Result<TcpListener> {
    let socket = Socket::new(
        Domain::for_address(address),
        Type::STREAM,
        Some(Protocol::TCP),
    )?;
    if address.is_ipv6() {
        socket.set_only_v6(true)?;
    }
    #[cfg(windows)]
    exclusive(&socket)?;
    socket.bind(&address.into())?;
    // Only a browser's short callback connects here; no large accept queue is needed.
    const CALLBACK_BACKLOG: i32 = 8;
    socket.listen(CALLBACK_BACKLOG)?;
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
