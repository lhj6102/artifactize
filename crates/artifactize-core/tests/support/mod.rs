//! A fake model provider on loopback for end-to-end Agent tests. It answers whatever
//! its handler scripts, in the OpenAI Responses, Anthropic Messages and Codex wire
//! formats, and records every request. Point a backend at it with its
//! `ARTIFACTIZE_<BACKEND>_BASE_URL` test endpoint (and Codex sign-in at
//! `ARTIFACTIZE_CODEX_AUTH_URL`).
//!
//! `os` holds the operating-system fixtures, and `copy_fixture` the repositories under
//! `tests/fixtures`.
#![expect(
    dead_code,
    reason = "each test binary uses a different part of the fake"
)]

use std::{
    cell::RefCell,
    collections::{BTreeMap, VecDeque},
    fs,
    io::{BufRead, BufReader, Read, Write},
    net::{Shutdown, TcpListener, TcpStream},
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};

use serde_json::{Value, json};

pub mod declaration;
pub mod os;
pub mod runs;
pub mod schema;
pub mod tool_sessions;

/// Copy `tests/fixtures/<name>` to `target`. The fixtures name Unix commands, so that the
/// docs can run them as they are; on Windows the copies name the `os::bin` stand-ins, and
/// their deadlines of a second or more get `os::slow` room. A shorter one is there to expire.
pub fn copy_fixture(name: &str, target: &Path) {
    copy(
        &Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../artifactize-core/tests/fixtures")
            .join(name),
        target,
        true,
    );
}

/// Copy a directory; with the stand-ins, its declarations then name them. Nothing else
/// changes, so a copy keeps every fingerprint that covers its declarations.
pub fn copy_directory(source: &Path, target: &Path) {
    copy(source, target, false);
}

fn copy(source: &Path, target: &Path, deadlines: bool) {
    fs::create_dir_all(target).unwrap();
    for entry in fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let destination = target.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy(&entry.path(), &destination, deadlines);
        } else {
            fs::copy(entry.path(), &destination).unwrap();
            os::port_fixture(&destination, deadlines);
        }
    }
}

#[derive(Clone, Debug)]
pub struct Request {
    pub method: String,
    pub path: String,
    /// Lowercase names.
    pub headers: BTreeMap<String, String>,
    /// The JSON body, a form body as its raw text, or null when there is none.
    pub body: Value,
}

pub enum Reply {
    /// A `text/event-stream` of `event: <type>` and `data: <json>` records.
    Events(Vec<Value>),
    Json(u16, Value),
    /// A status, extra headers and a raw body.
    Raw(u16, Vec<(&'static str, String)>, String),
}

type Handler = dyn Fn(&Request) -> Reply + Send + Sync;

#[derive(Default)]
struct ProviderLifetime {
    stop: AtomicBool,
    gates: Mutex<Vec<Gate>>,
}

struct ServingConnection {
    socket: TcpStream,
    thread: thread::JoinHandle<()>,
}

/// The retained teardown socket must not keep a completed HTTP connection open.
struct CloseConnection(TcpStream);

impl Drop for CloseConnection {
    fn drop(&mut self) {
        let _ = self.0.shutdown(Shutdown::Both);
    }
}

// Listener polling only observes cancellation; no test orders events by this interval.
const ACCEPT_POLL: Duration = Duration::from_millis(5);
// Bound a malformed/incomplete request, not the order of a test's provider interactions.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
// An unreleased response gate fails rather than leaving a test process hung indefinitely.
const GATE_TIMEOUT: Duration = Duration::from_secs(30);

thread_local! {
    // Response gates join the provider that owns this connection, so teardown wakes them.
    static PROVIDER: RefCell<Option<Arc<ProviderLifetime>>> = const { RefCell::new(None) };
}

pub struct FakeProvider {
    /// `http://127.0.0.1:PORT`, without a trailing slash.
    pub url: String,
    requests: Arc<Mutex<Vec<Request>>>,
    lifetime: Arc<ProviderLifetime>,
    connections: Arc<Mutex<Vec<ServingConnection>>>,
    thread: Option<thread::JoinHandle<()>>,
}

impl FakeProvider {
    /// Answer every request with `handler`, one connection at a time per thread.
    pub fn start(handler: impl Fn(&Request) -> Reply + Send + Sync + 'static) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let lifetime = Arc::new(ProviderLifetime::default());
        let connections = Arc::new(Mutex::new(Vec::new()));
        let handler: Arc<Handler> = Arc::new(handler);
        let thread = thread::spawn({
            let requests = requests.clone();
            let lifetime = lifetime.clone();
            let connections = connections.clone();
            move || {
                while !lifetime.stop.load(Ordering::SeqCst) {
                    match listener.accept() {
                        Ok((stream, _)) => {
                            let socket = stream.try_clone().unwrap();
                            let handler = handler.clone();
                            let requests = requests.clone();
                            let lifetime = lifetime.clone();
                            let serving = thread::spawn(move || {
                                PROVIDER.with(|provider| {
                                    *provider.borrow_mut() = Some(lifetime.clone())
                                });
                                serve(stream, &*handler, &requests, &lifetime.stop);
                                PROVIDER.with(|provider| *provider.borrow_mut() = None);
                            });
                            connections
                                .lock()
                                .unwrap_or_else(std::sync::PoisonError::into_inner)
                                .push(ServingConnection {
                                    socket,
                                    thread: serving,
                                });
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            thread::sleep(ACCEPT_POLL);
                        }
                        Err(error) => panic!("fake provider: {error}"),
                    }
                }
            }
        });
        Self {
            url,
            requests,
            lifetime,
            connections,
            thread: Some(thread),
        }
    }

    /// Answer requests in order; once the script runs out, every reply is HTTP 500.
    pub fn script(replies: Vec<Reply>) -> Self {
        let replies = Mutex::new(VecDeque::from(replies));
        Self::start(move |_| {
            replies.lock().unwrap().pop_front().unwrap_or_else(|| {
                Reply::Json(
                    500,
                    json!({"error":{"message":"The fake provider's script is exhausted."}}),
                )
            })
        })
    }

    /// The value for `ARTIFACTIZE_OPENAI_BASE_URL`.
    pub fn openai_base(&self) -> String {
        format!("{}/v1", self.url)
    }

    /// The value for `ARTIFACTIZE_ANTHROPIC_BASE_URL`.
    pub fn anthropic_base(&self) -> String {
        self.url.clone()
    }

    /// The value for `ARTIFACTIZE_CODEX_BASE_URL`, shaped like the real Codex root.
    pub fn codex_base(&self) -> String {
        format!("{}/backend-api/codex", self.url)
    }

    pub fn requests(&self) -> Vec<Request> {
        self.requests.lock().unwrap().clone()
    }
}

impl Drop for FakeProvider {
    fn drop(&mut self) {
        self.lifetime.stop.store(true, Ordering::SeqCst);
        // Finish accepting before taking the connections, including an accept racing stop.
        let mut panic = self.thread.take().and_then(|thread| thread.join().err());
        let connections = std::mem::take(
            &mut *self
                .connections
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        );
        // Close every client before joining any handler, even a handler paused at a gate.
        for connection in &connections {
            let _ = connection.socket.shutdown(Shutdown::Both);
        }
        // Stop is set before waking gates; holding each gate's mutex prevents a lost wake.
        for gate in self
            .lifetime
            .gates
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
        {
            let _open = gate
                .0
                .0
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            gate.0.1.notify_all();
        }
        for connection in connections {
            if let Err(error) = connection.thread.join() {
                panic.get_or_insert(error);
            }
        }
        // Never detach later handlers or double-panic while cleaning up a failing test.
        if !thread::panicking()
            && let Some(panic) = panic
        {
            std::panic::resume_unwind(panic);
        }
    }
}

fn serve(stream: TcpStream, handler: &Handler, requests: &Mutex<Vec<Request>>, stop: &AtomicBool) {
    let stream = CloseConnection(stream);
    stream.0.set_nonblocking(false).unwrap();
    stream.0.set_read_timeout(Some(REQUEST_TIMEOUT)).unwrap();
    let mut reader = BufReader::new(stream.0.try_clone().unwrap());
    let mut line = String::new();
    if reader.read_line(&mut line).unwrap_or(0) == 0 {
        return;
    }
    let mut parts = line.split_whitespace();
    let method = parts.next().unwrap_or_default().to_owned();
    let path = parts.next().unwrap_or_default().to_owned();
    let mut headers = BTreeMap::new();
    loop {
        let mut header = String::new();
        if reader.read_line(&mut header).unwrap_or(0) == 0 {
            return;
        }
        let header = header.trim_end();
        if header.is_empty() {
            break;
        }
        if let Some((name, value)) = header.split_once(':') {
            headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_owned());
        }
    }
    let length = headers
        .get("content-length")
        .map_or(0, |length| length.parse().unwrap());
    let mut body = vec![0; length];
    if reader.read_exact(&mut body).is_err() {
        return;
    }
    let request = Request {
        method,
        path,
        headers,
        body: if body.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&body)
                .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&body).into_owned()))
        },
    };
    let reply = handler(&request);
    requests.lock().unwrap().push(request);
    if stop.load(Ordering::SeqCst) {
        return;
    }
    let (status, mut extra, body) = match reply {
        Reply::Events(events) => (
            200,
            vec![("content-type", "text/event-stream".to_owned())],
            events
                .iter()
                .map(|event| {
                    format!(
                        "event: {}\ndata: {event}\n\n",
                        event["type"].as_str().unwrap()
                    )
                })
                .collect(),
        ),
        Reply::Json(status, body) => (
            status,
            vec![("content-type", "application/json".to_owned())],
            body.to_string(),
        ),
        Reply::Raw(status, headers, body) => (status, headers, body),
    };
    extra.push(("content-length", body.len().to_string()));
    extra.push(("connection", "close".to_owned()));
    let mut response = format!("HTTP/1.1 {status} Fake\r\n");
    for (name, value) in extra {
        response.push_str(&format!("{name}: {value}\r\n"));
    }
    response.push_str("\r\n");
    response.push_str(&body);
    let mut stream = reader.into_inner();
    let _ = stream.write_all(response.as_bytes());
    let _ = stream.flush();
}

/// OpenAI Responses replies (`POST {base}/responses`, `GET {base}/models`).
pub mod openai {
    use super::{Reply, Request};
    use serde_json::{Value, json};

    pub fn message(text: &str) -> Value {
        json!({"type":"message","id":"msg_fake","role":"assistant","status":"completed",
            "content":[{"type":"output_text","text":text,"annotations":[]}]})
    }

    pub fn function_call(call_id: &str, name: &str, arguments: &Value) -> Value {
        json!({"type":"function_call","id":format!("fc_{call_id}"),"call_id":call_id,
            "name":name,"arguments":arguments.to_string(),"status":"completed"})
    }

    /// A reasoning item with a summary and the encrypted content a stateless replay needs.
    pub fn reasoning(id: &str, summary: &str, encrypted: &str) -> Value {
        json!({"type":"reasoning","id":id,"encrypted_content":encrypted,
            "summary":[{"type":"summary_text","text":summary}]})
    }

    pub fn usage(input: u64, output: u64) -> Value {
        json!({"input_tokens":input,"output_tokens":output,"total_tokens":input + output,
            "input_tokens_details":{"cached_tokens":0},
            "output_tokens_details":{"reasoning_tokens":0}})
    }

    /// A streamed, completed response for the requested model. Function calls and reasoning
    /// items are also streamed as items, as the providers do.
    pub fn completed(request: &Request, output: Vec<Value>, usage: Value) -> Reply {
        let mut events = Vec::new();
        for (index, item) in output.iter().enumerate() {
            if item["type"] == "function_call" || item["type"] == "reasoning" {
                for kind in ["response.output_item.added", "response.output_item.done"] {
                    events.push(json!({"type":kind,"output_index":index,"item":item}));
                }
            }
        }
        let response = json!({"id":"resp_fake","object":"response","created_at":1,
            "model":request.body["model"],"status":"completed","output":output,"usage":usage});
        events.push(json!({"type":"response.completed","response":response}));
        for (index, event) in events.iter_mut().enumerate() {
            event["sequence_number"] = json!(index);
        }
        Reply::Events(events)
    }

    pub fn models(ids: &[&str]) -> Reply {
        let data: Vec<_> = ids
            .iter()
            .map(|id| json!({"id":id,"object":"model","created":1,"owned_by":"fake"}))
            .collect();
        Reply::Json(200, json!({"object":"list","data":data}))
    }
}

/// Anthropic Messages replies (`POST {base}/v1/messages`, `GET {base}/v1/models`).
pub mod anthropic {
    use super::{Reply, Request};
    use serde_json::json;

    /// A streamed text answer for the requested model.
    pub fn text(request: &Request, text: &str, input: u64, output: u64) -> Reply {
        Reply::Events(vec![
            json!({"type":"message_start","message":{"id":"msg_fake","type":"message",
                "role":"assistant","model":request.body["model"],"content":[],
                "stop_reason":null,"stop_sequence":null,
                "usage":{"input_tokens":input,"output_tokens":0}}}),
            json!({
                "type":"content_block_start",
                "index":0,
                "content_block":{"type":"text","text":""},
            }),
            json!({
                "type":"content_block_delta",
                "index":0,
                "delta":{"type":"text_delta","text":text},
            }),
            json!({"type":"content_block_stop","index":0}),
            json!({"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},
                "usage":{"output_tokens":output}}),
            json!({"type":"message_stop"}),
        ])
    }

    pub fn models(ids: &[&str]) -> Reply {
        let data: Vec<_> = ids
            .iter()
            .map(|id| {
                json!({
                    "id":id,
                    "type":"model",
                    "display_name":id,
                    "created_at":"2026-01-01T00:00:00Z",
                })
            })
            .collect();
        Reply::Json(
            200,
            json!({"data":data,"has_more":false,"first_id":null,"last_id":null}),
        )
    }
}

/// OpenAI Chat Completions replies (`POST {base}/chat/completions`), as compatible servers
/// stream them: `data:` records ending with `[DONE]`.
pub mod chat {
    use super::{Reply, Request};
    use serde_json::json;

    /// A streamed text answer for the requested model.
    pub fn text(request: &Request, text: &str, input: u64, output: u64) -> Reply {
        let chunk = |delta: serde_json::Value, finish: serde_json::Value| {
            json!({"id":"chatcmpl-fake","object":"chat.completion.chunk","created":0,
                "model":request.body["model"],
                "choices":[{"index":0,"delta":delta,"finish_reason":finish}]})
        };
        let mut last = chunk(json!({}), json!("stop"));
        last["usage"] = json!({"prompt_tokens":input,"completion_tokens":output,
            "total_tokens":input + output});
        let body = [
            chunk(json!({"role":"assistant","content":text}), json!(null)),
            last,
        ]
        .iter()
        .map(|record| format!("data: {record}\n\n"))
        .chain(["data: [DONE]\n\n".to_owned()])
        .collect();
        Reply::Raw(
            200,
            vec![("content-type", "text/event-stream".to_owned())],
            body,
        )
    }
}

/// Gemini replies (`POST {root}/v1beta/models/{model}:streamGenerateContent?alt=sse`).
pub mod gemini {
    use super::Reply;
    use serde_json::json;

    /// A streamed text answer from `model`.
    pub fn text(model: &str, text: &str, input: u64, output: u64) -> Reply {
        let record = json!({
            "candidates":[{"content":{"role":"model","parts":[{"text":text}]},
                "finishReason":"STOP","index":0}],
            "usageMetadata":{"promptTokenCount":input,"candidatesTokenCount":output,
                "totalTokenCount":input + output},
            "modelVersion":model,
        });
        Reply::Raw(
            200,
            vec![("content-type", "text/event-stream".to_owned())],
            format!("data: {record}\n\n"),
        )
    }

    pub fn models(ids: &[&str]) -> Reply {
        let models: Vec<_> = ids
            .iter()
            .map(|id| {
                json!({"name":format!("models/{id}"),"displayName":id,
                "supportedGenerationMethods":["generateContent"]})
            })
            .collect();
        Reply::Json(200, json!({"models":models}))
    }
}

/// Codex replies (`POST <root>/responses`, `GET <root>/models`) and sign-in tokens.
pub mod codex {
    /// No CLI fixture expires while scheduling; expiry boundaries use injected unit clocks.
    pub const VALID_EXPIRY: u64 = u64::MAX;
    /// Expired independently of wall-clock resolution or how long the fixture takes to run.
    pub const EXPIRED: u64 = 0;

    use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
    use serde_json::{Value, json};

    use super::{Reply, Request, openai};

    /// An unsigned access token naming a ChatGPT account and an expiry, as the
    /// Codex token endpoint issues them.
    pub fn jwt(account: &str, exp: u64) -> String {
        let encode = |value: Value| URL_SAFE_NO_PAD.encode(value.to_string());
        format!(
            "{}.{}.fake-signature",
            encode(json!({"alg":"RS256","typ":"JWT"})),
            encode(json!({"exp":exp,"https://api.openai.com/auth":{"chatgpt_account_id":account}}))
        )
    }

    /// A completed Responses stream with no content type, as the Codex backend sends it.
    pub fn completed(request: &Request, output: Vec<Value>, usage: Value) -> Reply {
        let Reply::Events(events) = openai::completed(request, output, usage) else {
            unreachable!()
        };
        stream(events)
    }

    /// Server-sent events without a content type.
    pub fn stream(events: Vec<Value>) -> Reply {
        let body = events
            .iter()
            .map(|event| {
                format!(
                    "event: {}\ndata: {event}\n\n",
                    event["type"].as_str().unwrap()
                )
            })
            .collect();
        Reply::Raw(200, Vec::new(), body)
    }

    pub fn tokens(access: &str, refresh: &str) -> Reply {
        Reply::Json(
            200,
            json!({
                "access_token":access,
                "refresh_token":refresh,
                "expires_in":3600,
                "id_token":"id",
            }),
        )
    }
}

/// A reusable explicit provider-response barrier. Deadlines bound failures, not ordering.
#[derive(Clone, Default)]
pub struct Gate(Arc<(Mutex<bool>, std::sync::Condvar)>);

impl Gate {
    pub fn close(&self) {
        *self.0.0.lock().unwrap() = false;
    }
    pub fn open(&self) {
        *self.0.0.lock().unwrap() = true;
        self.0.1.notify_all();
    }
    pub fn wait(&self) {
        let provider = PROVIDER.with(|provider| provider.borrow().clone());
        if let Some(provider) = &provider {
            provider.gates.lock().unwrap().push(self.clone());
        }
        let cancelled = || {
            provider
                .as_ref()
                .is_some_and(|provider| provider.stop.load(Ordering::SeqCst))
        };
        let (lock, changed) = &*self.0;
        let (open, _) = changed
            .wait_timeout_while(lock.lock().unwrap(), os::patience(GATE_TIMEOUT), |open| {
                !*open && !cancelled()
            })
            .unwrap();
        assert!(cancelled() || *open, "response gate was never released");
    }
}

#[cfg(test)]
mod lifecycle_tests {
    use super::*;
    use std::sync::mpsc;

    fn client(provider: &FakeProvider, path: &str) -> TcpStream {
        let mut client = TcpStream::connect(provider.url.strip_prefix("http://").unwrap()).unwrap();
        client
            .set_read_timeout(Some(os::patience(REQUEST_TIMEOUT)))
            .unwrap();
        write!(client, "GET {path} HTTP/1.1\r\nHost: fake\r\n\r\n").unwrap();
        client
    }

    fn assert_closed(mut client: TcpStream) {
        let mut byte = [0];
        match client.read(&mut byte) {
            Ok(0) => {}
            Err(error) if error.kind() == std::io::ErrorKind::ConnectionReset => {}
            result => panic!("provider teardown left the client open: {result:?}"),
        }
    }

    #[test]
    fn completed_request_closes_its_socket_before_provider_teardown() {
        let provider = FakeProvider::script(vec![Reply::Json(200, json!({"answer":"done"}))]);
        let mut client = client(&provider, "/completed");
        let mut response = String::new();
        client.read_to_string(&mut response).unwrap();
        assert!(response.starts_with("HTTP/1.1 200 "), "{response}");
        assert!(response.ends_with(r#"{"answer":"done"}"#), "{response}");
        assert_eq!(provider.requests().len(), 1);
    }

    #[test]
    fn provider_teardown_cancels_all_gated_handlers_and_closes_clients() {
        let (entered, wait) = mpsc::channel();
        let (finished, exits) = mpsc::channel();
        let gate = Gate::default();
        let provider = FakeProvider::start(move |_| {
            entered.send(()).unwrap();
            gate.wait();
            finished.send(()).unwrap();
            Reply::Json(200, json!({}))
        });
        let clients = [client(&provider, "/one"), client(&provider, "/two")];
        for _ in &clients {
            wait.recv_timeout(os::patience(GATE_TIMEOUT)).unwrap();
        }
        drop(provider);
        for client in clients {
            exits.recv_timeout(os::patience(GATE_TIMEOUT)).unwrap();
            assert_closed(client);
        }
    }

    #[test]
    fn provider_teardown_joins_every_handler_before_reporting_a_panic() {
        for unwinding in [false, true] {
            let (entered, wait) = mpsc::channel();
            let (finished, exits) = mpsc::channel();
            let gate = Gate::default();
            let provider = FakeProvider::start(move |request| {
                if request.path == "/panic" {
                    panic!("scripted handler panic");
                }
                entered.send(()).unwrap();
                gate.wait();
                finished.send(()).unwrap();
                Reply::Json(200, json!({}))
            });
            // EOF proves the first accepted handler has finished unwinding, not just entered.
            assert_closed(client(&provider, "/panic"));
            let waiting = client(&provider, "/gated");
            wait.recv_timeout(os::patience(GATE_TIMEOUT)).unwrap();
            // The provider is moved into and destroyed inside the closure, never reused
            // after an unwind; JoinHandle's conservative UnwindSafe bound is immaterial.
            let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let provider = provider;
                if unwinding {
                    panic!("scripted test panic");
                }
                drop(provider);
            }))
            .unwrap_err();
            assert_eq!(
                panic.downcast_ref::<&str>(),
                Some(&if unwinding {
                    "scripted test panic"
                } else {
                    "scripted handler panic"
                })
            );
            exits.recv_timeout(os::patience(GATE_TIMEOUT)).unwrap();
            assert_closed(waiting);
        }
    }
}
