use std::{collections::BTreeMap, fs};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{Terminal, backend::TestBackend};
use serde_json::{Value, json};
use time::format_description::well_known::Rfc3339;

use super::*;
use crate::store::HumanClaim;

fn now() -> OffsetDateTime {
    OffsetDateTime::parse("2026-01-01T00:01:05Z", &Rfc3339).unwrap()
}

fn request(eval: &str, status: &str, extra: Value) -> RequestView {
    let mut value = json!({
        "id":format!("run-1-{eval}"),"runId":"run-1","evalId":eval,"target":eval.split('/').next(),
        "title":"Title","profile":{"kind":"runtime","command":"true","args":[]},
        "requestedProfile":{"kind":"runtime","command":"true","args":[]},
        "payload":{"instruction":"Check."},"references":{},"deps":[],"status":status,
        "createdAt":"2026-01-01T00:00:00Z","cwd":"/repo"
    });
    value
        .as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    RequestView {
        request: serde_json::from_value(value).unwrap(),
        claim: None,
        execution: None,
        definition: None,
    }
}

fn eval(id: &str, deps: &[&str]) -> Value {
    json!({"id":id,"target":id.split('/').next(),"deps":deps,
        "declaration":{"title":"Title","profile":{"kind":"agent","backend":"openai","model":"gpt-x","reasoning":"high"},"payload":{"instruction":"Look."}}})
}

fn run(definitions: Value) -> RunView {
    RunView {
        run: serde_json::from_value(json!({
            "id":"run-1","repoPath":"/repo","stateDir":"/state","status":"RUNNING",
            "createdAt":"2026-01-01T00:00:00Z","selection":{"kind":"all"},
            "definitions":definitions,"validation":null,"maxExecutions":5,"executionsStarted":2
        }))
        .unwrap(),
        requests: Vec::new(),
    }
}

fn live() -> (RunView, Vec<RequestView>) {
    let definitions = json!({
        "artifacts":{
            "lib":{"path":"lib","basis":true},"dep":{"path":"dep"},"app":{"path":""},
            "p1":{"path":"pages","family":{"name":"pages"}},"p2":{"path":"pages","family":{"name":"pages"}}
        },
        "evals":[eval("dep/check", &[]), eval("app/check", &["lib", "dep"]), eval("app/review", &[]), eval("p1/check", &[]), eval("p2/check", &[])],
        "relations":[
            {"source":"lib","target":"app","kind":"mount","alias":"shared","cyclic":false},
            {"source":"dep","target":"app","kind":"instruction","name":"dep","evalId":"app/check","cyclic":false}
        ],
        "components":[{"id":0,"artifacts":["lib"]},{"id":1,"artifacts":["dep"]},{"id":2,"artifacts":["p1"]},{"id":3,"artifacts":["p2"]},{"id":4,"artifacts":["app"],"gates":["dep/check"]}],
        "families":{"pages":{"path":"pages","artifactIds":["p1","p2"]}}
    });
    let mut waiting = request(
        "app/review",
        "WAITING_HUMAN",
        json!({"profile":{"kind":"human"}}),
    );
    waiting.claim = Some(HumanClaim {
        request_id: waiting.request.id.clone(),
        reviewer: "alice".into(),
        claimed_at: "2026-01-01T00:00:30Z".into(),
    });
    let requests = vec![
        request(
            "app/check",
            "RUNNING",
            json!({"startedAt":"2026-01-01T00:00:00Z","deps":["lib","dep"]}),
        ),
        waiting,
        request(
            "p1/check",
            "GREEN",
            json!({"startedAt":"2026-01-01T00:00:01Z","completedAt":"2026-01-01T00:00:04Z"}),
        ),
        request(
            "p2/check",
            "ERROR",
            json!({"error":"spawn failed","errorCode":"SPAWN","completedAt":"2026-01-01T00:00:02Z"}),
        ),
    ];
    let mut view = run(definitions);
    view.requests = requests.iter().map(|view| view.request.clone()).collect();
    (view, requests)
}

fn search<'a>(nodes: &'a [Node], id: &str) -> Option<&'a Node> {
    let mut nodes = nodes.iter();
    nodes.find_map(|node| {
        (node.id == id)
            .then_some(node)
            .or_else(|| search(&node.children, id))
    })
}

fn find<'a>(nodes: &'a [Node], id: &str) -> &'a Node {
    search(nodes, id).unwrap_or_else(|| panic!("{id} not in tree"))
}

#[test]
fn live_progress_tree_and_details_are_pure_projections() {
    let (view, requests) = live();
    let progress = progress(&view, &requests, now());
    assert_eq!(progress.validation, "pending");
    assert!(progress.timing.ends_with("elapsed 1m 05s"));
    assert_eq!(progress.running, [("app/check".into(), "1m 05s".into())]);
    assert_eq!(
        progress.waiting,
        [(
            "app/review".into(),
            "claimed by alice at 2026-01-01T00:00:30Z".into()
        )]
    );
    assert_eq!(
        progress.errors,
        [("p2/check".into(), "[SPAWN] spawn failed".into())]
    );
    assert!(progress.work.starts_with("executions 2/5 · jobs 4"));
    assert_eq!(
        progress.counts.iter().map(|(_, n)| n).sum::<u64>(),
        4,
        "{:?}",
        progress.counts
    );

    let nodes = tree(&view, &requests, now());
    assert_eq!(
        nodes
            .iter()
            .map(|node| node.id.as_str())
            .collect::<Vec<_>>(),
        ["a:lib", "a:dep", "f:pages", "a:app"]
    );
    let family = find(&nodes, "f:pages");
    assert_eq!(family.status.as_deref(), Some("ERROR"));
    assert_eq!(family.children.len(), 2);
    assert_eq!(find(&nodes, "a:app").status.as_deref(), Some("RUNNING"));
    assert_eq!(find(&nodes, "a:app").text, "app  RUNNING 0/2");
    assert_eq!(
        find(&nodes, "e:app/check").text,
        "check RUNNING 1m 05s  ← lib, dep"
    );
    assert_eq!(find(&nodes, "e:p1/check").text, "check GREEN 3s");
    let absent = find(&nodes, "e:dep/check");
    assert_eq!(
        (absent.status.as_deref(), absent.text.as_str()),
        (None, "check not in Run")
    );
    assert_eq!(find(&nodes, "r:lib").text, "⇐ lib (mount shared)");
    assert_eq!(find(&nodes, "r:dep").text, "⇐ dep ({dep} in app/check)");

    let absent = detail(&view, &requests, &Target::Eval("dep/check".into()), now());
    assert!(
        absent
            .field("Status")
            .unwrap()
            .starts_with("not in this Run")
    );
    assert_eq!(
        absent.field("Profile"),
        Some("agent openai gpt-x reasoning high")
    );
    let human = detail(&view, &requests, &Target::Eval("app/review".into()), now());
    assert_eq!(
        human.field("Status"),
        Some("WAITING_HUMAN — waiting for a Human submission")
    );
    assert!(
        human
            .field("Claim")
            .unwrap()
            .starts_with("claimed by alice")
    );
    assert_eq!(human.field("Identity"), Some("none (no reuse)"));
    let failed = detail(&view, &requests, &Target::Eval("p2/check".into()), now());
    assert_eq!(failed.field("Error"), Some("[SPAWN] spawn failed"));
    let app = detail(&view, &requests, &Target::Artifact("app".into()), now());
    assert_eq!(app.field("Gates"), Some("- dep/check not in Run"));
    assert_eq!(
        app.field("Inputs"),
        Some("lib — mount shared\ndep — {dep} in app/check")
    );
    let pages = detail(&view, &requests, &Target::Family("pages".into()), now());
    assert_eq!(
        pages.field("Instances"),
        Some("✓ p1 GREEN 1/1\n! p2 ERROR 0/1")
    );

    // Runs saved without definitions still expose every requested Artifact and eval.
    let mut bare = run(Value::Null);
    bare.requests = view.requests.clone();
    let nodes = tree(&bare, &requests, now());
    for request in &requests {
        find(&nodes, &format!("a:{}", request.request.target));
        find(&nodes, &format!("e:{}", request.request.eval_id));
    }
}

#[test]
fn run_rows_and_durations() {
    assert_eq!(
        [5, 65, 3_700, 90_000, -3].map(duration),
        ["5s", "1m 05s", "1h 01m", "1d 1h", "0s"]
    );
    let rows = run_rows(&[summary("run-a", 2)], now());
    assert_eq!(rows[0].age, "1m 05s");
    assert_eq!(rows[0].counts, "GREEN 2  RED 1");
}

fn summary(id: &str, green: u64) -> RunSummary {
    RunSummary {
        id: id.into(),
        repo_path: "/repo".into(),
        created_at: "2026-01-01T00:00:00Z".into(),
        completed_at: None,
        status: "RED".into(),
        counts: BTreeMap::from([("GREEN".into(), green), ("RED".into(), 1)]),
    }
}

fn screen(monitor: &mut Monitor) -> String {
    let mut terminal = Terminal::new(TestBackend::new(120, 20)).unwrap();
    terminal.draw(|frame| monitor.draw(frame)).unwrap();
    let buffer = terminal.backend().buffer();
    let rows = buffer.content().chunks(buffer.area.width as usize);
    let rows = rows.map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>());
    rows.collect::<Vec<_>>().join("\n")
}

#[tokio::test]
async fn refresh_errors_keep_last_known_data() {
    let root = tempfile::tempdir().unwrap();
    let mut monitor = Monitor::new(root.path().join("missing"), None);
    monitor.refresh().await;
    assert!(screen(&mut monitor).contains("No saved Runs."));

    let broken = root.path().join("broken");
    fs::create_dir_all(broken.join(store::DATABASE)).unwrap();
    monitor.state = broken;
    monitor.set_runs(vec![summary("run-a", 2), summary("run-b", 3)]);
    monitor.refresh().await;
    let text = screen(&mut monitor);
    assert!(text.contains("run-a") && text.contains("run-b"), "{text}");
    assert!(
        text.contains("read failed at") && text.contains("regular files"),
        "{text}"
    );
}

#[test]
fn keys_page_older_runs_open_and_quit() {
    let key = |code| KeyEvent::from(code);
    let mut monitor = Monitor::new("/state".into(), None);
    monitor.limit = 2;
    monitor.set_runs(vec![summary("run-a", 1), summary("run-b", 1)]);
    assert_eq!(monitor.key(key(KeyCode::Char('j'))), Action::None);
    assert_eq!(monitor.selected_run().unwrap().id, "run-b");
    assert_eq!(monitor.key(key(KeyCode::Down)), Action::Refresh);
    assert_eq!(monitor.limit, 2 + PAGE);
    // A refresh keeps the selected Run even when newer Runs arrive above it.
    monitor.set_runs(vec![
        summary("run-new", 1),
        summary("run-a", 1),
        summary("run-b", 1),
    ]);
    assert_eq!(monitor.selected_run().unwrap().id, "run-b");
    assert_eq!(monitor.key(key(KeyCode::Enter)), Action::Refresh);
    assert_eq!(monitor.open.as_deref(), Some("run-b"));
    assert_eq!(monitor.key(key(KeyCode::Esc)), Action::Refresh);
    assert_eq!(monitor.open, None);
    assert_eq!(monitor.key(key(KeyCode::Char('r'))), Action::Refresh);
    assert_eq!(monitor.key(key(KeyCode::Esc)), Action::Quit);
    assert_eq!(
        monitor.key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)),
        Action::Quit
    );
}

#[test]
fn run_screen_renders_progress_tree_and_detail() {
    let (view, requests) = live();
    let mut monitor = Monitor::new("/state".into(), Some("/repo".into()));
    monitor.open = Some("run-1".into());
    monitor.set_run(view, requests);
    let text = screen(&mut monitor);
    for expected in [
        "repo /repo",
        "Run run-1",
        "running app/check",
        "waiting Human app/review · claimed by alice",
        "error p2/check · [SPAWN] spawn failed",
        "◇ lib  BASIS",
        "▶ ! family pages  ERROR · 2 instances",
        "▼ ◐ app  RUNNING 0/2",
        "Artifact lib [basis]",
    ] {
        assert!(text.contains(expected), "{expected}\n{text}");
    }
    assert_eq!(monitor.target(), Some(Target::Artifact("lib".into())));
}
