//! A fake model provider on loopback for end-to-end Agent tests. It answers whatever
//! its handler scripts, in the OpenAI Responses and Anthropic Messages wire formats,
//! and records every request. Point a backend at it with its
//! `ARTIFACTIZE_<BACKEND>_BASE_URL` test endpoint.
#![allow(
    dead_code,
    reason = "each test binary uses a different part of the fake"
)]

use std::{
    collections::{BTreeMap, VecDeque},
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};

use serde_json::{Value, json};

#[derive(Clone, Debug)]
pub struct Request {
    pub method: String,
    pub path: String,
    /// Lowercase names.
    pub headers: BTreeMap<String, String>,
    /// The JSON body, or null when there is none.
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
        method,
        path,
        headers,
        body: if body.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&body).unwrap()
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

    pub fn usage(input: u64, output: u64) -> Value {
        json!({"input_tokens":input,"output_tokens":output,"total_tokens":input + output,
            "input_tokens_details":{"cached_tokens":0},
            "output_tokens_details":{"reasoning_tokens":0}})
    }

    /// A streamed, completed response for the requested model.
    pub fn completed(request: &Request, output: Vec<Value>, usage: Value) -> Reply {
        let mut events = Vec::new();
        for (index, item) in output.iter().enumerate() {
            if item["type"] == "function_call" {
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
