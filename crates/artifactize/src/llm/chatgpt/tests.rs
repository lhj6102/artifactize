use std::{
    collections::BTreeMap,
    fs,
    os::unix::fs::{DirBuilderExt, OpenOptionsExt},
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};

use rig_core::test_utils::MockHttpResponse;
use serde_json::json;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

use super::*;

pub(crate) fn stored_credentials(state: &Path, token: &str) {
    use std::io::Write;
    let directory = state.join("auth");
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&directory)
        .unwrap();
    let identity =
        json!({"issuer":"https://auth.openai.com", "subject":"test-account", "email":null});
    let registration =
        json!({"client_id":"oaiapp_test", "ext_agent_host_id":"test-host", "identity":identity});
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let credentials = json!({
        "access_token":token, "refresh_token":"test-refresh", "id_token":"test-id",
        "token_type":"Bearer", "expires_at":now + 3600, "saved_at":now,
        "client_id":"oaiapp_test", "ext_agent_host_id":"test-host",
        "scopes":["chatgpt.tokens.use.direct"], "identity":identity,
    });
    for (name, data) in [
        ("chatgpt-registration.json", registration),
        ("chatgpt.json", credentials),
    ] {
        let mut file = fs::OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .mode(0o600)
            .open(directory.join(name))
            .unwrap();
        file.write_all(data.to_string().as_bytes()).unwrap();
    }
}

#[derive(Clone)]
pub(crate) struct Request {
    pub line: String,
    pub headers: BTreeMap<String, String>,
    pub body: Value,
}

pub(crate) struct Server {
    pub base: String,
    requests: Arc<Mutex<Vec<Request>>>,
    task: tokio::task::JoinHandle<()>,
}

impl Server {
    pub async fn new(responses: Vec<MockHttpResponse>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}/v1", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let captured = requests.clone();
        let task = tokio::spawn(async move {
            for response in responses {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut bytes = Vec::new();
                let end = loop {
                    let mut buffer = [0; 4096];
                    let count = socket.read(&mut buffer).await.unwrap();
                    assert_ne!(count, 0);
                    bytes.extend_from_slice(&buffer[..count]);
                    if let Some(end) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
                        break end + 4;
                    }
                };
                let head = String::from_utf8(bytes[..end].to_vec()).unwrap();
                let mut lines = head.lines();
                let line = lines.next().unwrap().to_owned();
                let headers: BTreeMap<String, String> = lines
                    .filter_map(|line| line.split_once(':'))
                    .map(|(name, value)| (name.to_ascii_lowercase(), value.trim().to_owned()))
                    .collect();
                let length = headers
                    .get("content-length")
                    .map(|value| value.parse().unwrap())
                    .unwrap_or(0);
                while bytes.len() < end + length {
                    let mut buffer = [0; 4096];
                    let count = socket.read(&mut buffer).await.unwrap();
                    assert_ne!(count, 0);
                    bytes.extend_from_slice(&buffer[..count]);
                }
                let body = if length == 0 {
                    Value::Null
                } else {
                    serde_json::from_slice(&bytes[end..end + length]).unwrap()
                };
                captured.lock().unwrap().push(Request {
                    line,
                    headers,
                    body,
                });
                let (status, body, content_type) = match response {
                    MockHttpResponse::Success(body) => (200, body, "application/json".to_owned()),
                    MockHttpResponse::SuccessWithHeaders(body, headers) => (
                        200,
                        body,
                        headers["content-type"].to_str().unwrap().to_owned(),
                    ),
                    MockHttpResponse::ErrorResponse(status, body) => {
                        (status.as_u16(), body, "application/json".into())
                    }
                    MockHttpResponse::ErrorWithHeaders(status, body, _) => {
                        (status.as_u16(), body.into(), "application/json".into())
                    }
                    _ => panic!("unsupported test response"),
                };
                // An empty type omits the header, as the real SIWC event stream does.
                let content_type = match content_type.as_str() {
                    "" => String::new(),
                    value => format!("Content-Type: {value}\r\n"),
                };
                let header = format!(
                    "HTTP/1.1 {status} Test\r\n{content_type}Content-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                socket.write_all(header.as_bytes()).await.unwrap();
                socket.write_all(&body).await.unwrap();
            }
        });
        Self {
            base,
            requests,
            task,
        }
    }

    pub fn requests(&self) -> Vec<Request> {
        self.requests.lock().unwrap().clone()
    }

    pub fn client(&self, state: &Path, repo: &Path) -> crate::llm::Client {
        let crate::llm::Client::Chatgpt(client) =
            crate::llm::Client::new(crate::config::Backend::Chatgpt, "exact-model", state, repo)
                .unwrap()
        else {
            unreachable!()
        };
        crate::llm::Client::Chatgpt(client.with_base_url(self.base.clone()))
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}

#[tokio::test]
async fn models_use_stored_bearer_filter_visibility_and_preserve_server_order() {
    let directory = tempfile::tempdir().unwrap();
    let state = directory.path().join("state");
    stored_credentials(&state, "models-token");
    let server = Server::new(vec![MockHttpResponse::success(
        json!({"models":[
            {"slug":"z-model", "display_name":"Z model", "visibility":"list"},
            {"slug":"hidden", "visibility":"hidden"},
            {"slug":"unknown"},
            {"slug":"a-model", "display_name":"A model", "visibility":"list"},
        ]})
        .to_string(),
    )])
    .await;
    let models = models_at(Some(&state), None, &server.base).await.unwrap();
    assert_eq!(
        serde_json::to_value(models).unwrap(),
        json!([
            {"slug":"z-model", "display_name":"Z model"},
            {"slug":"a-model", "display_name":"A model"},
        ])
    );
    let requests = server.requests();
    assert_eq!(requests[0].line, "GET /v1/models HTTP/1.1");
    assert_eq!(requests[0].headers["authorization"], "Bearer models-token");
    assert_eq!(requests[0].body, Value::Null);
}

#[tokio::test]
async fn models_errors_keep_codes_and_auth_guidance() {
    let directory = tempfile::tempdir().unwrap();
    let state = directory.path().join("state");
    stored_credentials(&state, "models-token");
    for (status, body, expected) in [
        (
            401,
            json!({"detail":"Identity rejected"}),
            "artifactize login chatgpt",
        ),
        (
            403,
            json!({"error":{"code":"subscription_sharing_user_not_eligible", "message":"Account not eligible"}}),
            "subscription_sharing_user_not_eligible: Account not eligible",
        ),
    ] {
        let server = Server::new(vec![MockHttpResponse::error(
            rig_core::http_client::StatusCode::from_u16(status).unwrap(),
            body.to_string(),
        )])
        .await;
        let error = models_at(Some(&state), None, &server.base)
            .await
            .err()
            .unwrap();
        assert!(error.contains(expected), "{error}");
        assert!(error.contains(&format!("HTTP {status}")));
    }
}

#[test]
fn production_uses_public_api_without_environment_overrides() {
    let client = Chatgpt::new("exact-model", Path::new("state"), Path::new("repo")).unwrap();
    assert_eq!(client.base_url, "https://api.openai.com/v1");
}
