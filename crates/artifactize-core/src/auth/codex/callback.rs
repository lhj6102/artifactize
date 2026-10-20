//! Reserve both localhost address families, or release both and use pasted redirects.
use std::{
    io,
    net::{Ipv4Addr, Ipv6Addr, SocketAddr},
};

use tokio::net::TcpListener;

use super::Pending;
use crate::platform::net::{address_in_use, exclusive_listener, ipv6_unavailable};

pub(super) struct Callback {
    pub(super) ipv4: TcpListener,
    pub(super) ipv6: Option<TcpListener>,
}

impl Callback {
    /// Port zero is for callers testing with an OS-assigned port. Both listeners still
    /// reserve the same port, just as the fixed registered OAuth redirect does.
    pub(super) fn bind(port: u16) -> io::Result<Option<Self>> {
        let ipv4 = match exclusive_listener(SocketAddr::from((Ipv4Addr::LOCALHOST, port))) {
            Ok(listener) => listener,
            Err(error) if address_in_use(&error) => return Ok(None),
            Err(error) => return Err(error),
        };
        let port = ipv4.local_addr()?.port();
        let ipv6 = match exclusive_listener(SocketAddr::from((Ipv6Addr::LOCALHOST, port))) {
            Ok(listener) => Some(listener),
            Err(error) if address_in_use(&error) => return Ok(None),
            Err(error) if ipv6_unavailable(&error) => None,
            Err(error) => return Err(error),
        };
        Ok(Some(Self { ipv4, ipv6 }))
    }

    pub(super) async fn wait(self, pending: &Pending) -> Result<String, String> {
        let ipv6 = async {
            match self.ipv6 {
                Some(listener) => pending.serve(listener).await,
                None => std::future::pending().await,
            }
        };
        tokio::select! {
            result = pending.serve(self.ipv4) => result,
            result = ipv6 => result,
        }
    }
}
