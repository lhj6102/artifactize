//! Saved Agent conversations, `session show` and `session send`, and their size-bound
//! collection, against loopback fakes.

mod support;

use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use serde_json::{Value, json};
use support::{FakeProvider, Reply, Request, anthropic, codex, openai};

use artifactize::agent::FOLLOW_UP;

/// A workspace of one Artifact, `notes`, with one Agent eval whose result must list
/// `covered`, and a content fingerprint.
struct Project {
    root: tempfile::TempDir,
    repo: PathBuf,
    state: PathBuf,
}

impl Project {
    fn new(backend: &str) -> Self {
        let root = support::os::tempdir();
        let repo = root.path().join("repo");
        let folder = repo.join("notes");
        fs::create_dir_all(&folder).unwrap();
        fs::write(folder.join("notes.md"), "notes: R1 holds.\n").unwrap();
        let profile = json!({"kind":"agent","backend":backend,
            "model":format!("{backend}-model"),"timeout_ms":20000});
        support::declaration::write(
            folder.join("index.artf"),
            json!({
                "name":"notes",
                "fingerprint":{},
                "views":{"agent_tools":{"read":{"builtin":"read"}}},
                "evals":[
                    {
                        "id":"review",
                        "title":"Review",
                        "profile":profile,
                        "payload":{"instruction":"Review {notes}."},
                        "pass_schema":{
                            "type":"object",
                            "required":["covered"],
                            "properties":{"covered":{"type":"array","items":{"type":"string"}}},
                        },
                    },
                ],
            })
            .to_string(),
        )
        .unwrap();
        let state = root.path().join("state");
        let auth = state.join("auth");
        support::os::create_private_dir_all(&auth);
        let expires = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
            + 3600;
        let credentials = json!({"access_token":codex::jwt("account-1", expires),
            "refresh_token":"refresh","account_id":"account-1","expires_at":expires,"saved_at":1});
        support::os::write_private_file(&auth.join("codex.json"), credentials.to_string());
        Self { root, repo, state }
    }

    fn limits(&self, limits: Value) {
        fs::write(self.state.join("limits.json"), limits.to_string()).unwrap();
    }

    fn command(&self, provider: &FakeProvider, state: &Path, args: &[&str]) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_artifactize"));
        command
            .arg("--repo")
            .arg(&self.repo)
            .arg("--state-dir")
            .arg(state)
            .args(args)
            .env("ARTIFACTIZE_REMOTE", "off")
            .env("OPENAI_API_KEY", "fake-openai-key")
            .env("ANTHROPIC_API_KEY", "fake-anthropic-key")
            .env("ARTIFACTIZE_OPENAI_BASE_URL", provider.openai_base())
            .env("ARTIFACTIZE_ANTHROPIC_BASE_URL", provider.anthropic_base())
            .env("ARTIFACTIZE_CODEX_BASE_URL", provider.codex_base())
            .env("ARTIFACTIZE_CODEX_AUTH_URL", &provider.url)
            .env_remove("ARTIFACTIZE_CODEX_AUTH_FILE");
        command
    }

    fn run_in(&self, provider: &FakeProvider, state: &Path, args: &[&str], code: i32) -> Output {
        let output = self.command(provider, state, args).output().unwrap();
        assert_eq!(
            output.status.code(),
            Some(code),
            "{args:?}: {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        output
    }

    fn run(&self, provider: &FakeProvider, args: &[&str], code: i32) -> Output {
        self.run_in(provider, &self.state, args, code)
    }

    fn json(&self, provider: &FakeProvider, args: &[&str], code: i32) -> Value {
        let mut args = args.to_vec();
        args.push("--json");
        serde_json::from_slice(&self.run(provider, &args, code).stdout).unwrap()
    }

    fn text(&self, provider: &FakeProvider, args: &[&str], code: i32) -> String {
        let output = self.run(provider, args, code);
        format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    }

    fn sessions(&self) -> PathBuf {
        self.state.join("agent-sessions")
    }

    fn events(&self, session: &str) -> Vec<Value> {
        fs::read_to_string(self.sessions().join(format!("{session}.jsonl")))
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }
}

fn input(request: &Request) -> Vec<Value> {
    request.body["input"]
        .as_array()
        .cloned()
        .unwrap_or_default()
}

fn text_of(item: &Value) -> &str {
    item["content"][0]["text"].as_str().unwrap_or_default()
}

/// How many follow-ups the request carries, and whether a tool already answered the last.
fn follow_ups(request: &Request) -> (usize, bool) {
    let input = input(request);
    let asked: Vec<_> = input
        .iter()
        .enumerate()
        .filter(|(_, item)| text_of(item).starts_with(FOLLOW_UP))
        .map(|(index, _)| index)
        .collect();
    let answered = asked.last().is_some_and(|last| {
        input[*last..]
            .iter()
            .any(|item| item["type"] == "function_call_output")
    });
    (asked.len(), answered)
}

fn replayed(request: &Request) -> bool {
    input(request)
        .iter()
        .any(|item| item["type"] == "function_call_output")
}

fn repairing(request: &Request) -> bool {
    input(request)
        .last()
        .is_some_and(|item| text_of(item).starts_with("Your final response did not match"))
}

fn responses(request: &Request, output: Vec<Value>) -> Reply {
    if request.path.starts_with("/backend-api/codex") {
        codex::completed(request, output, openai::usage(10, 2))
    } else {
        openai::completed(request, output, openai::usage(10, 2))
    }
}

/// The review reasons and reads the notes, answers without `covered`, then repairs. The
/// first follow-up reads the notes again before it answers; later ones answer at once.
fn reply(request: &Request) -> Reply {
    if request.path == "/v1/messages" {
        let text = if request.body.to_string().contains(FOLLOW_UP) {
            "Anthropic: R1 holds."
        } else {
            r#"{"verdict":"GREEN","covered":["R1"]}"#
        };
        return anthropic::text(request, text, 10, 2);
    }
    let tool = request.body["tools"][0]["name"]
        .as_str()
        .unwrap_or_default();
    let read = |id: &str| openai::function_call(id, tool, &json!({"path":"notes.md"}));
    let output = match follow_ups(request) {
        (1, false) => vec![
            openai::reasoning("rs_f1", "Rereading the notes", "enc-send-1"),
            read("call_f1"),
        ],
        (1, true) => vec![openai::message("Because R1 holds.")],
        (2, _) => vec![openai::message("Still GREEN.")],
        (count, _) if count > 2 => vec![openai::message("Noted.")],
        _ if repairing(request) => vec![openai::message(r#"{"verdict":"GREEN","covered":["R1"]}"#)],
        _ if replayed(request) => vec![openai::message(r#"{"verdict":"GREEN"}"#)],
        _ => vec![
            openai::reasoning("rs_1", "Reading the notes", "enc-review-1"),
            read("call_1"),
        ],
    };
    responses(request, output)
}

fn roles(events: &[Value]) -> Vec<String> {
    events
        .iter()
        .filter(|event| event["kind"] == "message")
        .map(|event| {
            let message = &event["message"];
            let role = message["role"].as_str().unwrap().to_owned();
            if event["repair"] == true {
                "repair".into()
            } else if message["content"][0]["type"] == "toolresult" {
                "tool".into()
            } else {
                role
            }
        })
        .collect()
}

#[test]
fn a_review_saves_its_conversation_and_follow_ups_continue_it() {
    for backend in ["openai", "codex"] {
        let project = Project::new(backend);
        let provider = FakeProvider::start(reply);
        let run = project.json(&provider, &["verify", "--all"], 0);
        let review = &run["requests"][0];
        assert_eq!(review["status"], "GREEN", "{run}");
        let session = review["sessionId"].as_str().unwrap().to_owned();
        let id = review["id"].as_str().unwrap().to_owned();
        let reference = review["session"]["ref"].as_str().unwrap().to_owned();
        assert_eq!(review["session"]["sessionId"], session.as_str());
        assert_eq!(review["session"]["requestId"], id.as_str());
        assert_eq!(review["session"]["runId"], run["id"]);
        assert!(reference.ends_with(&format!("/{}/{id}/{session}", run["id"].as_str().unwrap())));
        assert_eq!(provider.requests().len(), 3, "{backend}");

        // The whole conversation, encrypted reasoning, the tool call and the repair included.
        let events = project.events(&session);
        let header = &events[0];
        assert_eq!(header["kind"], "review");
        assert_eq!(header["sessionId"], session.as_str());
        assert_eq!(header["requestId"], id.as_str());
        assert_eq!(header["evalId"], "notes/review");
        assert_eq!(header["backend"], backend);
        assert_eq!(header["model"], format!("{backend}-model"));
        assert_eq!(header["parameters"]["prompt_cache_key"], session.as_str());
        assert_eq!(header["budgets"]["timeoutMs"], 20000);
        assert_eq!(header["tools"][0]["name"], "read_notes");
        assert_eq!(header["state"], review["session"]["state"]);
        assert_eq!(
            roles(&events),
            [
                "system",
                "user",
                "assistant",
                "tool",
                "assistant",
                "repair",
                "assistant"
            ],
            "{backend}"
        );
        let messages: Vec<_> = events
            .iter()
            .filter(|event| event["kind"] == "message")
            .collect();
        // The system prompt says from the start how a later follow-up is answered.
        let system = messages[0]["message"]["content"].as_str().unwrap();
        assert!(system.starts_with(
            "Follow the artifactize review instructions. For the review itself, return only one JSON object matching the schema for its verdict."
        ));
        assert!(system.ends_with(
            "If a person later asks a follow-up question about this review, answer that question in plain text instead, not JSON; the verdict stays as recorded."
        ));
        let reasoning = &messages[2]["message"]["content"][0];
        assert_eq!(reasoning["type"], "reasoning");
        assert_eq!(reasoning["id"], "rs_1");
        assert!(
            reasoning.to_string().contains("enc-review-1"),
            "{reasoning}"
        );
        assert_eq!(messages[2]["message"]["content"][1]["type"], "toolcall");
        assert!(messages[3].to_string().contains("notes: R1 holds."));
        // The answer the repair replaced, and the prompt that asked for it.
        assert!(
            messages[4]
                .to_string()
                .contains(r#"{\"verdict\":\"GREEN\"}"#)
        );
        assert!(
            text_of(&messages[5]["message"])
                .starts_with("Your final response did not match the required schema")
        );
        assert_eq!(
            (messages[2]["turn"].clone(), messages[5]["turn"].clone()),
            (json!(1), json!(3))
        );
        let end = events.last().unwrap();
        assert_eq!(end["kind"], "end");
        assert_eq!(end["result"], json!({"verdict":"GREEN","covered":["R1"]}));
        {
            assert!(support::os::private_dir(&project.sessions()));
            assert!(support::os::private_file(
                &project.sessions().join(format!("{session}.jsonl"))
            ));
        }

        // show: text, and the raw events with --json, by any form of the reference.
        let shown = project.text(&provider, &["session", "show", &id], 0);
        for expected in [
            format!("Session {session} · {backend} {backend}-model"),
            format!("Reference: {reference}"),
            "── System".into(),
            "[reasoning] Reading the notes".into(),
            r#"[tool call read_notes] {"path":"notes.md"}"#.into(),
            "[tool result read_notes]".into(),
            "── Repair prompt (turn 3)".into(),
            "── Result: {".into(),
        ] {
            assert!(shown.contains(&expected), "{expected} in {shown}");
        }
        for form in [&id, &session, &reference] {
            let raw = project.json(&provider, &["session", "show", form], 0);
            assert_eq!(raw["events"], json!(events));
            assert_eq!(raw["reference"], reference.as_str());
        }

        // A follow-up: same session and cache key, the tools, the exact replay.
        let before = project.json(&provider, &["request", "show", &id], 0);
        let answer = project.json(&provider, &["session", "send", &id, "Why GREEN?"], 0);
        assert_eq!(answer["answer"], "Because R1 holds.");
        assert_eq!(answer["send"], 1);
        assert_eq!(answer["sessionId"], session.as_str());
        assert_eq!(answer["filesChanged"], false);
        assert!(answer.get("toolCalls").is_none());
        assert_eq!(answer["usage"].as_array().unwrap().len(), 2);
        let calls = provider.requests();
        assert_eq!(calls.len(), 5, "{backend}");
        for call in &calls[3..] {
            assert_eq!(call.body["prompt_cache_key"], session.as_str());
            assert_eq!(call.body["model"], format!("{backend}-model"));
            assert_eq!(call.body["tools"][0]["name"], "read_notes");
            if backend == "codex" {
                assert_eq!(call.headers["session-id"], session);
            }
            let body = call.body.to_string();
            assert!(body.contains("enc-review-1"), "{body}");
            assert!(body.contains("Why GREEN?"));
        }
        assert!(calls[4].body.to_string().contains("enc-send-1"));
        // Only a framed question is added: the system prompt and the history the repair
        // turn sent, and the answer to it, are the prefix the cache already holds.
        let (review, follow) = (input(&calls[2]), input(&calls[3]));
        // The instructions every request sends are the review's system prompt, byte for byte.
        for call in &calls {
            assert_eq!(call.body["instructions"], calls[0].body["instructions"]);
        }
        if backend == "codex" {
            assert_eq!(calls[3].body["instructions"], system);
        }
        assert_eq!(follow[..review.len()], review[..], "{backend}");
        assert_eq!(follow.len(), review.len() + 2);
        assert_eq!(
            text_of(follow.last().unwrap()),
            format!("{FOLLOW_UP}\n\nQuestion:\nWhy GREEN?")
        );
        assert!(FOLLOW_UP.contains("This is not a new review") && FOLLOW_UP.contains("not JSON"));

        // A second follow-up continues the same thread.
        let text = project.text(&provider, &["session", "send", &session, "Still?"], 0);
        assert!(
            text.starts_with(&format!(
                "Session {session} · follow-up 2 · {backend} {backend}-model\n\nStill GREEN."
            )),
            "{text}"
        );
        assert!(!text.contains("files changed"));
        let second = provider.requests()[5].body.to_string();
        for earlier in ["Why GREEN?", "Because R1 holds.", "enc-send-1", "Still?"] {
            assert!(second.contains(earlier), "{earlier} in {second}");
        }
        let recorded = project.events(&session);
        assert!(recorded.iter().any(|event| event["kind"] == "delivery"));
        // Display-only delivery must not alter authoritative replay/message order.
        let events = recorded
            .into_iter()
            .filter(|event| event["kind"] != "delivery")
            .collect::<Vec<_>>();
        let end = events
            .iter()
            .position(|event| event["kind"] == "end")
            .unwrap();
        let kinds: Vec<_> = events[end + 1..]
            .iter()
            .map(|event| {
                (
                    event["kind"].as_str().unwrap(),
                    event["send"].as_u64().unwrap(),
                )
            })
            .collect();
        assert_eq!(
            kinds,
            [
                ("send", 1),
                ("message", 1),
                ("attempt", 1),
                ("message", 1),
                ("message", 1),
                ("attempt", 1),
                ("message", 1),
                ("answer", 1),
                ("send", 2),
                ("message", 2),
                ("attempt", 2),
                ("message", 2),
                ("answer", 2)
            ]
        );
        // The follow-up's tool result, and whether it failed.
        assert_eq!(events[end + 5]["isError"], json!([false]));
        // The file keeps the framing and the person's own words; show prints the words.
        assert_eq!(
            (&events[end + 1]["text"], &events[end + 1]["framing"]),
            (&json!("Why GREEN?"), &json!(FOLLOW_UP))
        );
        assert_eq!(events[end + 2]["question"], "Why GREEN?");
        assert_eq!(
            text_of(&events[end + 2]["message"]),
            format!("{FOLLOW_UP}\n\nQuestion:\nWhy GREEN?")
        );
        let shown = project.text(&provider, &["session", "show", &id], 0);
        for expected in [
            "══ Follow-up 2",
            "── Person (turn 1, follow-up 1)\nWhy GREEN?\n",
            "── Person (turn 1, follow-up 2)\nStill?\n",
        ] {
            assert!(shown.contains(expected), "{expected} in {shown}");
        }
        assert!(!shown.contains(FOLLOW_UP), "{shown}");

        // The summary, from the session file alone: three review turns (the call, the
        // answer and its repair), two turns of the first follow-up and one of the second.
        let summary = project.json(&provider, &["session", "show", &id, "--summary"], 0);
        assert_eq!(summary["sessionId"], session.as_str());
        assert_eq!(summary["reference"], reference.as_str());
        assert_eq!(
            (&summary["backend"], &summary["followUps"]),
            (&json!(backend), &json!(2))
        );
        let turns: Vec<_> = summary["turns"]
            .as_array()
            .unwrap()
            .iter()
            .map(|turn| (turn["followUp"].as_u64(), turn["turn"].as_u64().unwrap()))
            .collect();
        assert_eq!(
            turns,
            [
                (None, 1),
                (None, 2),
                (None, 3),
                (Some(1), 1),
                (Some(1), 2),
                (Some(2), 1)
            ]
        );
        assert_eq!(summary["turns"][0]["tokens"]["inputTokens"], 10);
        assert_eq!(summary["tokens"]["inputTokens"], 60);
        assert_eq!(summary["tokens"]["outputTokens"], 12);
        assert_eq!(
            summary["toolCalls"],
            json!({"read_notes":{"calls":2,"failed":0}})
        );
        assert!(summary["durationMs"].is_u64());
        let shown = project.text(&provider, &["session", "show", &session, "--summary"], 0);
        for expected in [
            format!("Session {session} · {backend} {backend}-model\n"),
            "Turns: 6 · follow-ups: 2\n".into(),
            "Tokens: input 60 · output 12 · cache read 0 · reasoning 0\n".into(),
            "  turn 1: input 10 · output 2 · cache read 0 · reasoning 0\n".into(),
            "  follow-up 2, turn 1: input 10 · output 2 · cache read 0 · reasoning 0\n".into(),
            "Tool calls: 2\n  read_notes: 2\n".into(),
        ] {
            assert!(shown.contains(&expected), "{expected} in {shown}");
        }

        // The recorded review and its reuse never change.
        let after = project.json(&provider, &["request", "show", &id], 0);
        for field in ["status", "result", "usage", "completedAt"] {
            assert_eq!(after[field], before[field], "{field}");
        }
        let calls = provider.requests().len();
        let rerun = project.json(&provider, &["verify", "--all"], 0);
        assert_eq!(rerun["requests"][0]["source"]["requestId"], id.as_str());
        assert_eq!(provider.requests().len(), calls);
        // The reusing request names the same conversation.
        let reused = rerun["requests"][0]["id"].as_str().unwrap();
        assert_eq!(rerun["requests"][0]["session"]["ref"], reference.as_str());
        let shown = project.text(&provider, &["session", "show", reused], 0);
        assert!(shown.contains(&format!("Request {reused} reused this review's result.")));

        // Once the reviewed files change, the answer says so.
        fs::write(project.repo.join("notes/notes.md"), "notes: R1 fails.\n").unwrap();
        let text = project.text(&provider, &["session", "send", &id, "And now?"], 0);
        assert!(
            text.starts_with(&format!(
                "Session {session} · follow-up 3 · {backend} {backend}-model · files changed since this review\n\nNoted."
            )),
            "{text}"
        );
        assert!(
            provider
                .requests()
                .last()
                .unwrap()
                .body
                .to_string()
                .contains("The Artifact files changed since this review")
        );
    }
}

#[test]
fn anthropic_reviews_are_saved_and_continued_without_a_cache_key() {
    let project = Project::new("anthropic");
    let provider = FakeProvider::start(reply);
    let run = project.json(&provider, &["verify", "--all"], 0);
    let id = run["requests"][0]["id"].as_str().unwrap();
    let session = run["requests"][0]["sessionId"].as_str().unwrap();
    assert_eq!(
        roles(&project.events(session)),
        ["system", "user", "assistant"]
    );
    let answer = project.json(&provider, &["session", "send", id, "Why?"], 0);
    assert_eq!(answer["answer"], "Anthropic: R1 holds.");
    let call = provider.requests().pop().unwrap();
    assert!(call.body.get("prompt_cache_key").is_none());
    assert!(call.body.to_string().contains(r#"{\"verdict\":\"GREEN\""#));
}

#[test]
fn concurrent_sends_take_turns_in_one_thread() {
    let project = Project::new("openai");
    let gate = support::Gate::default();
    let (entered, waiting) = std::sync::mpsc::channel();
    let in_flight = Arc::new(AtomicUsize::new(0));
    let most = Arc::new(AtomicUsize::new(0));
    let provider = FakeProvider::start({
        let gate = gate.clone();
        let (in_flight, most) = (in_flight.clone(), most.clone());
        move |request| {
            let asked: Vec<_> = input(request)
                .iter()
                .filter(|item| text_of(item).starts_with(FOLLOW_UP))
                .map(|item| {
                    text_of(item)
                        .rsplit("Question:\n")
                        .next()
                        .unwrap()
                        .to_owned()
                })
                .collect();
            let Some(last) = asked.last() else {
                return responses(
                    request,
                    vec![openai::message(r#"{"verdict":"GREEN","covered":["R1"]}"#)],
                );
            };
            let now = in_flight.fetch_add(1, Ordering::SeqCst) + 1;
            most.fetch_max(now, Ordering::SeqCst);
            entered.send(()).unwrap();
            gate.wait();
            in_flight.fetch_sub(1, Ordering::SeqCst);
            responses(request, vec![openai::message(&format!("answer to {last}"))])
        }
    });
    let run = project.json(&provider, &["verify", "--all"], 0);
    let id = run["requests"][0]["id"].as_str().unwrap().to_owned();
    let session = run["requests"][0]["sessionId"].as_str().unwrap().to_owned();
    let spawned: Vec<_> = ["first", "second"]
        .into_iter()
        .map(|message| {
            project
                .command(
                    &provider,
                    &project.state,
                    &["session", "send", &id, message],
                )
                .stdout(Stdio::piped())
                .spawn()
                .unwrap()
        })
        .collect();
    waiting
        .recv_timeout(support::os::patience(Duration::from_secs(20)))
        .unwrap();
    gate.open();
    for child in spawned {
        assert!(child.wait_with_output().unwrap().status.success());
    }
    assert_eq!(most.load(Ordering::SeqCst), 1);
    // The second send continues the first, whichever started first.
    let events = project.events(&session);
    let answers: Vec<_> = events
        .iter()
        .filter(|event| event["kind"] == "answer")
        .map(|event| (event["send"].as_u64().unwrap(), event["text"].clone()))
        .collect();
    assert_eq!(
        answers.iter().map(|(send, _)| *send).collect::<Vec<_>>(),
        [1, 2]
    );
    let last = provider.requests().last().unwrap().body.to_string();
    assert!(last.contains(answers[0].1.as_str().unwrap()), "{last}");
}

#[test]
fn the_session_store_is_collected_oldest_first_down_to_its_target() {
    let project = Project::new("openai");
    let release = support::Gate::default();
    release.open();
    let provider = FakeProvider::start({
        let release = release.clone();
        move |request| {
            release.wait();
            responses(
                request,
                vec![openai::message(r#"{"verdict":"GREEN","covered":["R1"]}"#)],
            )
        }
    });
    let mut reviews = Vec::new();
    for force in [false, true, true] {
        let args: &[&str] = if force {
            &["verify", "--all", "--force"]
        } else {
            &["verify", "--all"]
        };
        let run = project.json(&provider, args, 0);
        reviews.push((
            run["requests"][0]["id"].as_str().unwrap().to_owned(),
            run["requests"][0]["sessionId"].as_str().unwrap().to_owned(),
        ));
    }
    let size = |session: &str| {
        fs::metadata(project.sessions().join(format!("{session}.jsonl")))
            .unwrap()
            .len()
    };
    let sizes: Vec<_> = reviews.iter().map(|(_, session)| size(session)).collect();
    let total: u64 = sizes.iter().sum();
    // Down to the largest session, two of the three go, whichever are oldest: the order
    // itself is the store's unit test, on given write times.
    let target = *sizes.iter().max().unwrap();
    let doctor = project.json(&provider, &["doctor"], 0);
    let check = doctor["checks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|check| check["name"] == "sessions")
        .unwrap()
        .clone();
    assert_eq!(check["details"]["sessions"], 3);
    assert_eq!(check["details"]["bytes"], total);

    // At the maximum nothing goes; one byte over, the oldest go down to the target.
    project.limits(json!({"agentSessions":{"maxBytes":total,"targetBytes":target}}));
    let pruned = project.json(&provider, &["prune"], 0);
    assert_eq!(pruned["removedSessions"], json!([]));
    project.limits(json!({"agentSessions":{"maxBytes":total - 1,"targetBytes":target}}));
    let dry = project.json(&provider, &["prune", "--dry-run"], 0);
    let doomed: Vec<_> = dry["wouldRemoveSessions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|session| session.as_str().unwrap().to_owned())
        .collect();
    assert_eq!(doomed.len(), 2, "{dry}");
    let [removed, survivor] = [true, false].map(|gone| {
        reviews
            .iter()
            .filter(|(_, session)| doomed.contains(session) == gone)
            .cloned()
            .collect::<Vec<_>>()
    });
    assert_eq!((removed.len(), survivor.len()), (2, 1));
    let survivor = survivor[0].1.clone();
    let file = |session: &str| project.sessions().join(format!("{session}.jsonl"));
    assert!(file(&removed[0].1).exists());
    let pruned = project.json(&provider, &["prune"], 0);
    assert_eq!(pruned["removedSessions"], dry["wouldRemoveSessions"]);
    assert!(!file(&removed[0].1).exists() && !file(&removed[1].1).exists());
    assert!(file(&survivor).exists());
    for form in [&removed[0].0, &removed[1].1] {
        let error = project.json(&provider, &["session", "show", form], 2);
        assert!(
            error["error"]
                .as_str()
                .unwrap()
                .contains("was removed by the session GC"),
            "{error}"
        );
    }

    // A running review's session survives any collection; verify collects at its end.
    project.limits(json!({"agentSessions":{"maxBytes":1,"targetBytes":0}}));
    release.close();
    let running = project
        .command(
            &provider,
            &project.state,
            &["verify", "--all", "--force", "--json"],
        )
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + support::os::patience(Duration::from_secs(20));
    let session = loop {
        let found = fs::read_dir(project.sessions())
            .unwrap()
            .filter_map(|entry| {
                let name = entry.unwrap().file_name().into_string().unwrap();
                name.strip_suffix(".jsonl").map(str::to_owned)
            })
            .find(|session| *session != survivor);
        if let Some(session) = found {
            break session;
        }
        assert!(Instant::now() < deadline, "the review never started");
        thread::sleep(Duration::from_millis(20));
    };
    let pruned = project.json(&provider, &["prune"], 0);
    assert_eq!(pruned["removedSessions"], json!([&survivor]));
    assert!(project.sessions().join(format!("{session}.jsonl")).exists());
    release.open();
    let output = running.wait_with_output().unwrap();
    let run: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(run["requests"][0]["sessionId"], session.as_str());
    assert_eq!(run["requests"][0]["status"], "GREEN");
    assert!(!project.sessions().join(format!("{session}.jsonl")).exists());

    // Saving off: the review keeps its session id, and nothing is saved.
    project.limits(json!({"agentSessions":{"enabled":false}}));
    let run = project.json(&provider, &["verify", "--all", "--force"], 0);
    let request = &run["requests"][0];
    assert!(request["sessionId"].is_string());
    assert!(request.get("session").is_none(), "{request}");
    assert_eq!(fs::read_dir(project.sessions()).unwrap().count(), 0);
    let id = request["id"].as_str().unwrap();
    for command in [
        &["session", "show", id][..],
        &["session", "send", id, "Why?"],
    ] {
        let error = project.json(&provider, command, 2);
        assert!(
            error["error"].as_str().unwrap().contains("was not saved"),
            "{error}"
        );
    }

    // limits.json bounds are validated before a Run starts, and doctor reports them.
    project.limits(json!({"agentSessions":{"maxBytes":10,"targetBytes":10}}));
    let error = project.json(&provider, &["verify", "--all"], 2);
    assert!(
        error["error"]
            .as_str()
            .unwrap()
            .contains("agentSessions.targetBytes (10) must be lower than maxBytes (10)."),
        "{error}"
    );
    let doctor = project.json(&provider, &["doctor"], 1);
    assert!(
        doctor["checks"]
            .as_array()
            .unwrap()
            .iter()
            .any(|check| check["name"] == "limits" && check["status"] == "FAIL")
    );
}

#[tokio::test]
async fn a_reference_elsewhere_names_where_its_session_lives() {
    use artifactize::{cache, remote::Record, store::Receipts};

    let project = Project::new("openai");
    let provider = FakeProvider::start(reply);
    let run = project.json(&provider, &["verify", "--all"], 0);
    let review = &run["requests"][0];
    let reference = review["session"]["ref"].as_str().unwrap().to_owned();
    let state_id = review["session"]["state"].as_str().unwrap().to_owned();
    let producer = review["session"]["producer"].as_str().unwrap().to_owned();
    let lives = |error: &Value, producer: &str| {
        let error = error["error"].as_str().unwrap();
        assert!(
            error.contains(&format!("lives in {producer}'s state {state_id}")),
            "{error}"
        );
    };

    // Another state on this machine, and another machine.
    let other = project.root.path().join("other");
    let output = project.run_in(
        &provider,
        &other,
        &["session", "show", &reference, "--json"],
        2,
    );
    lives(&serde_json::from_slice(&output.stdout).unwrap(), &producer);
    let elsewhere = reference.replacen(&producer, "alice@laptop", 1);
    lives(
        &project.json(&provider, &["session", "send", &elsewhere, "Why?"], 2),
        "alice@laptop",
    );

    // A record published to the team store carries the reference, never the conversation.
    let key = review["key"].as_str().unwrap();
    let execution = cache::show(&project.state, key, false)
        .await
        .unwrap()
        .pop()
        .unwrap();
    let mut record = Record::new(&execution, false).unwrap();
    let text = serde_json::to_string(&record).unwrap();
    assert!(text.contains(&format!(
        r#""sessionId":"{}""#,
        review["sessionId"].as_str().unwrap()
    )));
    for leaked in ["enc-review-1", "notes: R1 holds", "Follow the artifactize"] {
        assert!(!text.contains(leaked), "{leaked} in {text}");
    }
    record
        .producer
        .as_mut()
        .unwrap()
        .session
        .as_mut()
        .unwrap()
        .producer = "alice@laptop".into();
    record.publisher = Some("alice".parse().unwrap());
    record.published_at = Some("2026-10-06T00:00:00Z".parse().unwrap());
    let receipts = Receipts::open(&other, &project.repo).await.unwrap();
    receipts
        .mirror_execution(&record.mirror("https://reviews.example/").unwrap())
        .await
        .unwrap()
        .unwrap();
    let output = project.run_in(&provider, &other, &["verify", "--all", "--json"], 0);
    let consumer: Value = serde_json::from_slice(&output.stdout).unwrap();
    let reused = &consumer["requests"][0];
    assert_eq!(reused["source"]["kind"], "remote");
    assert_eq!(reused["session"]["producer"], "alice@laptop");
    let output = project.run_in(
        &provider,
        &other,
        &["session", "show", reused["id"].as_str().unwrap(), "--json"],
        2,
    );
    lives(
        &serde_json::from_slice(&output.stdout).unwrap(),
        "alice@laptop",
    );
}
