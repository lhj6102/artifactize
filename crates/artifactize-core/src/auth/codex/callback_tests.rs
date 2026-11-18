//! Exercise real competing listeners and both browser routes without the registered port.
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};

use super::{
    tests::{browser_at, jwt},
    *,
};
use crate::{llm::tests::Server, platform::net::fixtures::competitor};
use rig_core::test_utils::MockHttpResponse;

async fn token_server() -> Server {
    Server::new(vec![MockHttpResponse::success(
        json!({
            "access_token":jwt("fixture-account", u64::MAX),
            "refresh_token":"fixture-refresh",
            "expires_in":3600,
        })
        .to_string(),
    )])
    .await
}

async fn pasted_after_conflict(address: SocketAddr, reuse: bool) {
    let Some((other, port)) = competitor(address, reuse) else {
        return;
    };
    let directory = crate::test_os::tempdir();
    let storage = Storage::new(Some(directory.path()), None, Tokens::Codex).unwrap();
    let server = token_server().await;
    let (send, receive) = oneshot::channel();
    let listener = Callback::bind(port).unwrap();
    // An IPv6 conflict must release the IPv4 reservation too, not leave a partial
    // callback behind while the UI instructs the person to paste.
    if address.is_ipv6() {
        assert_eq!(
            tokio::net::TcpStream::connect((Ipv4Addr::LOCALHOST, port))
                .await
                .unwrap_err()
                .kind(),
            std::io::ErrorKind::ConnectionRefused,
        );
    }
    sign_in(
        &storage,
        &server.base.parse().unwrap(),
        listener,
        "http://localhost:1455/auth/callback",
        Some(receive),
        false,
        |url, callback| {
            assert!(!callback, "competing listener at {address}, reuse={reuse}");
            let state = url.query_pairs().find(|(key, _)| key == "state").unwrap().1;
            send.send(format!(
                "http://localhost:1455/auth/callback?code=pasted&state={state}"
            ))
            .unwrap();
        },
    )
    .await
    .unwrap();
    let saved: Credentials = storage.read(CREDENTIALS).unwrap().unwrap();
    assert_eq!(saved.account_id, "fixture-account");
    let requests = server.requests();
    assert_eq!(requests.len(), 1);
    let form: BTreeMap<_, _> =
        url::form_urlencoded::parse(requests[0].body.as_str().unwrap().as_bytes())
            .into_owned()
            .collect();
    assert_eq!(form["code"], "pasted");
    drop(other);
}

#[tokio::test]
async fn wildcard_conflicts_with_and_without_reuse_complete_from_pasted_redirects() {
    for reuse in [false, true] {
        pasted_after_conflict(SocketAddr::from((Ipv4Addr::UNSPECIFIED, 0)), reuse).await;
    }
}

#[tokio::test]
async fn ipv4_loopback_conflicts_complete_from_pasted_redirects() {
    for reuse in [false, true] {
        pasted_after_conflict(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)), reuse).await;
    }
}

#[tokio::test]
async fn ipv6_loopback_and_wildcard_conflicts_release_ipv4_and_accept_pastes() {
    for address in [Ipv6Addr::LOCALHOST, Ipv6Addr::UNSPECIFIED] {
        for reuse in [false, true] {
            pasted_after_conflict(SocketAddr::from((address, 0)), reuse).await;
        }
    }
}

#[tokio::test]
async fn ipv6_browser_callback_completes_and_releases_both_listeners() {
    let listener = Callback::bind(0).unwrap().unwrap();
    let Some(ipv6) = &listener.ipv6 else { return };
    let address = ipv6.local_addr().unwrap();
    let port = address.port();
    let directory = crate::test_os::tempdir();
    let storage = Storage::new(Some(directory.path()), None, Tokens::Codex).unwrap();
    let server = token_server().await;
    let (send, receive) = oneshot::channel();
    let browse = async {
        let url: Url = receive.await.unwrap();
        let state = url.query_pairs().find(|(key, _)| key == "state").unwrap().1;
        let response = browser_at(
            address,
            &format!("localhost:{port}"),
            &format!("/auth/callback?code=ipv6&state={state}"),
        )
        .await;
        assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    };
    let redirect = format!("http://localhost:{port}/auth/callback");
    let auth_root = server.base.parse().unwrap();
    let (result, ()) = tokio::join!(
        sign_in(
            &storage,
            &auth_root,
            Some(listener),
            &redirect,
            None,
            false,
            |url, callback| {
                assert!(callback);
                send.send(url.clone()).unwrap();
            }
        ),
        browse,
    );
    result.unwrap();
    let saved: Credentials = storage.read(CREDENTIALS).unwrap().unwrap();
    assert_eq!(saved.account_id, "fixture-account");
    let requests = server.requests();
    let form: BTreeMap<_, _> =
        url::form_urlencoded::parse(requests[0].body.as_str().unwrap().as_bytes())
            .into_owned()
            .collect();
    assert_eq!(form["code"], "ipv6");
    // The winning connection can remain in TIME_WAIT, so check that neither live
    // listener remains by connecting, rather than requiring immediate exclusive rebind.
    assert!(
        tokio::net::TcpStream::connect((Ipv4Addr::LOCALHOST, port))
            .await
            .is_err()
    );
    assert!(tokio::net::TcpStream::connect(address).await.is_err());
}
