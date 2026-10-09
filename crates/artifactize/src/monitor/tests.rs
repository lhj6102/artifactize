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

pub(crate) fn request(eval: &str, status: &str, extra: Value) -> RequestView {
    let mut value = json!({
        "id":format!("run-1-{}", eval.replace('/', "-")),
        "runId":"run-1",
        "evalId":eval,
        "target":eval.split('/').next(),
        "title":"Title",
        "profile":{"kind":"runtime","command":"true","args":[]},
        "requestedProfile":{"kind":"runtime","command":"true","args":[]},
        "evalDefHash":"hash",
        "payload":{"instruction":"Check."},
        "references":{},
        "deps":[],
        "status":status,
        "createdAt":"2026-01-01T00:00:00Z",
        "cwd":"/repo",
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
    json!({
        "id":id,
        "target":id.split('/').next(),
        "deps":deps,
        "declaration":{
            "title":"Title",
            "profile":{"kind":"agent","backend":"openai","model":"gpt-x","reasoning":"high"},
            "payload":{"instruction":"Look."},
        },
    })
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

pub(super) fn live() -> (RunView, Vec<RequestView>) {
    let definitions = json!({
        "artifacts":{
            "lib":{"path":"lib","basis":true},
            "dep":{"path":"dep"},
            "app":{"path":""},
            "p1":{"path":"pages/one"},
            "p2":{"path":"pages/two"},
        },
        "evals":[
            eval("dep/check", &[]),
            eval("app/check", &["lib", "dep"]),
            eval("app/review", &[]),
            eval("p1/check", &[]),
            eval("p2/check", &[]),
        ],
        "relations":[
            {"source":"lib","target":"app","kind":"mount","alias":"shared","cyclic":false},
            {
                "source":"dep",
                "target":"app",
                "kind":"instruction",
                "name":"dep",
                "evalId":"app/check",
                "cyclic":false,
            },
        ],
        "components":[
            {"id":0,"artifacts":["lib"]},
            {"id":1,"artifacts":["dep"]},
            {"id":2,"artifacts":["p1"]},
            {"id":3,"artifacts":["p2"]},
            {"id":4,"artifacts":["app"],"gates":["dep/check"]},
        ],
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
            json!({
                "error":"spawn failed",
                "errorCode":"SPAWN",
                "completedAt":"2026-01-01T00:00:02Z",
            }),
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
    assert_eq!(progress.budget, "executions 2/5 · jobs 4");
    assert_eq!(progress.work, "executed 0 · reused 0 · derived 0");
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
        ["a:lib", "a:dep", "a:p1", "a:p2", "a:app"]
    );
    let lines: Vec<_> = ["a:lib", "a:dep", "e:dep/check", "a:p1", "e:p1/check"]
        .into_iter()
        .chain(["a:p2", "e:p2/check", "a:app", "e:app/check", "e:app/review"])
        .map(|id| find(&nodes, id).line())
        .collect();
    assert_eq!(
        lines,
        [
            "◇ lib  basis",
            "- dep  not in this Run  0/1",
            "- check  not in this Run",
            "✓ p1  1/1",
            "✓ check  GREEN  3s",
            "! p2  ERROR: check  0/1",
            "! check  ERROR · [SPAWN] spawn failed  2s",
            "◐ app  in progress: check, review  0/2",
            "◐ check  running · runtime true  1m 05s",
            "? review  Human sign-off · claimed by alice  1m 05s",
        ]
    );
    // One row per eval: no relation rows, no `← deps`.
    assert!(search(&nodes, "r:dep").is_none());
    let app = find(&nodes, "e:app/check");
    assert_eq!(
        app.upstream
            .iter()
            .map(|up| (up.artifact.as_str(), up.completion))
            .collect::<Vec<_>>(),
        [
            ("dep", Completion::Waiting { blocked: false }),
            ("lib", Completion::Complete)
        ]
    );

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
    assert_eq!(human.field("Fingerprint"), Some("none"));
    assert_eq!(human.field("Key"), Some("none (no reuse)"));
    let failed = detail(&view, &requests, &Target::Eval("p2/check".into()), now());
    assert_eq!(failed.field("Error"), Some("[SPAWN] spawn failed"));
    let app = detail(&view, &requests, &Target::Artifact("app".into()), now());
    assert_eq!(app.field("Gates"), Some("- dep/check not in Run"));
    assert_eq!(
        app.field("Inputs"),
        Some("lib — mount shared\ndep — {dep} in app/check")
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
    let mut finished = summary("run-b", 0);
    finished.completed_at = Some("2026-01-01T00:00:42Z".into());
    let rows = run_rows(&[summary("run-a", 2), finished], now());
    assert_eq!(rows[0].age, "1m 05s");
    // Runs show the same glyphs as the tree, most urgent first and without zero counts.
    assert_eq!(rows[0].counts, "✗1 ✓2");
    assert_eq!((rows[0].took.as_str(), rows[1].took.as_str()), ("", "42s"));
    assert_eq!(rows[1].counts, "✗1");
}

pub(super) fn summary(id: &str, green: u64) -> RunSummary {
    RunSummary {
        id: id.parse().unwrap(),
        repo_path: "/repo".into(),
        created_at: "2026-01-01T00:00:00Z".into(),
        completed_at: None,
        status: crate::types::RunStatus::Red,
        counts: BTreeMap::from([
            (crate::types::RequestStatus::Green, green),
            (crate::types::RequestStatus::Red, 1),
        ]),
    }
}

fn screen(monitor: &mut Monitor) -> String {
    sized(monitor, 220, 40)
}

pub(super) fn sized(monitor: &mut Monitor, width: u16, height: u16) -> String {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
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
    assert_eq!(monitor.key(key(KeyCode::Char('j'))), Action::Refresh);
    assert_eq!(monitor.selected_run().unwrap().id.as_str(), "run-b");
    assert_eq!(monitor.key(key(KeyCode::Down)), Action::Refresh);
    assert_eq!(monitor.limit, 2 + PAGE);
    // A refresh keeps the selected Run even when newer Runs arrive above it.
    monitor.set_runs(vec![
        summary("run-new", 1),
        summary("run-a", 1),
        summary("run-b", 1),
    ]);
    assert_eq!(monitor.selected_run().unwrap().id.as_str(), "run-b");
    assert_eq!(monitor.key(key(KeyCode::Enter)), Action::Refresh);
    assert_eq!(monitor.open.as_deref(), Some("run-b"));
    assert_eq!(monitor.key(key(KeyCode::Esc)), Action::None);
    assert_eq!(monitor.focus, Pane::Runs);
    assert_eq!(monitor.key(key(KeyCode::Char('r'))), Action::Refresh);
    // Esc steps back one level and never quits, even on the first level.
    for pane in [Pane::Repositories, Pane::Repositories] {
        assert_eq!(monitor.key(key(KeyCode::Esc)), Action::None);
        assert_eq!(monitor.focus, pane);
    }
    assert_eq!(monitor.key(key(KeyCode::Char('q'))), Action::Quit);
    assert_eq!(
        monitor.key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)),
        Action::Quit
    );
}

#[tokio::test]
async fn artifact_details_display_saved_tags_and_tolerate_untagged_snapshots() {
    let view = run(json!({"artifacts":{
        "app":{"path":"app","tags":["type:image","scope:combat"]},
        "old":{"path":"old"},
        "empty":{"path":"empty","tags":[]}
    }}));
    let app = detail(&view, &[], &Target::Artifact("app".into()), now());
    assert_eq!(app.field("Tags"), Some("type:image, scope:combat"));
    for id in ["old", "empty"] {
        assert_eq!(
            detail(&view, &[], &Target::Artifact(id.into()), now()).field("Tags"),
            None
        );
    }
    let mut monitor = Monitor::new("/state".into(), Some("/repo".into()));
    monitor.open = Some("run-1".parse().unwrap());
    monitor.set_run(view, Vec::new());
    monitor.tree.select(vec!["a:app".into()]);
    monitor.open_detail().await;
    assert_eq!(monitor.focus, Pane::Detail);
    // Tags are Technical: folded, but named, until `t` shows them.
    let text = screen(&mut monitor);
    assert!(text.contains("▸ Technical  Tags"), "{text}");
    assert!(!text.contains("type:image, scope:combat"), "{text}");
    monitor.key(KeyEvent::from(KeyCode::Char('t')));
    let text = screen(&mut monitor);
    assert!(text.contains("type:image, scope:combat"), "{text}");
}

#[test]
fn run_screen_renders_progress_tree_and_detail() {
    let (view, requests) = live();
    let mut monitor = Monitor::new("/state".into(), Some("/repo".into()));
    monitor.open = Some("run-1".parse().unwrap());
    monitor.set_run(view, requests);
    let text = screen(&mut monitor);
    for expected in [
        "Run run-1",
        "│   ◇ lib       basis   ",
        "│ ▸ ✓ p1        ",
        "│ ▾ ! p2        ERROR: check  ",
        "│     ! check   ERROR · [SPAWN] spawn failed  ",
        "│ ▾ ◐ app       in progress: check, review  ",
        "│     ? review  Human sign-off · claimed by alice  ",
        "◐ RUNNING · validation pending · !1 ◐1 ?1 ✓1 · elapsed",
        "! p2/check  [SPAWN] spawn failed",
        "? app/review  waiting Human · claimed by alice",
        "◐ app/check  running",
        "│   ◐ Run       Enter: usage, budgets, counts",
    ] {
        assert!(text.contains(expected), "{expected}\n{text}");
    }
    // All-done Artifacts start folded; the cursor starts on the first failed or running eval.
    assert!(!text.contains("GREEN  3s"), "{text}");
    assert_eq!(monitor.target(), Some(Target::Eval("p2/check".into())));
}

/// Usage, budgets and errors that used to push the Run summary's last lines off.
fn heavy() -> (RunView, Vec<RequestView>) {
    let (mut view, mut requests) = live();
    let usage = json!([
        {
            "turn":1,
            "attempt":1,
            "usage":{
                "inputTokens":3915049,
                "outputTokens":51234,
                "cacheReadTokens":3618816,
                "cacheWriteTokens":0,
                "reasoningTokens":20480,
                "totalTokens":3966283,
            },
        },
    ]);
    for (index, eval) in ["p1/check", "p2/check"].into_iter().enumerate() {
        let request = &mut requests
            .iter_mut()
            .find(|view| view.request.eval_id == eval)
            .unwrap()
            .request;
        request.provenance = Some(
            serde_json::from_value(
                json!({"repoPath":"/repo","runId":"run-1","requestId":request.id,
                "evalId":eval,"evalDefHash":"hash","completedAt":"2026-01-01T00:00:04Z"}),
            )
            .unwrap(),
        );
        request.execution_id = Some(format!("execution-{index}").parse().unwrap());
        request.usage = Some(serde_json::from_value(usage.clone()).unwrap());
    }
    view.requests = requests.iter().map(|view| view.request.clone()).collect();
    (view, requests)
}

#[test]
fn run_headline_keeps_attention_lines_at_every_width_and_moves_usage_to_run_detail() {
    let (view, requests) = heavy();
    let progress = progress(&view, &requests, now());
    assert!(progress.usage.contains("cacheReadTokens 7237632"));
    assert_eq!(progress.tokens, Some(7_932_566));
    let strip = strip(&progress);
    assert!(
        strip.headline.ends_with("· tokens 7.9M"),
        "{}",
        strip.headline
    );
    for (width, height) in [(160, 45), (100, 30), (80, 24)] {
        for focus in [Pane::Runs, Pane::Artifacts] {
            let mut monitor = Monitor::new("/state".into(), Some("/repo".into()));
            monitor.open = Some("run-1".parse().unwrap());
            let (view, requests) = heavy();
            monitor.set_run(view, requests);
            monitor.focus = focus;
            let text = sized(&mut monitor, width, height);
            if width < 100 && focus == Pane::Runs {
                continue;
            }
            for expected in [
                "tokens 7.9M",
                "! p2/check  [SPAWN] spawn fa",
                "? app/review  waiting Human",
                "◐ app/check  running",
            ] {
                assert!(
                    text.contains(expected),
                    "{width}x{height} {focus:?}: {expected}\n{text}"
                );
            }
            // Counter names and budgets belong to the Run detail.
            for absent in ["cacheReadTokens", "executions 2/5", "jobs 4"] {
                assert!(!text.contains(absent), "{width}x{height}: {absent}\n{text}");
            }
        }
    }
    let detail = detail(&view, &requests, &Target::Run, now());
    assert_eq!(detail.field("Budget"), Some("executions 2/5 · jobs 4"));
    assert_eq!(
        detail.field("Work"),
        Some("executed 2 · reused 0 · derived 0")
    );
    assert!(
        detail
            .field("Usage")
            .unwrap()
            .contains("cacheReadTokens 7237632")
    );
    assert_eq!(
        detail.field("Errors"),
        Some("p2/check  [SPAWN] spawn failed")
    );
    assert_eq!(Target::parse("run:run-1"), Some(Target::Run));
}

#[test]
fn more_than_three_attention_items_fold_into_more() {
    let (view, mut requests) = live();
    for eval in ["x/one", "x/two"] {
        requests.push(request(eval, "ERROR", json!({"error":"boom"})));
    }
    let strip = strip(&progress(&view, &requests, now()));
    assert_eq!(
        strip
            .attention
            .iter()
            .map(|(status, _)| status.as_str())
            .collect::<Vec<_>>(),
        ["ERROR", "ERROR", "ERROR"]
    );
    assert_eq!(strip.more, 2);
    let mut monitor = Monitor::new("/state".into(), None);
    monitor.open = Some("run-1".parse().unwrap());
    monitor.set_run(view, requests);
    monitor.focus = Pane::Artifacts;
    let text = sized(&mut monitor, 160, 40);
    assert!(text.contains("+2 more · Enter on the Run node"), "{text}");
}

#[tokio::test]
async fn tree_peek_follows_selection_and_detail_orders_sections_verdict_first() {
    let (view, mut requests) = live();
    requests.push(request(
        "p1/review",
        "RED",
        json!({"result":{"verdict":"RED","reason":"two mismatches\nsecond line","findings":[1,2]}}),
    ));
    let mut monitor = Monitor::new("/state".into(), None);
    monitor.open = Some("run-1".parse().unwrap());
    monitor.set_run(view, requests);
    monitor.focus = Pane::Artifacts;
    monitor
        .tree
        .select(vec!["a:p2".into(), "e:p2/check".into()]);
    let text = sized(&mut monitor, 160, 40);
    assert!(text.contains("Error: [SPAWN] spawn failed"), "{text}");
    // The peek is the outcome only; provenance and hashes wait for Enter.
    assert!(
        !text.contains("Fingerprint") && !text.contains("Timing"),
        "{text}"
    );
    monitor
        .tree
        .select(vec!["a:p1".into(), "e:p1/review".into()]);
    let text = sized(&mut monitor, 160, 40);
    assert!(text.contains("Result: verdict RED"), "{text}");
    assert!(text.contains("reason two mismatches …"), "{text}");
    assert!(text.contains("findings [2 items]"), "{text}");
    monitor.open_detail().await;
    assert_eq!(monitor.focus, Pane::Detail);
    let text = sized(&mut monitor, 160, 40);
    let at = |needle: &str| {
        text.find(needle)
            .unwrap_or_else(|| panic!("{needle}\n{text}"))
    };
    assert!(at("Outcome") < at("What"));
    assert!(at("What") < at("Provenance"));
    assert!(at("Provenance") < at("▸ Technical"));
    assert!(at("Status: RED") < at("Instruction:"));
    assert!(
        !text.contains("Fingerprint: none"),
        "Technical is folded: {text}"
    );
    // Detail keeps at most two panes: the tree as Compact context and Detail.
    assert!(!text.contains("Runs ("), "{text}");
    assert!(!text.contains("Ctrl-S"), "{text}");
    monitor.key(KeyEvent::from(KeyCode::Char('t')));
    assert!(sized(&mut monitor, 160, 40).contains("Fingerprint: none"));
}

#[test]
fn header_names_the_selected_scope_and_counts_attention_globally() {
    let mut monitor = Monitor::new("/state".into(), None);
    monitor.catalog.update(
        &[crate::store::CatalogRun {
            repo_path: "/work/alpha".into(),
            repository: Default::default(),
            status: crate::types::RunStatus::Running,
            red: 2,
            error: 1,
            waiting: Vec::new(),
        }],
        None,
    );
    let text = sized(&mut monitor, 120, 20);
    assert!(text.contains("artifactize › all repositories"), "{text}");
    assert!(text.contains("!1 ✗2 ◐1"), "{text}");
    monitor.scope = Scope::Worktree(
        Repository::Workspace("/work/alpha".into()),
        "/work/alpha".into(),
    );
    let text = sized(&mut monitor, 120, 20);
    let header = text.lines().next().unwrap();
    assert!(
        header.starts_with("artifactize › alpha (non-Git)"),
        "{header}"
    );
    assert!(!header.contains("all repositories"), "{header}");
    // The Scope pane merges the repository row and `ALL`, and badges share the glyphs.
    let rows: Vec<_> = monitor
        .catalog
        .rows
        .iter()
        .map(|row| (row.label.as_str(), row.badge.text()))
        .collect();
    assert_eq!(
        rows,
        [
            ("ALL", "!1 ✗2 ◐1".to_owned()),
            ("alpha", "!1 ✗2 ◐1".to_owned()),
            ("alpha (non-Git)", "!1 ✗2 ◐1".to_owned()),
        ]
    );
}

#[test]
fn breadcrumb_shortens_earlier_segments_then_drops_the_program_name() {
    let segments = [
        "tui/reactive-layout · tui-redesign",
        "run-bUYY1e",
        "docs/matches-cli",
    ]
    .map(String::from);
    assert_eq!(
        view::breadcrumb(&segments, 200),
        "artifactize › tui/reactive-layout · tui-redesign › run-bUYY1e › docs/matches-cli"
    );
    assert_eq!(
        view::breadcrumb(&segments, 64),
        "artifactize › tui/reactive-la… › run-bUYY1e › docs/matches-cli"
    );
    assert_eq!(
        view::breadcrumb(&segments, 60),
        "artifactize › tui/react… › run-bUYY1e › docs/matches-cli"
    );
    assert_eq!(
        view::breadcrumb(&segments, 44),
        "tui/react… › run-bUYY1e › docs/matches-cli"
    );
    assert_eq!(view::breadcrumb(&segments, 20), "… › docs/matches-cli");
}

#[test]
fn runs_pane_draws_counts_age_and_took_and_keeps_age_when_narrow() {
    let mut monitor = Monitor::new("/state".into(), None);
    let mut finished = summary("run-done", 3);
    finished.completed_at = Some("2026-01-01T00:00:42Z".into());
    monitor.set_runs(vec![summary("run-a", 2), finished]);
    let text = sized(&mut monitor, 160, 20);
    assert!(text.contains("✗ RED run-a "), "{text}");
    assert!(text.contains("✗1 ✓2"), "{text}");
    assert!(text.contains("took 42s"), "{text}");
    let age = run_rows(&monitor.runs, OffsetDateTime::now_utc())[1]
        .age
        .clone();
    let age = age.split(' ').next().unwrap();
    let text = sized(&mut monitor, 22, 20);
    // Narrow Runs keep the glyph, id and age; the status word and counts go first.
    assert!(text.contains(&format!("✗ run-done {age}")), "{text}");
    assert!(!text.contains("RED") && !text.contains("✓3"), "{text}");
}

#[test]
fn review_key_hands_off_only_waiting_human_requests() {
    let (view, requests) = live();
    let mut monitor = Monitor::new("/state".into(), Some("/repo".into()));
    let review = KeyEvent::from(KeyCode::Char('o'));
    assert_eq!(monitor.key(review), Action::None, "no Run is open");
    monitor.open = Some("run-1".parse().unwrap());
    monitor.set_run(view, requests);
    monitor.focus = Pane::Artifacts;
    for (path, action) in [
        (vec!["a:app", "e:app/check"], Action::OpenDetail),
        (vec!["a:app"], Action::OpenDetail),
        (vec!["a:app", "e:app/review"], Action::OpenDetail),
    ] {
        monitor
            .tree
            .select(path.iter().map(|id| (*id).to_owned()).collect());
        assert_eq!(monitor.key(review), action, "{path:?}");
    }
    assert!(screen(&mut monitor).contains("Enter open"));
    monitor.notice = Some("review exited with exit status: 2: Review request not found.".into());
    assert!(screen(&mut monitor).contains("review exited with exit status: 2"));
    monitor.key(KeyEvent::from(KeyCode::Char('j')));
    assert!(!screen(&mut monitor).contains("review exited"));
}

#[test]
fn saved_texts_of_any_length_never_overflow_natural_widths() {
    let long = "X".repeat(usize::from(u16::MAX));
    let (view, mut requests) = live();
    for view in &mut requests {
        if view.request.eval_id == "p2/check" {
            view.request.error = Some(long.clone());
        }
        if view.request.eval_id == "app/check" {
            view.request.profile = serde_json::from_value(
                json!({"kind":"runtime","command":long.clone(),"args":[long.clone()]}),
            )
            .unwrap();
        }
    }
    let mut monitor = Monitor::new("/state".into(), None);
    monitor.catalog.update(
        &[crate::store::CatalogRun {
            repo_path: format!("/work/{long}").into(),
            repository: Default::default(),
            status: crate::types::RunStatus::Running,
            red: 0,
            error: 1,
            waiting: Vec::new(),
        }],
        None,
    );
    let mut row = summary("run-1", 1);
    row.repo_path = format!("/work/{long}").into();
    monitor.set_runs(vec![row]);
    monitor.open = Some("run-1".parse().unwrap());
    monitor.set_run(view, requests);
    for focus in [Pane::Repositories, Pane::Runs, Pane::Artifacts] {
        monitor.focus = focus;
        for (width, height) in [(160, 30), (100, 24), (80, 24)] {
            // Natural widths of the scope label, the Run headline and the tree rows are capped.
            assert!(!sized(&mut monitor, width, height).is_empty());
        }
    }
    monitor.focus = Pane::Artifacts;
    monitor
        .tree
        .select(vec!["a:p2".into(), "e:p2/check".into()]);
    let text = sized(&mut monitor, 160, 30);
    assert!(text.contains("! p2/check  [SPAWN] XXX"), "{text}");
}

#[test]
fn one_row_text_turns_line_breaks_and_tabs_into_spaces_and_drops_other_controls() {
    assert_eq!(rows::plain("plain"), "plain");
    assert_eq!(rows::plain("a\nb\tc\r\nd\u{7}e"), "a b c de");
    assert_eq!(rows::fit("a\nb\tc\u{7}d", 10), "a b cd");
    assert_eq!(rows::fit("first\nsecond", 8), "first s…");
}

#[tokio::test]
async fn control_characters_in_saved_texts_never_reach_cell_widths() {
    let messy = "first diagnostic\nsecond\tdiagnostic\r\nthird\u{7}diagnostic";
    let (view, mut requests) = live();
    for view in &mut requests {
        if view.request.eval_id == "p2/check" {
            view.request.error = Some(messy.into());
        }
        if view.request.eval_id == "app/check" {
            view.request.profile = serde_json::from_value(
                json!({"kind":"runtime","command":"line\nbreak","args":["tab\there"]}),
            )
            .unwrap();
        }
    }
    let mut monitor = Monitor::new("/state".into(), None);
    let path = "/work/line\nbreak\tname";
    monitor.catalog.update(
        &[crate::store::CatalogRun {
            repo_path: path.into(),
            repository: Default::default(),
            status: crate::types::RunStatus::Running,
            red: 0,
            error: 1,
            waiting: Vec::new(),
        }],
        None,
    );
    let mut row = summary("run-1", 1);
    row.repo_path = path.into();
    monitor.set_runs(vec![row]);
    monitor.open = Some("run-1".parse().unwrap());
    monitor.set_run(view, requests);
    monitor
        .tree
        .select(vec!["a:p2".into(), "e:p2/check".into()]);
    for (focus, expected) in [
        (Pane::Repositories, "line break name (non-Git)"),
        (Pane::Runs, "line break name"),
        // The attention line and the tree row (Full) keep one row each.
        (
            Pane::Artifacts,
            "! p2/check  [SPAWN] first diagnostic second diagnostic thirddiagnostic",
        ),
        (Pane::Artifacts, "ERROR · [SPAWN] first diagnostic  "),
        (Pane::Artifacts, "running · runtime line break  "),
    ] {
        monitor.focus = focus;
        for (width, height) in [(160, 30), (100, 24), (80, 24)] {
            let text = sized(&mut monitor, width, height);
            if width == 160 {
                assert!(text.contains(expected), "{focus:?}: {expected}\n{text}");
            }
        }
    }
    // The Compact tree beside Detail, and Detail keeps the full text on its own lines.
    monitor.focus = Pane::Artifacts;
    monitor.open_detail().await;
    let text = sized(&mut monitor, 160, 30);
    assert!(text.contains("Error: [SPAWN] first diagnostic"), "{text}");
    assert!(text.contains("  seconddiagnostic"), "{text}");
    assert!(text.contains("│     ! check"), "{text}");
}

#[test]
fn short_terminals_keep_every_attention_line_or_count_the_rest() {
    for height in [15, 18] {
        for focus in [Pane::Runs, Pane::Artifacts] {
            let (view, requests) = live();
            let mut monitor = Monitor::new("/state".into(), None);
            monitor.open = Some("run-1".parse().unwrap());
            monitor.set_run(view, requests);
            monitor.focus = focus;
            let text = sized(&mut monitor, 160, height);
            for expected in [
                "! p2/check  [SPAWN] spawn failed",
                "? app/review  waiting Human",
                "◐ app/check  running",
            ] {
                assert!(
                    text.contains(expected),
                    "{height} {focus:?}: {expected}\n{text}"
                );
            }
        }
    }
    // Rows that do not fit are counted, including the ones beyond the first three.
    let (view, mut requests) = live();
    for eval in ["x/one", "x/two"] {
        requests.push(request(eval, "ERROR", json!({"error":"boom"})));
    }
    let mut monitor = Monitor::new("/state".into(), None);
    monitor.open = Some("run-1".parse().unwrap());
    monitor.set_run(view, requests);
    monitor.focus = Pane::Artifacts;
    let text = sized(&mut monitor, 160, 12);
    assert!(text.contains("+3 more · Enter on the Run node"), "{text}");
    assert_eq!(text.matches("  boom").count(), 1, "{text}");
}
