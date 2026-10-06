//! A fake model provider on loopback for end-to-end Agent tests. It answers whatever
//! its handler scripts, in the OpenAI Responses, Anthropic Messages and Codex wire
//! formats, and records every request. Point a backend at it with its
//! `ARTIFACTIZE_<BACKEND>_BASE_URL` test endpoint (and Codex sign-in at
//! `ARTIFACTIZE_CODEX_AUTH_URL`).
//!
//! `os` holds the operating-system fixtures, and `copy_fixture` the repositories under
//! `tests/fixtures`.
#![allow(
    dead_code,
    reason = "each test binary uses a different part of the fake"
)]

use std::{
    collections::{BTreeMap, VecDeque},
    fs,
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use serde_json::{Value, json};

pub mod os;

/// A recorded `tests/fixtures/state-v*.sql` with `@ROOT@` set to `root`. Each recorded path
/// below it takes the platform's separators, as artifactize records them there, and is
/// escaped inside the JSON documents but not in SQL text.
pub fn recorded_state(sql: &str, root: &Path) -> String {
    let paths = regex::Regex::new(r"('?)@ROOT@((?:/[A-Za-z0-9._-]+)*)").unwrap();
    paths
        .replace_all(sql, |found: &regex::Captures| {
            let path = found[2]
                .split('/')
                .filter(|part| !part.is_empty())
                .fold(root.to_owned(), |path, part| path.join(part));
            let path = path.to_str().unwrap();
            if found[1].is_empty() {
                let escaped = serde_json::to_string(path).unwrap();
                escaped[1..escaped.len() - 1].to_owned()
            } else {
                format!("'{path}")
            }
        })
        .into_owned()
}

/// Copy `tests/fixtures/<name>` to `target`. The fixtures name Unix commands, so that the
/// docs can run them as they are; on Windows the copies name the `os::bin` stand-ins, and
/// their deadlines of a second or more get `os::slow` room. A shorter one is there to expire.
pub fn copy_fixture(name: &str, target: &Path) {
    copy(
        &Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
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
            if os::stand_ins() {
                port(&destination, deadlines);
            }
        }
    }
}

/// Point a copied declaration's Unix commands at the stand-ins, and with `deadlines` give
/// its deadlines of a second or more `os::slow` room. The runtime fixture's `check.sh`
/// compares its working folder as a file rather than as text: Windows spells that folder
/// with `\`, where the script appends `/review`.
fn port(path: &Path, deadlines: bool) {
    /// Whether anything changed.
    fn commands(value: &mut Value, deadlines: bool) -> bool {
        match value {
            Value::Object(object) => {
                let mut changed = false;
                for (key, value) in object.iter_mut() {
                    changed |= match value {
                        Value::String(command)
                            if key == "command"
                                && (command.starts_with("/bin/")
                                    || command.starts_with("/usr/bin/")
                                    || command == "sh") =>
                        {
                            *command = os::bin(command);
                            true
                        }
                        Value::Number(deadline)
                            if deadlines
                                && key == "timeoutMs"
                                && deadline.as_u64().is_some_and(|ms| ms >= 1000) =>
                        {
                            *value = json!(os::slow(deadline.as_u64().unwrap()));
                            true
                        }
                        other => commands(other, deadlines),
                    };
                }
                changed
            }
            Value::Array(items) => {
                let mut changed = false;
                for item in items {
                    changed |= commands(item, deadlines);
                }
                changed
            }
            _ => false,
        }
    }
    match path.file_name().and_then(|name| name.to_str()) {
        // Rewritten only when needed: a declaration's bytes can be part of a fingerprint.
        Some("artifactize.json") => {
            let mut declaration: Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
            if commands(&mut declaration, deadlines) {
                fs::write(path, serde_json::to_string_pretty(&declaration).unwrap()).unwrap();
            }
        }
        Some("check.sh") => {
            let script = fs::read_to_string(path).unwrap();
            let script = script.replace(r#"test "$PWD" = "#, r#"test "$PWD" -ef "#);
            fs::write(path, script).unwrap();
        }
        _ => {}
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
    /// When the request arrived.
    pub received: Instant,
}

pub enum Reply {
    /// A `text/event-stream` of `event: <type>` and `data: <json>` records.
    Events(Vec<Value>),
    Json(u16, Value),
    /// A status, extra headers and a raw body.
    Raw(u16, Vec<(&'static str, String)>, String),
}

type Handler = dyn Fn(&Request) -> Reply + Send + Sync;

pub struct FakeProvider {
    /// `http://127.0.0.1:PORT`, without a trailing slash.
    pub url: String,
    requests: Arc<Mutex<Vec<Request>>>,
    stop: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}

impl FakeProvider {
    /// Answer every request with `handler`, one connection at a time per thread.
    pub fn start(handler: impl Fn(&Request) -> Reply + Send + Sync + 'static) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let handler: Arc<Handler> = Arc::new(handler);
        let thread = thread::spawn({
            let requests = requests.clone();
            let stop = stop.clone();
            move || {
                while !stop.load(Ordering::SeqCst) {
                    match listener.accept() {
                        Ok((stream, _)) => {
                            let handler = handler.clone();
                            let requests = requests.clone();
                            thread::spawn(move || serve(stream, &*handler, &requests));
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            thread::sleep(Duration::from_millis(5));
                        }
                        Err(error) => panic!("fake provider: {error}"),
                    }
                }
            }
        });
        Self {
            url,
            requests,
            stop,
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
        self.stop.store(true, Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn serve(stream: TcpStream, handler: &Handler, requests: &Mutex<Vec<Request>>) {
    stream.set_nonblocking(false).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    let mut reader = BufReader::new(stream.try_clone().unwrap());
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
        reader.read_line(&mut header).unwrap();
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
    reader.read_exact(&mut body).unwrap();
    let request = Request {
        received: Instant::now(),
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
            json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}),
            json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":text}}),
            json!({"type":"content_block_stop","index":0}),
            json!({"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},
                "usage":{"output_tokens":output}}),
            json!({"type":"message_stop"}),
        ])
    }

    pub fn models(ids: &[&str]) -> Reply {
        let data: Vec<_> = ids
            .iter()
            .map(|id| json!({"id":id,"type":"model","display_name":id,"created_at":"2026-01-01T00:00:00Z"}))
            .collect();
        Reply::Json(
            200,
            json!({"data":data,"has_more":false,"first_id":null,"last_id":null}),
        )
    }
}

/// Codex replies (`POST <root>/responses`, `GET <root>/models`) and sign-in tokens.
pub mod codex {
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
            json!({"access_token":access,"refresh_token":refresh,"expires_in":3600,"id_token":"id"}),
        )
    }
}
