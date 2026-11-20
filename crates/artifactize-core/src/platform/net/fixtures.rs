use std::net::{Ipv4Addr, SocketAddr};

use socket2::{Domain, Protocol, Socket, Type};

/// A competing tool may enable reuse before binding wildcard or loopback. Retain
/// its socket until sign-in has completed, so the fallback cannot pass by a race. None when
/// the system has no IPv6.
fn competing(address: SocketAddr, reuse: bool) -> std::io::Result<Option<Socket>> {
    let bind = || {
        let socket = Socket::new(
            Domain::for_address(address),
            Type::STREAM,
            Some(Protocol::TCP),
        )?;
        if address.is_ipv6() {
            socket.set_only_v6(true)?;
        }
        socket.set_reuse_address(reuse)?;
        socket.bind(&address.into())?;
        // One browser request is enough to reproduce the competing sign-in.
        const BACKLOG: i32 = 1;
        socket.listen(BACKLOG)?;
        Ok::<_, std::io::Error>(socket)
    };
    match bind() {
        Ok(socket) => Ok(Some(socket)),
        Err(error) if address.is_ipv6() && super::ipv6_unavailable(&error) => Ok(None),
        Err(error) => Err(error),
    }
}

/// The competing listener at `address` and its port. An IPv6 competitor takes a port that
/// IPv4 loopback has free as well, reserved until the competitor holds it: the test later
/// takes that IPv4 port itself, which another process may hold on any port it did not check.
pub(crate) fn competitor(address: SocketAddr, reuse: bool) -> Option<(Socket, u16)> {
    let port = |socket: &Socket| socket.local_addr().unwrap().as_socket().unwrap().port();
    if address.is_ipv4() {
        let other = competing(address, reuse)
            .unwrap_or_else(|error| panic!("cannot start competing listener: {error}"))?;
        let port = port(&other);
        return Some((other, port));
    }
    loop {
        let ipv4 = super::exclusive_listener(SocketAddr::from((Ipv4Addr::LOCALHOST, 0))).unwrap();
        let free = ipv4.local_addr().unwrap().port();
        match competing(SocketAddr::new(address.ip(), free), reuse) {
            Ok(other) => return other.map(|other| (other, free)),
            // Another process holds that port for IPv6: choose another.
            Err(error) if super::address_in_use(&error) => {}
            Err(error) => panic!("cannot start competing listener: {error}"),
        }
    }
}
