//! A loopback HTTP server that replays canned provider responses, and the test endpoint rules.

use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

use rig_core::test_utils::MockHttpResponse;
use serde_json::Value;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

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
                // A form body is kept as its raw text.
                let raw = &bytes[end..end + length];
                let body = if length == 0 {
                    Value::Null
                } else {
                    serde_json::from_slice(raw).unwrap_or_else(|_| {
                        Value::String(String::from_utf8_lossy(raw).into_owned())
                    })
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
                // An empty type omits the header.
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
}

impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}

#[test]
fn test_endpoints_are_loopback_only() {
    for (value, expected) in [
        ("http://127.0.0.1:8080/v1", "http://127.0.0.1:8080/v1"),
        ("http://127.0.0.1:8080/v1/", "http://127.0.0.1:8080/v1"),
        ("http://127.1.2.3:9/root", "http://127.1.2.3:9/root"),
        ("http://LOCALHOST:9", "http://localhost:9"),
        ("https://[::1]:8443/", "https://[::1]:8443"),
    ] {
        assert_eq!(super::loopback_url(value).unwrap(), expected, "{value}");
    }
    for value in [
        "https://api.openai.com/v1",
        "http://10.0.0.1/v1",
        "http://0.0.0.0:8080",
        "http://[::ffff:10.0.0.1]/",
        "http://localhost.example/",
        "http://user:secret@127.0.0.1/",
        "http://127.0.0.1/?key=1",
        "http://127.0.0.1/#part",
        "ftp://127.0.0.1/",
        "127.0.0.1:8080",
        "",
    ] {
        assert!(super::loopback_url(value).is_err(), "{value}");
    }
}
