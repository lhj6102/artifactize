//! Pin the public redirect contract without real OAuth or fixed-port binding.
use super::*;
use crate::{auth::codex::tests::jwt, llm::tests::Server};
use rig_core::test_utils::MockHttpResponse;

#[tokio::test]
async fn shared_port_redirect_matches_authorize_callback_and_token_exchange() {
    let directory = crate::test_os::tempdir();
    let storage = Storage::new(Some(directory.path()), None, Tokens::Codex).unwrap();
    let server = Server::new(vec![MockHttpResponse::success(
        json!({
            "access_token":jwt("fixture-account", now().unwrap() + 3600),
            "refresh_token":"fixture-refresh",
            "expires_in":3600,
        })
        .to_string(),
    )])
    .await;
    let redirect = format!("http://localhost:{CALLBACK_PORT}{CALLBACK_PATH}");
    assert_eq!(redirect, "http://localhost:1455/auth/callback");
    let (send, receive) = oneshot::channel();
    sign_in(
        &storage,
        &server.base,
        None,
        &redirect,
        Some(receive),
        false,
        |url, callback| {
            assert!(!callback);
            let pairs: BTreeMap<_, _> = url.query_pairs().into_owned().collect();
            assert_eq!(pairs["redirect_uri"], "http://localhost:1455/auth/callback");
            let target = format!("{CALLBACK_PATH}?code=fixture-code&state={}", pairs["state"]);
            let pending = Pending {
                verifier: String::new(),
                state: pairs["state"].clone(),
            };
            assert!(
                matches!(pending.callback(&target).2, Some(Ok(code)) if code == "fixture-code")
            );
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
    assert_eq!(form["redirect_uri"], "http://localhost:1455/auth/callback");
    assert_eq!(form["code"], "fixture-code");
}
