//! Verify both redirect policies without contacting Codex or binding its registered port.
use super::*;
use crate::{auth::codex::tests::jwt, llm::tests::Server};
use rig_core::test_utils::MockHttpResponse;

#[tokio::test]
async fn shared_port_redirect_matches_authorize_and_token_exchange() {
    let redirect = CallbackPort::default().redirect(None).unwrap();
    assert_eq!(redirect, "http://localhost:1455/auth/callback");
    assert_redirect_exchange(None, &redirect).await;
}

#[tokio::test]
async fn ephemeral_redirect_matches_the_listener_and_token_exchange() {
    let callback = CallbackPort::Ephemeral;
    assert!(callback.redirect(None).is_err());
    let listener = callback.bind().unwrap().unwrap();
    let port = listener.ipv4.local_addr().unwrap().port();
    assert_ne!(port, 0);
    assert_ne!(port, CALLBACK_PORT);
    let redirect = callback.redirect(Some(&listener)).unwrap();
    assert_eq!(Url::parse(&redirect).unwrap().port(), Some(port));
    assert_redirect_exchange(Some(listener), &redirect).await;
}

async fn assert_redirect_exchange(listener: Option<Callback>, redirect: &str) {
    let callback_expected = listener.is_some();
    let directory = crate::test_os::tempdir();
    let storage = Storage::new(Some(directory.path()), None, Tokens::Codex).unwrap();
    Server::run(
        vec![MockHttpResponse::success(
            json!({
                "access_token":jwt("fixture-account", u64::MAX),
                "refresh_token":"fixture-refresh",
                "expires_in":3600,
            })
            .to_string(),
        )],
        async |server| {
            let (send, receive) = oneshot::channel();
            sign_in(
                &storage,
                &server.base.parse().unwrap(),
                listener,
                redirect,
                Some(receive),
                false,
                |url, callback| {
                    assert_eq!(callback, callback_expected);
                    let pairs: BTreeMap<_, _> = url.query_pairs().into_owned().collect();
                    assert_eq!(pairs["redirect_uri"], redirect);
                    send.send(format!(
                        "{redirect}?code=fixture-code&state={}",
                        pairs["state"]
                    ))
                    .unwrap();
                },
            )
            .await
            .unwrap();
            let requests = server.requests();
            assert_eq!(requests.len(), 1);
            let form: BTreeMap<_, _> =
                url::form_urlencoded::parse(requests[0].body.as_str().unwrap().as_bytes())
                    .into_owned()
                    .collect();
            assert_eq!(form["redirect_uri"], redirect);
            assert_eq!(form["code"], "fixture-code");
        },
    )
    .await;
}
