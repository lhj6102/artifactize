//! Eval view states, X selection, roll-ups, ordering, folding and the upstream highlight.
use super::*;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{Terminal, backend::TestBackend};
use serde_json::{Value, json};
use time::format_description::well_known::Rfc3339;

use super::tests::request;

fn now() -> OffsetDateTime {
    OffsetDateTime::parse("2026-01-01T00:10:00Z", &Rfc3339).unwrap()
}

fn runtime(id: &str) -> Value {
    json!({"id":id,"target":id.split('/').next(),"deps":[],
        "declaration":{"title":"Title","profile":{"kind":"runtime","command":crate::test_os::true_program(),"args":[]}}})
}

fn referenced(id: &str, on: &[&str]) -> Value {
    json!({"id":id,"target":id.split('/').next(),"deps":on,
        "declaration":{"title":"Title","profile":{"kind":"runtime","command":crate::test_os::true_program(),"args":[]}}})
}

/// Saved definitions: `components` lists `(members, dependencies)` with saved ids from 10 up,
/// so lookups must use the saved id rather than the list position.
fn definitions(components: &[(&[&str], &[usize])], mut evals: Vec<Value>) -> Value {
    for eval in &mut evals {
        if eval["deps"].as_array().is_some_and(Vec::is_empty) {
            let target = eval["target"].as_str().unwrap();
            let deps: Vec<_> = components
                .iter()
                .find(|(members, _)| members.contains(&target))
                .into_iter()
                .flat_map(|(_, deps)| deps.iter())
                .flat_map(|&index| components[index].0.iter().copied())
                .collect();
            eval["deps"] = json!(deps);
        }
    }
    let artifacts: serde_json::Map<String, Value> = components
        .iter()
        .flat_map(|(members, _)| members.iter())
        .map(|id| ((*id).to_owned(), json!({"path":id})))
        .collect();
    let components: Vec<_> = components
        .iter()
        .enumerate()
        .map(|(index, (members, dependencies))| {
            json!({"id":index + 10,"artifacts":members,
                "dependencies":dependencies.iter().map(|id| id + 10).collect::<Vec<_>>(),
                "cyclic":members.len() > 1})
        })
        .collect();
    json!({"artifacts":artifacts,"evals":evals,"components":components})
}

fn saved(
    definitions: Value,
    running: bool,
    requests: Vec<RequestView>,
) -> (RunView, Vec<RequestView>) {
    let mut run = json!({
        "id":"run-1","repoPath":"repo","stateDir":"state",
        "status":if running { "RUNNING" } else { "RED" },
        "createdAt":"2026-01-01T00:00:00Z","selection":{"kind":"all"},
        "definitions":definitions,"validation":null
    });
    if !running {
        run["completedAt"] = json!("2026-01-01T00:05:00Z");
    }
    let mut view = RunView {
        run: serde_json::from_value(run).unwrap(),
        requests: Vec::new(),
        unreadable: Vec::new(),
    };
    view.requests = requests.iter().map(|view| view.request.clone()).collect();
    (view, requests)
}

fn find<'a>(nodes: &'a [Node], id: &str) -> &'a Node {
    nodes
        .iter()
        .flat_map(|node| std::iter::once(node).chain(&node.children))
        .find(|node| node.id == id)
        .unwrap_or_else(|| panic!("{id} not in tree"))
}

fn view<'a>(nodes: &'a [Node], eval: &str) -> &'a EvalView {
    match &find(nodes, &format!("e:{eval}")).kind {
        Kind::Eval(view) => view,
        kind => panic!("{kind:?}"),
    }
}

fn line(nodes: &[Node], id: &str) -> String {
    find(nodes, id).line()
}

fn x(ids: &[&str]) -> Vec<crate::types::ArtifactName> {
    ids.iter().map(|id| id.parse().unwrap()).collect()
}

/// code-style ← cli ← docs, each with one eval.
fn chain(running: bool, statuses: [(&str, Value); 3]) -> Vec<Node> {
    let definitions = definitions(
        &[(&["code-style"], &[]), (&["cli"], &[0]), (&["docs"], &[1])],
        vec![
            runtime("code-style/approved"),
            runtime("cli/follows-style"),
            runtime("docs/matches-cli"),
        ],
    );
    let requests = [
        "code-style/approved",
        "cli/follows-style",
        "docs/matches-cli",
    ]
    .into_iter()
    .zip(statuses)
    .map(|(eval, (status, extra))| request(eval, status, extra))
    .collect();
    let (run, requests) = saved(definitions, running, requests);
    tree(&run, &requests, now())
}

#[test]
fn own_facts_come_before_dependencies() {
    let started = json!({"startedAt":"2026-01-01T00:08:00Z"});
    for (status, expected) in [
        ("GREEN", EvalView::Done(Source::Executed)),
        ("RED", EvalView::Failed { verdict: true }),
        ("ERROR", EvalView::Failed { verdict: false }),
        ("RUNNING", EvalView::InProgress(Activity::Running)),
        ("WAITING_HUMAN", EvalView::InProgress(Activity::Human(None))),
        ("BUDGET_EXHAUSTED", EvalView::NotRun(NotRun::Budget)),
        ("STALE", EvalView::NotRun(NotRun::Stale)),
    ] {
        // Upstream code-style is itself still running.
        let nodes = chain(
            true,
            [
                ("RUNNING", started.clone()),
                (status, started.clone()),
                ("QUEUED", json!({})),
            ],
        );
        assert_eq!(view(&nodes, "cli/follows-style"), &expected, "{status}");
    }
    let nodes = chain(
        true,
        [
            ("RUNNING", started.clone()),
            (
                "RED",
                json!({"result":{"verdict":"RED","violations":[1,2,3]},
                "startedAt":"2026-01-01T00:08:00Z","completedAt":"2026-01-01T00:09:00Z"}),
            ),
            ("QUEUED", json!({})),
        ],
    );
    assert_eq!(
        line(&nodes, "e:cli/follows-style"),
        "✗ follows-style  RED · violations 3  1m 00s"
    );
    assert_eq!(
        line(&nodes, "e:code-style/approved"),
        format!(
            "◐ approved  running · runtime {}  2m 00s",
            crate::test_os::true_program()
        )
    );
    let reused = request(
        "cli/follows-style",
        "GREEN",
        json!({"provenance":{"runId":"run-0","requestId":"run-0-x","evalId":"cli/follows-style",
            "evalDefHash":"dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd","repoPath":"repo","completedAt":"2026-01-01T00:00:00Z"}}),
    );
    let (run, requests) = saved(
        definitions(&[(&["cli"], &[])], vec![runtime("cli/follows-style")]),
        true,
        vec![reused],
    );
    let nodes = tree(&run, &requests, now());
    assert_eq!(
        view(&nodes, "cli/follows-style"),
        &EvalView::Done(Source::Reused)
    );
    assert_eq!(
        line(&nodes, "e:cli/follows-style"),
        "✓ follows-style  GREEN · reused"
    );
}

#[test]
fn queued_for_a_slot_or_jobs_differs_from_waiting_for_an_artifact() {
    let running = ("RUNNING", json!({"startedAt":"2026-01-01T00:08:00Z"}));
    // During a running Run the scheduler leaves gated requests QUEUED.
    let nodes = chain(
        true,
        [
            running.clone(),
            ("QUEUED", json!({})),
            ("QUEUED", json!({})),
        ],
    );
    assert_eq!(
        view(&nodes, "cli/follows-style"),
        &EvalView::WaitingOn(Waits {
            artifacts: x(&["code-style"]),
            root: None
        })
    );
    assert_eq!(
        line(&nodes, "e:cli/follows-style"),
        "… follows-style  waits for code-style ◐ in progress"
    );
    let follows = find(&nodes, "e:cli/follows-style");
    assert_eq!(follows.weight, Weight::Dim);
    assert_eq!(
        follows.text[1],
        Segment {
            text: "code-style ◐ in progress".into(),
            tone: Some(Tone::Running)
        }
    );
    assert_eq!(follows.compact, "←code-style");
    for (extra, queue, text) in [
        (json!({}), Queue::Ready, "queued"),
        (
            json!({
                "blockedReason":"Waiting for a free codex slot: all 2 are in use on this machine (limits.json).",
                "queue":{"kind":"slot","backend":"codex"},
            }),
            Queue::Slot(crate::config::Backend::Codex),
            "queued · codex slots full",
        ),
        (
            json!({
                "executionId":"execution-1",
                "blockedReason":"Waiting for the active execution of this reuse key.",
            }),
            Queue::Joined,
            "queued · joins the running review",
        ),
    ] {
        let nodes = chain(
            true,
            [
                ("GREEN", json!({})),
                ("QUEUED", extra),
                ("QUEUED", json!({})),
            ],
        );
        assert_eq!(
            view(&nodes, "cli/follows-style"),
            &EvalView::InProgress(Activity::Queued(queue))
        );
        let node = find(&nodes, "e:cli/follows-style");
        assert_eq!(node.status(), text);
        assert_eq!((node.tone, node.weight), (Tone::Queued, Weight::Normal));
    }
}

#[test]
fn red_upstream_blocks_and_error_upstream_waits_for_a_retry() {
    let red = ("RED", json!({"completedAt":"2026-01-01T00:04:00Z"}));
    let error = ("ERROR", json!({"error":"spawn failed","errorCode":"SPAWN"}));
    // Running: RED blocks the direct downstream, which blocks the next one.
    let nodes = chain(
        true,
        [red.clone(), ("QUEUED", json!({})), ("QUEUED", json!({}))],
    );
    assert_eq!(
        view(&nodes, "cli/follows-style"),
        &EvalView::BlockedBy(x(&["code-style"]))
    );
    assert_eq!(
        view(&nodes, "docs/matches-cli"),
        &EvalView::BlockedBy(x(&["cli"]))
    );
    assert_eq!(
        line(&nodes, "e:cli/follows-style"),
        "⊘ follows-style  blocked by code-style ✗ failed"
    );
    assert_eq!(
        line(&nodes, "e:docs/matches-cli"),
        "⊘ matches-cli  blocked by cli ⊘ blocked"
    );
    // Finished: the saved BLOCKED reads as not run.
    let nodes = chain(false, [red, ("BLOCKED", json!({})), ("BLOCKED", json!({}))]);
    assert_eq!(
        line(&nodes, "e:cli/follows-style"),
        "⊘ follows-style  not run: blocked by code-style ✗ failed"
    );
    assert_eq!(
        line(&nodes, "a:cli"),
        "⊘ cli  not run: blocked by code-style  0/1"
    );
    // ERROR is not a verdict: downstream waits, and asks for a retry.
    let nodes = chain(
        true,
        [error.clone(), ("QUEUED", json!({})), ("QUEUED", json!({}))],
    );
    assert_eq!(
        line(&nodes, "e:cli/follows-style"),
        "… follows-style  waits for code-style ! failed (retry code-style first)"
    );
    let nodes = chain(
        false,
        [
            error,
            ("WAIT_DEPENDENCY", json!({})),
            ("WAIT_DEPENDENCY", json!({})),
        ],
    );
    assert_eq!(
        view(&nodes, "cli/follows-style"),
        &EvalView::NotRun(NotRun::Dependency(Waits {
            artifacts: x(&["code-style"]),
            root: None
        }))
    );
    assert_eq!(
        line(&nodes, "e:cli/follows-style"),
        "○ follows-style  not run: waited for code-style ! failed (retry code-style first)"
    );
    assert_eq!(
        line(&nodes, "a:code-style"),
        "! code-style  ERROR: approved  0/1"
    );
    assert_eq!(
        line(&nodes, "a:cli"),
        "○ cli  not run: waited for code-style  0/1"
    );
    assert!(!find(&nodes, "a:cli").changed);
}

#[test]
fn every_x_waiting_adds_one_level_of_root_cause() {
    let nodes = chain(
        true,
        [
            ("WAITING_HUMAN", json!({"profile":{"kind":"human"}})),
            ("QUEUED", json!({})),
            ("QUEUED", json!({})),
        ],
    );
    assert_eq!(
        view(&nodes, "docs/matches-cli"),
        &EvalView::WaitingOn(Waits {
            artifacts: x(&["cli"]),
            root: Some(("cli".parse().unwrap(), "code-style".parse().unwrap()))
        })
    );
    assert_eq!(
        line(&nodes, "e:docs/matches-cli"),
        "… matches-cli  waits for cli … waiting (cli waits for code-style)"
    );
    assert_eq!(line(&nodes, "a:docs"), "… docs  waits for cli  0/1");
    assert_eq!(
        line(&nodes, "a:code-style"),
        "? code-style  in progress: approved  0/1"
    );
    assert_eq!(
        line(&nodes, "e:code-style/approved"),
        "? approved  Human sign-off · unclaimed  10m 00s"
    );
}

/// style ← {api, web} (one SCC) with a runtime Eval in api naming web.
fn cycle(
    dependency_status: &str,
) -> (
    Vec<Node>,
    Vec<(crate::types::EvalId, Vec<crate::types::ArtifactName>)>,
) {
    let definitions = definitions(
        &[(&["style"], &[]), (&["web", "api"], &[0])],
        vec![
            runtime("style/tokens"),
            runtime("api/schema"),
            referenced("api/ready", &["web"]),
            runtime("web/colors"),
        ],
    );
    let requests = vec![
        request(
            "style/tokens",
            "RUNNING",
            json!({"startedAt":"2026-01-01T00:09:56Z"}),
        ),
        request("api/schema", "QUEUED", json!({})),
        request(
            "api/ready",
            dependency_status,
            json!({"profile":{"kind":"runtime","command":crate::test_os::true_program(),"args":[]},"deps":["web"]}),
        ),
        request("web/colors", "QUEUED", json!({})),
    ];
    let (run, requests) = saved(definitions, true, requests);
    (
        tree(&run, &requests, now()),
        upstream_index(&run, &requests),
    )
}

#[test]
fn referencing_evals_wait_for_their_inputs_with_at_most_two_x() {
    let definitions = definitions(
        &[
            (&["engine"], &[]),
            (&["hero-art"], &[]),
            (&["level"], &[]),
            (&["player-movement"], &[0]),
            (&["player"], &[0]),
        ],
        vec![
            runtime("hero-art/approved"),
            runtime("hero-art/svg-valid"),
            runtime("level/check"),
            runtime("player-movement/walks"),
            referenced("player/ready", &["player-movement", "hero-art", "level"]),
        ],
    );
    let mut definitions = definitions;
    definitions["artifacts"]["engine"]["basis"] = json!(true);
    let requests = vec![
        request(
            "hero-art/approved",
            "WAITING_HUMAN",
            json!({"profile":{"kind":"human"},"createdAt":"2026-01-01T00:07:00Z"}),
        ),
        request("hero-art/svg-valid", "GREEN", json!({})),
        request("level/check", "QUEUED", json!({})),
        request(
            "player-movement/walks",
            "RUNNING",
            json!({"startedAt":"2026-01-01T00:08:20Z"}),
        ),
        request(
            "player/ready",
            "WAIT_DEPENDENCY",
            json!({
                "profile":{"kind":"runtime","command":crate::test_os::true_program(),"args":[]},
                "deps":["player-movement","hero-art","level"],
            }),
        ),
    ];
    let (run, requests) = saved(definitions, true, requests);
    let nodes = tree(&run, &requests, now());
    // All three are in progress, so they keep the tree order; the rest become `+N`.
    assert_eq!(
        view(&nodes, "player/ready"),
        &EvalView::WaitingOn(Waits {
            artifacts: x(&["hero-art", "level", "player-movement"]),
            root: None
        })
    );
    assert_eq!(
        line(&nodes, "e:player/ready"),
        "… ready  waits for hero-art ?, level · +1"
    );
    assert_eq!(find(&nodes, "e:player/ready").compact, "←3");
    assert_eq!(line(&nodes, "a:engine"), "◇ engine  basis");
    assert!(find(&nodes, "a:engine").children.is_empty());
    assert_eq!(
        line(&nodes, "a:hero-art"),
        "? hero-art  in progress: approved  1/2"
    );
    assert_eq!(
        line(&nodes, "a:player"),
        "… player  waits for hero-art, level +1  0/1"
    );
    // The upstream index keeps the Artifacts in tree order; the row orders them by urgency.
    let upstream = &find(&nodes, "e:player/ready").upstream;
    assert_eq!(
        upstream
            .iter()
            .map(|up| (up.artifact.as_str(), up.completion))
            .collect::<Vec<_>>(),
        [
            ("hero-art", Completion::InProgress(Busy::Human)),
            ("level", Completion::InProgress(Busy::Queued)),
            ("player-movement", Completion::InProgress(Busy::Running)),
        ]
    );
}

#[test]
fn artifact_rows_roll_up_current_eval_rows_and_mark_changes_after_the_run() {
    let definitions = definitions(
        &[(&["code-style"], &[]), (&["cli"], &[0]), (&["docs"], &[1])],
        vec![
            runtime("code-style/approved"),
            runtime("cli/follows-style"),
            runtime("cli/lint"),
            runtime("docs/matches-cli"),
        ],
    );
    let mut requests = vec![
        // Approved after the Run ended at 00:05.
        request(
            "code-style/approved",
            "GREEN",
            json!({"profile":{"kind":"human"},"completedAt":"2026-01-01T00:06:00Z"}),
        ),
        request("cli/follows-style", "WAIT_DEPENDENCY", json!({})),
        request("cli/lint", "WAIT_DEPENDENCY", json!({})),
        request("docs/matches-cli", "WAIT_DEPENDENCY", json!({})),
    ];
    let (mut run, saved_requests) = saved(definitions.clone(), false, requests.clone());
    run.run.validation = serde_json::from_value(json!({"satisfied":false,"artifacts":[
        {"id":"code-style","status":"UNREVIEWED","passed":0,"total":1}]}))
    .unwrap();
    let nodes = tree(&run, &saved_requests, now());
    // The current eval row, not the Run-end snapshot.
    assert_eq!(line(&nodes, "a:code-style"), "✓ code-style  1/1 *");
    assert_eq!(
        line(&nodes, "e:code-style/approved"),
        "✓ approved  GREEN  6m 00s *"
    );
    assert!(find(&nodes, "a:code-style").done());
    // The upstream completed after the Run: cli is no longer waiting.
    assert_eq!(
        line(&nodes, "e:cli/follows-style"),
        "○ follows-style  not reviewed *"
    );
    assert_eq!(line(&nodes, "a:cli"), "○ cli  not reviewed  0/2 *");
    // docs still waits for cli exactly as at Run end.
    assert_eq!(
        line(&nodes, "e:docs/matches-cli"),
        "○ matches-cli  not run: waited for cli … waiting"
    );
    let detail = detail(
        &run,
        &saved_requests,
        &Target::Artifact("code-style".parse().unwrap()),
        now(),
    );
    assert_eq!(detail.field("Status"), Some("✓ complete · 1/1 Evals GREEN"));
    assert_eq!(
        detail.field("At Run end"),
        Some("UNREVIEWED · 0/1 Evals GREEN")
    );

    // A failure rolls up first, and an eval still in progress gets a second clause.
    requests[1] = request("cli/follows-style", "RED", json!({}));
    requests[2] = request("cli/lint", "RUNNING", json!({}));
    let (run, requests) = saved(definitions, true, requests);
    let nodes = tree(&run, &requests, now());
    assert_eq!(
        line(&nodes, "a:cli"),
        "✗ cli  RED: follows-style · in progress: lint  0/2"
    );
    assert_eq!(find(&nodes, "a:cli").glyph, "✗");
    assert!(
        !find(&nodes, "a:cli").changed,
        "a running Run has no end yet"
    );
}

#[test]
fn speculative_readiness_and_saved_blocked_requests() {
    let definitions = definitions(
        &[(&["code-style"], &[]), (&["cli"], &[0])],
        vec![runtime("code-style/approved"), runtime("cli/follows-style")],
    );
    let requests = vec![
        request("code-style/approved", "RUNNING", json!({})),
        request("cli/follows-style", "QUEUED", json!({})),
    ];
    let (mut run, requests) = saved(definitions.clone(), true, requests);
    run.run.modes.speculate = true;
    let nodes = tree(&run, &requests, now());
    assert_eq!(
        view(&nodes, "cli/follows-style"),
        &EvalView::InProgress(Activity::Queued(Queue::Ready))
    );
    // A saved BLOCKED reads by the current state: blocked only while something upstream
    // still blocks, otherwise it waited.
    let requests = vec![
        request("code-style/approved", "UNREVIEWED", json!({})),
        request("cli/follows-style", "BLOCKED", json!({})),
    ];
    let (run, requests) = saved(definitions, false, requests);
    let nodes = tree(&run, &requests, now());
    assert_eq!(
        view(&nodes, "cli/follows-style"),
        &EvalView::NotRun(NotRun::Dependency(Waits {
            artifacts: x(&["code-style"]),
            root: None
        }))
    );
    assert_eq!(
        view(&nodes, "code-style/approved"),
        &EvalView::NotRun(NotRun::Unreviewed)
    );
    // The RED upstream turned GREEN after the Run: neither downstream row stays blocked.
    let nodes = chain(
        false,
        [
            ("GREEN", json!({"completedAt":"2026-01-01T00:06:00Z"})),
            ("BLOCKED", json!({})),
            ("BLOCKED", json!({})),
        ],
    );
    assert_eq!(
        line(&nodes, "e:cli/follows-style"),
        "○ follows-style  not reviewed *"
    );
    assert_eq!(
        line(&nodes, "e:docs/matches-cli"),
        "○ matches-cli  not run: waited for cli … waiting"
    );
}

#[test]
fn upstream_index_uses_eval_references_not_artifact_components() {
    // Material components and visibility alone do not add Eval gates.
    let definitions = json!({
        "artifacts":{"a":{"path":"a"},"b":{"path":"b"},"c":{"path":"c"},"d":{"path":"d"}},
        "evals":[runtime("a/x"), runtime("b/x"), referenced("c/x", &["a"]), runtime("d/x")],
        "relations":[
            {"source":"a","target":"b","kind":"mount","alias":"base"},
            {"source":"b","target":"a","kind":"child","path":"a"},
            {"source":"a","target":"c","kind":"instruction","name":"a","evalId":"c/x"},
            {"source":"c","target":"d","kind":"child","path":"c"},
        ],
        "components":[
            {"id":0,"artifacts":["a","b"],"cyclic":true},
            {"id":1,"artifacts":["c"]},
            {"id":2,"artifacts":["d"]},
        ],
    });
    let (run, requests) = saved(definitions, true, Vec::new());
    assert_eq!(
        upstream_index(&run, &requests),
        [
            ("a/x".parse().unwrap(), x(&[])),
            ("b/x".parse().unwrap(), x(&[])),
            ("c/x".parse().unwrap(), x(&["a"])),
            ("d/x".parse().unwrap(), x(&[])),
        ]
    );
    let nodes = tree(&run, &requests, now());
    assert_eq!(line(&nodes, "a:a"), "- a  ↻ b  not in this Run  0/1");
    // Runs saved without definitions: the request's referenced Artifacts.
    let requests = vec![
        request("lib/x", "GREEN", json!({})),
        request("app/x", "QUEUED", json!({"deps":["lib"]})),
    ];
    let (run, requests) = saved(Value::Null, true, requests);
    assert_eq!(
        upstream_index(&run, &requests),
        [
            ("app/x".parse().unwrap(), x(&["lib"])),
            ("lib/x".parse().unwrap(), x(&[]))
        ]
    );
    let nodes = tree(&run, &requests, now());
    assert_eq!(
        view(&nodes, "app/x"),
        &EvalView::InProgress(Activity::Queued(Queue::Ready))
    );
}

#[test]
fn waits_for_detail_lists_upstream_with_origins_and_pending_evals() {
    let definitions = json!({
        "artifacts":{
            "code-style":{"path":"s"},
            "cli":{"path":"c"},
            "engine":{"path":"e","basis":true},
        },
        "evals":[runtime("code-style/approved"), referenced("cli/follows-style", &["code-style"])],
        "relations":[
            {
                "source":"code-style",
                "target":"cli",
                "kind":"instruction",
                "name":"code-style",
                "evalId":"cli/follows-style",
            },
            {"source":"engine","target":"cli","kind":"mount","alias":"base"},
        ],
        "components":[
            {"id":0,"artifacts":["code-style"]},
            {"id":1,"artifacts":["engine"]},
            {"id":2,"artifacts":["cli"],"dependencies":[0,1]},
        ],
    });
    let requests = vec![
        request(
            "code-style/approved",
            "WAITING_HUMAN",
            json!({"profile":{"kind":"human"}}),
        ),
        request("cli/follows-style", "QUEUED", json!({})),
    ];
    let (run, requests) = saved(definitions, true, requests);
    let detail = detail(
        &run,
        &requests,
        &Target::Eval("cli/follows-style".parse().unwrap()),
        now(),
    );
    assert_eq!(
        detail.field("Waits for"),
        Some(
            "↑ code-style ? in progress · {code-style} in cli/follows-style\n  ? approved  Human sign-off · unclaimed"
        )
    );
    assert_eq!(detail.fields[1].0, "Waits for", "right after Status");
}

fn render(monitor: &mut Monitor, width: u16, height: u16) -> String {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal
        .draw(|frame| monitor.draw_at(frame, now()))
        .unwrap();
    let buffer = terminal.backend().buffer();
    let rows = buffer.content().chunks(buffer.area.width as usize);
    let rows = rows.map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>());
    rows.collect::<Vec<_>>().join("\n")
}

/// Eight Artifacts: a running upstream, a RED one, six waiting on both.
fn wide(monitor: &mut Monitor) {
    let names = ["up-a", "up-b", "m1", "m2", "m3", "m4", "m5", "m6"];
    let components: Vec<(&[&str], &[usize])> = vec![
        (&names[0..1], &[]),
        (&names[1..2], &[]),
        (&names[2..3], &[0, 1]),
        (&names[3..4], &[]),
        (&names[4..5], &[]),
        (&names[5..6], &[]),
        (&names[6..7], &[]),
        (&names[7..8], &[0]),
    ];
    let definitions = definitions(
        &components,
        names
            .iter()
            .map(|name| runtime(&format!("{name}/x")))
            .collect(),
    );
    let requests = names
        .iter()
        .map(|name| {
            let status = match *name {
                "up-a" => "RUNNING",
                "up-b" => "GREEN",
                _ => "QUEUED",
            };
            request(&format!("{name}/x"), status, json!({}))
        })
        .collect();
    let (run, requests) = saved(definitions, true, requests);
    monitor.open = Some("run-1".parse().unwrap());
    monitor.set_run(run, requests);
    monitor.focus = Pane::Artifacts;
}

#[test]
fn upstream_rows_are_marked_counted_off_screen_and_reached_with_b() {
    let (_monitor_root, mut monitor) = tests::monitor();
    wide(&mut monitor);
    // up-a/x runs, so the cursor starts there; up-b is all done and folded.
    assert_eq!(
        monitor.target(),
        Some(Target::Eval("up-a/x".parse().unwrap()))
    );
    let text = render(&mut monitor, 200, 60);
    assert!(text.contains("│ ▸ ✓ up-b  "), "{text}");
    monitor.tree.select(vec![
        "a:m1".parse::<crate::monitor::model::NodeId>().unwrap(),
        "e:m1/x".parse::<crate::monitor::model::NodeId>().unwrap(),
    ]);
    let text = render(&mut monitor, 200, 60);
    // Unmet upstream first; a complete one still gets a (dim) marker.
    assert!(text.contains("│↑▾ ◐ up-a  "), "{text}");
    assert!(text.contains("│↑▸ ✓ up-b  "), "{text}");
    assert!(
        text.contains("│ ▾ · m2  "),
        "no downstream or unrelated marks\n{text}"
    );
    assert!(text.contains("b/Backspace blocker"), "{text}");
    for expected in [
        Target::Artifact("up-a".parse().unwrap()),
        Target::Artifact("up-b".parse().unwrap()),
        Target::Artifact("up-a".parse().unwrap()),
    ] {
        monitor.key(KeyEvent::from(KeyCode::Char('b')));
        assert_eq!(monitor.target(), Some(expected));
        // While cycling, the origin's upstream stays marked.
        let text = render(&mut monitor, 200, 60);
        assert!(text.contains("│↑▸ ✓ up-b  "), "{text}");
    }
    monitor.key(KeyEvent::from(KeyCode::Backspace));
    assert_eq!(
        monitor.target(),
        Some(Target::Eval("m1/x".parse().unwrap()))
    );
    // Off-screen marks are counted on the borders, separately above and below. The Run
    // headline leaves eleven tree rows at this height.
    monitor.tree.select(vec![
        "a:m6".parse::<crate::monitor::model::NodeId>().unwrap(),
        "e:m6/x".parse::<crate::monitor::model::NodeId>().unwrap(),
    ]);
    let text = render(&mut monitor, 200, 18);
    assert!(text.contains("↑1 above"), "{text}");
    assert!(!text.contains("below"), "{text}");
    monitor.tree.select(vec![
        "a:m1".parse::<crate::monitor::model::NodeId>().unwrap(),
        "e:m1/x".parse::<crate::monitor::model::NodeId>().unwrap(),
    ]);
    render(&mut monitor, 200, 18);
    monitor.tree.scroll_down(3);
    let text = render(&mut monitor, 200, 18);
    assert!(text.contains("↑2 above"), "{text}");
    monitor.tree.scroll_up(20);
    let text = render(&mut monitor, 200, 18);
    assert!(!text.contains("above") && !text.contains("below"), "{text}");
    // A row without upstream: no marks, no hint, and `b` stays put.
    monitor.tree.select(vec![
        "a:m2".parse::<crate::monitor::model::NodeId>().unwrap(),
        "e:m2/x".parse::<crate::monitor::model::NodeId>().unwrap(),
    ]);
    let text = render(&mut monitor, 200, 60);
    assert!(!text.contains("│↑") && !text.contains("blocker"), "{text}");
    monitor.key(KeyEvent::from(KeyCode::Char('b')));
    assert_eq!(
        monitor.target(),
        Some(Target::Eval("m2/x".parse().unwrap()))
    );
}

#[test]
fn a_referencing_eval_marks_a_peer_below_it() {
    let (nodes, _) = cycle("WAIT_DEPENDENCY");
    let path = [
        "a:api".parse::<crate::monitor::model::NodeId>().unwrap(),
        "e:api/ready"
            .parse::<crate::monitor::model::NodeId>()
            .unwrap(),
    ];
    let upstream = fold::upstream(&nodes, &path);
    let mut state = tui_tree_widget::TreeState::default();
    for node in &nodes {
        state.open(vec![node.id.clone()]);
    }
    state.select(path.to_vec());
    // Five inner rows: style (2), api (3); web is below.
    let mut terminal = Terminal::new(TestBackend::new(70, 7)).unwrap();
    terminal
        .draw(|frame| {
            rows::draw(
                frame,
                frame.area(),
                ratatui::widgets::Block::bordered(),
                &nodes,
                &mut state,
                rows::Layout {
                    total: 0,
                    compact: false,
                    upstream: &upstream,
                    now: now(),
                },
            );
        })
        .unwrap();
    let buffer = terminal.backend().buffer();
    let rows: Vec<String> = buffer
        .content()
        .chunks(70)
        .map(|row| row.iter().map(|cell| cell.symbol()).collect())
        .collect();
    assert!(rows[6].ends_with("─ ↑1 below ─┘"), "{rows:#?}");
    assert!(!rows.join("\n").contains("above"), "{rows:#?}");
    assert!(
        rows[1].starts_with("│ ▾ ◐ style"),
        "style is not upstream of ready: {rows:#?}"
    );
}

#[test]
fn refreshes_fold_done_artifacts_unless_the_user_toggled_them() {
    let (_monitor_root, mut monitor) = tests::monitor();
    wide(&mut monitor);
    let opened = |monitor: &Monitor, id: &str| {
        monitor
            .tree
            .opened()
            .contains(&vec![model::NodeId::artifact(id.parse().unwrap())])
    };
    assert!(opened(&monitor, "up-a") && !opened(&monitor, "up-b") && opened(&monitor, "m1"));
    render(&mut monitor, 200, 60);
    // The user unfolds up-b and folds m2.
    monitor.tree.select(vec![
        "a:up-b".parse::<crate::monitor::model::NodeId>().unwrap(),
    ]);
    monitor.key(KeyEvent::from(KeyCode::Char(' ')));
    monitor.tree.select(vec![
        "a:m2".parse::<crate::monitor::model::NodeId>().unwrap(),
    ]);
    monitor.key(KeyEvent::from(KeyCode::Char('h')));
    // up-a finishes while the cursor sits on its eval; m2 finishes too.
    monitor.tree.select(vec![
        "a:up-a".parse::<crate::monitor::model::NodeId>().unwrap(),
        "e:up-a/x".parse::<crate::monitor::model::NodeId>().unwrap(),
    ]);
    let (_, requests) = monitor.run.as_mut().unwrap();
    for view in requests {
        if matches!(view.request.eval_id.as_str(), "up-a/x" | "m2/x") {
            view.request.state = crate::store::RequestState::completed(
                serde_json::json!({"verdict":"GREEN"}).try_into().unwrap(),
                "2026-01-01T00:09:00Z".parse().unwrap(),
            );
        }
    }
    monitor.sync_tree(false);
    assert!(!opened(&monitor, "up-a"), "folded once all done");
    assert_eq!(
        monitor.target(),
        Some(Target::Artifact("up-a".parse().unwrap()))
    );
    assert!(opened(&monitor, "up-b"), "the user's unfold stays");
    assert!(!opened(&monitor, "m2"), "the user's fold stays");
    assert!(opened(&monitor, "m1"));
}

#[test]
fn compact_rows_keep_glyph_name_and_one_right_column() {
    let nodes = chain(
        true,
        [
            ("RUNNING", json!({"startedAt":"2026-01-01T00:08:00Z"})),
            ("QUEUED", json!({})),
            ("QUEUED", json!({})),
        ],
    );
    let layout = rows::Layout {
        total: 28,
        compact: true,
        upstream: &[],
        now: now(),
    };
    let items = rows::items(&nodes, &layout).unwrap();
    let mut state = tui_tree_widget::TreeState::default();
    for node in &nodes {
        state.open(vec![node.id.clone()]);
    }
    state.select(vec![
        "a:code-style"
            .parse::<crate::monitor::model::NodeId>()
            .unwrap(),
    ]);
    let mut terminal = Terminal::new(TestBackend::new(28, 6)).unwrap();
    terminal
        .draw(|frame| {
            frame.render_stateful_widget(
                tui_tree_widget::Tree::new(&items)
                    .unwrap()
                    .highlight_symbol(" ")
                    .node_open_symbol("▾ "),
                frame.area(),
                &mut state,
            )
        })
        .unwrap();
    let buffer = terminal.backend().buffer();
    let rows: Vec<String> = buffer
        .content()
        .chunks(28)
        .map(|row| row.iter().map(|cell| cell.symbol()).collect())
        .collect();
    assert_eq!(
        rows,
        [
            " ▾ ◐ code-style          0/1",
            "     ◐ approved       2m 00s",
            " ▾ … cli                 0/1",
            "     … follows-… ←code-style",
            " ▾ … docs                0/1",
            "     … matches-cli      ←cli",
        ]
    );
}

fn reused() -> Value {
    json!({"provenance":{"runId":"run-0","requestId":"run-0-x","evalId":"cli/follows-style",
        "evalDefHash":"dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd","repoPath":"repo","completedAt":"2026-01-01T00:00:00Z"}})
}

#[test]
fn a_done_eval_held_by_its_own_gates_does_not_fulfil_downstream_gates() {
    // graph.rs masks cli's reused GREEN as BLOCKED behind the RED code-style.
    let nodes = chain(
        true,
        [
            ("RED", json!({})),
            ("GREEN", reused()),
            ("QUEUED", json!({})),
        ],
    );
    assert_eq!(
        view(&nodes, "cli/follows-style"),
        &EvalView::Done(Source::Reused),
        "the row keeps its own fact"
    );
    assert_eq!(
        view(&nodes, "docs/matches-cli"),
        &EvalView::BlockedBy(x(&["cli"]))
    );
    assert_eq!(
        line(&nodes, "e:docs/matches-cli"),
        "⊘ matches-cli  blocked by cli ⊘ blocked"
    );
    assert_eq!(
        line(&nodes, "a:cli"),
        "⊘ cli  done, but blocked by code-style  0/1"
    );
    assert!(!find(&nodes, "a:cli").done(), "not folded as done");
    assert_eq!(
        find(&nodes, "e:docs/matches-cli").upstream,
        [Upstream {
            artifact: "cli".parse().unwrap(),
            completion: Completion::Waiting { blocked: true }
        }]
    );
    // Behind a running upstream the GREEN waits, and so does its downstream.
    let nodes = chain(
        true,
        [
            ("RUNNING", json!({})),
            ("GREEN", reused()),
            ("QUEUED", json!({})),
        ],
    );
    assert_eq!(
        view(&nodes, "docs/matches-cli"),
        &EvalView::WaitingOn(Waits {
            artifacts: x(&["cli"]),
            root: Some(("cli".parse().unwrap(), "code-style".parse().unwrap()))
        })
    );
    assert_eq!(
        line(&nodes, "a:cli"),
        "… cli  done, but waits for code-style  0/1"
    );
    // The same after the Run ended.
    let nodes = chain(
        false,
        [
            ("RED", json!({})),
            ("GREEN", reused()),
            ("BLOCKED", json!({})),
        ],
    );
    assert_eq!(
        line(&nodes, "e:docs/matches-cli"),
        "⊘ matches-cli  not run: blocked by cli ⊘ blocked"
    );
    assert!(!find(&nodes, "a:docs").changed);
    // A runtime Eval referencing the held Artifact is blocked too: speculation never
    // waives a known RED dependency.
    let definitions = definitions(
        &[(&["code-style"], &[]), (&["cli"], &[0]), (&["docs"], &[1])],
        vec![
            runtime("code-style/approved"),
            runtime("cli/follows-style"),
            runtime("docs/matches-cli"),
            referenced("docs/ready", &["cli"]),
        ],
    );
    let requests = vec![
        request("code-style/approved", "RED", json!({})),
        request("cli/follows-style", "GREEN", reused()),
        request("docs/matches-cli", "QUEUED", json!({})),
        request(
            "docs/ready",
            "WAIT_DEPENDENCY",
            json!({"profile":{"kind":"runtime","command":crate::test_os::true_program(),"args":[]},"deps":["cli"]}),
        ),
    ];
    let (mut run, requests) = saved(definitions, true, requests);
    let nodes = tree(&run, &requests, now());
    assert_eq!(
        view(&nodes, "docs/ready"),
        &EvalView::BlockedBy(x(&["cli"]))
    );
    run.run.modes.speculate = true;
    let nodes = tree(&run, &requests, now());
    assert_eq!(
        view(&nodes, "docs/matches-cli"),
        &EvalView::BlockedBy(x(&["cli"]))
    );
    assert_eq!(
        view(&nodes, "docs/ready"),
        &EvalView::BlockedBy(x(&["cli"]))
    );
}

#[test]
fn backspace_lands_on_a_visible_row_after_the_origin_folded() {
    let (_monitor_root, mut monitor) = tests::monitor();
    wide(&mut monitor);
    let opened = |monitor: &Monitor, id: &str| {
        monitor
            .tree
            .opened()
            .contains(&vec![model::NodeId::artifact(id.parse().unwrap())])
    };
    monitor.tree.select(vec![
        "a:m6".parse::<crate::monitor::model::NodeId>().unwrap(),
        "e:m6/x".parse::<crate::monitor::model::NodeId>().unwrap(),
    ]);
    render(&mut monitor, 200, 60);
    monitor.key(KeyEvent::from(KeyCode::Char('b')));
    assert_eq!(
        monitor.target(),
        Some(Target::Artifact("up-a".parse().unwrap()))
    );
    // Meanwhile up-a and m6 finish, and m6 folds automatically.
    let (_, requests) = monitor.run.as_mut().unwrap();
    for view in requests {
        if matches!(view.request.eval_id.as_str(), "up-a/x" | "m6/x") {
            view.request.state = crate::store::RequestState::completed(
                serde_json::json!({"verdict":"GREEN"}).try_into().unwrap(),
                "2026-01-01T00:09:00Z".parse().unwrap(),
            );
        }
    }
    monitor.sync_tree(false);
    assert!(!opened(&monitor, "m6"));
    monitor.key(KeyEvent::from(KeyCode::Backspace));
    assert_eq!(
        monitor.target(),
        Some(Target::Eval("m6/x".parse().unwrap()))
    );
    assert!(opened(&monitor, "m6"), "the origin unfolds");
    let text = render(&mut monitor, 200, 60);
    assert!(text.contains("│ ▾ ✓ m6 "), "{text}");
    // The unfold counts as the user's: later refreshes leave it open.
    monitor.sync_tree(false);
    assert!(opened(&monitor, "m6"));
    assert_eq!(
        monitor.target(),
        Some(Target::Eval("m6/x".parse().unwrap()))
    );
}

#[test]
fn frames_preserve_rendered_rows_and_refresh_updates_their_state() {
    let (_monitor_root, mut monitor) = tests::monitor();
    wide(&mut monitor);
    let built = monitor.nodes();
    let first = render(&mut monitor, 200, 60);
    assert_eq!(render(&mut monitor, 200, 60), first);
    monitor.key(KeyEvent::from(KeyCode::Char('j')));
    assert!(render(&mut monitor, 200, 60).contains("◐ up-a"));
    // Elapsed times come from each row's clock, not from the cached text.
    let running = built
        .iter()
        .flat_map(|node| &node.children)
        .find(|node| node.id == "e:up-a/x")
        .unwrap();
    assert_eq!(running.right_at(now(), false), "10m 00s");
    assert_eq!(
        running.right_at(now() + time::Duration::seconds(65), true),
        "11m 05s"
    );
    // A changed saved verdict changes the visible row on refresh; closing removes it.
    let (_, requests) = monitor.run.as_mut().unwrap();
    let request = requests
        .iter_mut()
        .find(|view| view.request.eval_id == "up-a/x")
        .unwrap();
    request.request.state = crate::store::RequestState::completed(
        serde_json::json!({"verdict":"GREEN"}).try_into().unwrap(),
        "2026-01-01T00:00:01Z".parse().unwrap(),
    );
    monitor.sync_tree(false);
    assert!(render(&mut monitor, 200, 60).contains("✓ up-a"));
    monitor.run = None;
    assert!(!render(&mut monitor, 200, 60).contains("up-a"));
}

/// Large independent wait chains retain their visible causes without quadratic model work.
#[test]
fn long_wait_chains_preserve_output_with_bounded_model_work_growth() {
    // Thousands of rows make the scaling check meaningful beyond fixed setup overhead.
    const SMALL_CHAIN: usize = 5_000;
    const LARGE_CHAIN: usize = 10_000;
    // Doubling input may triple measured work: room for different derivation strategies,
    // but not the fourfold growth of quadratic work. No per-Eval operation count is pinned.
    const MAX_WORK_GROWTH: usize = 3;
    let measure = |count, tail, expected_tail| {
        let names: Vec<String> = (0..count).map(|index| format!("a{index:05}")).collect();
        let components: Vec<(Vec<&str>, Vec<usize>)> = names
            .iter()
            .enumerate()
            .map(|(index, name)| {
                (
                    vec![name.as_str()],
                    index.checked_sub(1).into_iter().collect(),
                )
            })
            .collect();
        let components: Vec<(&[&str], &[usize])> = components
            .iter()
            .map(|(members, dependencies)| (members.as_slice(), dependencies.as_slice()))
            .collect();
        let definitions = definitions(
            &components,
            names
                .iter()
                .map(|name| runtime(&format!("{name}/x")))
                .collect(),
        );
        let requests = names
            .iter()
            .enumerate()
            .map(|(index, name)| {
                let status = match index {
                    0..10 => "GREEN",
                    10 => "RUNNING",
                    _ => "QUEUED",
                };
                request(&format!("{name}/x"), status, json!({}))
            })
            .collect();
        let (run, requests) = saved(definitions, true, requests);
        let nodes = tree(&run, &requests, now());
        assert_eq!(nodes.len(), count);
        // Independently specified model output at both the running head and waiting tail.
        assert_eq!(line(&nodes, tail), expected_tail);
        assert_eq!(
            line(&nodes, "e:a00011/x"),
            "… x  waits for a00010 ◐ in progress"
        );
        model::derivation_count(&run, &requests)
    };
    let small_work = measure(
        SMALL_CHAIN,
        "e:a04999/x",
        "… x  waits for a04998 … waiting (a04998 waits for a04997)",
    );
    let large_work = measure(
        LARGE_CHAIN,
        "e:a09999/x",
        "… x  waits for a09998 … waiting (a09998 waits for a09997)",
    );
    assert!(
        small_work > 0 && large_work > 0,
        "the model work probe records both inputs"
    );
    assert!(
        large_work <= small_work * MAX_WORK_GROWTH,
        "doubling the wait chain increased model work from {small_work} to {large_work}"
    );
}

#[test]
fn saved_evidence_outside_the_runs_requests_fulfils_gates() {
    // A partial Run of c: a and b have no requests; the Run recorded their saved results.
    let definitions = definitions(
        &[(&["a"], &[]), (&["b"], &[0]), (&["c"], &[1])],
        vec![
            runtime("a/x"),
            runtime("b/x"),
            runtime("c/x"),
            runtime("c/y"),
        ],
    );
    let requests = vec![
        request("c/x", "RUNNING", json!({})),
        request("c/y", "QUEUED", json!({})),
    ];
    let (mut run, requests) = saved(definitions.clone(), true, requests);
    run.run.evidence = [
        ("a/x".parse().unwrap(), crate::types::RequestStatus::Green),
        ("b/x".parse().unwrap(), crate::types::RequestStatus::Green),
    ]
    .into();
    let nodes = tree(&run, &requests, now());
    assert_eq!(view(&nodes, "b/x"), &EvalView::Done(Source::Saved));
    assert_eq!(
        line(&nodes, "e:b/x"),
        "✓ x  GREEN · saved result, not in this Run"
    );
    assert_eq!(line(&nodes, "a:b"), "✓ b  1/1");
    assert_eq!(
        view(&nodes, "c/y"),
        &EvalView::InProgress(Activity::Queued(Queue::Ready))
    );
    // A saved RED outside the Run blocks like any other.
    run.run
        .evidence
        .insert("b/x".parse().unwrap(), crate::types::RequestStatus::Red);
    let nodes = tree(&run, &requests, now());
    assert_eq!(view(&nodes, "c/y"), &EvalView::BlockedBy(x(&["b"])));
    // Runs saved before the record: the validation saved at their end.
    let requests = vec![
        request("c/x", "GREEN", json!({})),
        request("c/y", "GREEN", json!({})),
    ];
    let (mut run, requests) = saved(definitions, false, requests);
    run.run.validation = serde_json::from_value(json!({"satisfied":true,"evals":[
        {"id":"a/x","status":"GREEN"},{"id":"b/x","status":"GREEN"},
        {"id":"c/x","status":"GREEN"},{"id":"c/y","status":"GREEN"}]}))
    .unwrap();
    let nodes = tree(&run, &requests, now());
    assert_eq!(line(&nodes, "a:c"), "✓ c  2/2");
    assert!(find(&nodes, "a:c").done());
    assert_eq!(view(&nodes, "a/x"), &EvalView::Done(Source::Saved));
}

#[test]
fn a_mixed_red_and_error_artifact_looks_the_same_everywhere() {
    let definitions = definitions(
        &[(&["a"], &[]), (&["b"], &[0])],
        vec![runtime("a/x"), runtime("a/y"), runtime("b/x")],
    );
    let requests = vec![
        request("a/x", "RED", json!({})),
        request("a/y", "ERROR", json!({"error":"spawn failed"})),
        request("b/x", "QUEUED", json!({})),
    ];
    let (run, requests) = saved(definitions, true, requests);
    let nodes = tree(&run, &requests, now());
    assert_eq!(line(&nodes, "a:a"), "! a  ERROR: y  0/2");
    // ERROR-first in X tokens too; the RED still blocks.
    assert_eq!(
        line(&nodes, "e:b/x"),
        "⊘ x  blocked by a ! failed (retry a first)"
    );
    assert_eq!(
        find(&nodes, "e:b/x").upstream[0].completion,
        Completion::Failed { verdict: false }
    );
}

#[test]
fn root_cause_is_the_first_x_own_blocker_only() {
    // a ← b ← c ← d: a runs, the rest wait.
    let definitions = definitions(
        &[
            (&["a"], &[]),
            (&["b"], &[0]),
            (&["c"], &[1]),
            (&["d"], &[2]),
        ],
        vec![
            runtime("a/x"),
            runtime("b/x"),
            runtime("c/x"),
            runtime("d/x"),
        ],
    );
    let requests = vec![
        request("a/x", "RUNNING", json!({})),
        request("b/x", "QUEUED", json!({})),
        request("c/x", "QUEUED", json!({})),
        request("d/x", "QUEUED", json!({})),
    ];
    let (run, requests) = saved(definitions, true, requests);
    let nodes = tree(&run, &requests, now());
    assert_eq!(line(&nodes, "e:b/x"), "… x  waits for a ◐ in progress");
    assert_eq!(
        line(&nodes, "e:c/x"),
        "… x  waits for b … waiting (b waits for a)"
    );
    assert_eq!(
        line(&nodes, "e:d/x"),
        "… x  waits for c … waiting (c waits for b)"
    );
}

#[test]
fn effective_statuses_saved_at_run_end_are_not_changes() {
    // A RED, B's reused GREEN masked BLOCKED, C saved BLOCKED: nothing changed afterwards.
    let nodes = chain(
        false,
        [
            ("RED", json!({"completedAt":"2026-01-01T00:04:00Z"})),
            ("GREEN", reused()),
            ("BLOCKED", json!({"completedAt":"2026-01-01T00:05:00Z"})),
        ],
    );
    assert_eq!(
        line(&nodes, "a:cli"),
        "⊘ cli  done, but blocked by code-style  0/1"
    );
    assert_eq!(
        line(&nodes, "e:docs/matches-cli"),
        "⊘ matches-cli  not run: blocked by cli ⊘ blocked"
    );
    assert!(nodes.iter().all(|node| !node.changed), "{nodes:#?}");
}

/// Rows marked with `*`, as `id` → changed.
fn marks(nodes: &[Node]) -> Vec<(model::NodeId, bool)> {
    nodes
        .iter()
        .flat_map(|node| std::iter::once(node).chain(&node.children))
        .map(|node| (node.id.clone(), node.changed))
        .collect()
}

fn human(eval: &str, status: &str, extra: Value) -> RequestView {
    let mut extra = extra;
    extra["profile"] = json!({"kind":"human"});
    request(eval, status, extra)
}

#[test]
fn changes_after_the_run_are_a_diff_against_the_run_end_derivation() {
    let late = json!({"completedAt":"2026-01-01T00:06:00Z"});
    // (a) One of two Human evals of a is approved later: a changes, b still waits for a.
    let pair = definitions(
        &[(&["a"], &[]), (&["b"], &[0])],
        vec![runtime("a/one"), runtime("a/two"), runtime("b/x")],
    );
    let requests = vec![
        human("a/one", "GREEN", late.clone()),
        human("a/two", "WAITING_HUMAN", json!({})),
        request("b/x", "WAIT_DEPENDENCY", json!({})),
    ];
    let (run, requests) = saved(pair, false, requests);
    let nodes = tree(&run, &requests, now());
    assert_eq!(
        marks(&nodes),
        [
            (
                "a:a".parse::<crate::monitor::model::NodeId>().unwrap(),
                true
            ),
            (
                "e:a/one".parse::<crate::monitor::model::NodeId>().unwrap(),
                true
            ),
            (
                "e:a/two".parse::<crate::monitor::model::NodeId>().unwrap(),
                false
            ),
            (
                "a:b".parse::<crate::monitor::model::NodeId>().unwrap(),
                false
            ),
            (
                "e:b/x".parse::<crate::monitor::model::NodeId>().unwrap(),
                false
            ),
        ]
    );
    assert_eq!(
        line(&nodes, "e:b/x"),
        "○ x  not run: waited for a ? in progress"
    );

    // (b) a's late GREEN opens the gate of b's reused GREEN: b completes, c can run.
    let three = definitions(
        &[(&["a"], &[]), (&["b"], &[0]), (&["c"], &[1])],
        vec![runtime("a/one"), runtime("b/x"), runtime("c/x")],
    );
    let requests = vec![
        human("a/one", "GREEN", late.clone()),
        request("b/x", "GREEN", reused()),
        request("c/x", "WAIT_DEPENDENCY", json!({})),
    ];
    let (run, requests) = saved(three.clone(), false, requests);
    let nodes = tree(&run, &requests, now());
    assert_eq!(line(&nodes, "a:b"), "✓ b  1/1 *");
    assert_eq!(line(&nodes, "e:b/x"), "✓ x  GREEN · reused");
    assert_eq!(line(&nodes, "a:c"), "○ c  not reviewed  0/1 *");
    assert_eq!(line(&nodes, "e:c/x"), "○ x  not reviewed *");

    // A later claim alone changes the Human row, nothing downstream.
    let mut waiting = human("a/one", "WAITING_HUMAN", json!({}));
    waiting.claim = Some(crate::store::HumanClaim {
        request_id: waiting.request.id.clone(),
        reviewer: "hj".parse().unwrap(),
        claimed_at: "2026-01-01T00:07:00Z".parse().unwrap(),
    });
    let requests = vec![
        waiting,
        request("b/x", "WAIT_DEPENDENCY", json!({})),
        request("c/x", "WAIT_DEPENDENCY", json!({})),
    ];
    let (run, requests) = saved(three, false, requests);
    let nodes = tree(&run, &requests, now());
    assert_eq!(
        marks(&nodes)
            .into_iter()
            .filter(|(_, changed)| *changed)
            .collect::<Vec<_>>(),
        [(
            "e:a/one".parse::<crate::monitor::model::NodeId>().unwrap(),
            true
        )]
    );
}

#[tokio::test]
async fn detail_shows_waits_for_as_a_section_with_pending_evals_behind_w() {
    let (_monitor_root, mut monitor) = tests::monitor();
    wide(&mut monitor);
    monitor.tree.select(vec![
        "a:m1".parse::<crate::monitor::model::NodeId>().unwrap(),
        "e:m1/x".parse::<crate::monitor::model::NodeId>().unwrap(),
    ]);
    // The peek names the Artifacts and how many of their evals are pending.
    let text = render(&mut monitor, 200, 40);
    assert!(text.contains("Waits for:"), "{text}");
    assert!(text.contains("↑ up-a ◐ in progress"), "{text}");
    assert!(text.contains("1 pending evals · w shows"), "{text}");
    monitor.open_detail().await;
    let text = render(&mut monitor, 200, 40);
    let at = |text: &str, needle: &str| {
        text.find(needle)
            .unwrap_or_else(|| panic!("{needle}\n{text}"))
    };
    assert!(at(&text, "Outcome") < at(&text, "Waits for"));
    assert!(at(&text, "Waits for") < at(&text, "What"));
    assert!(!text.contains("◐ x  running"), "{text}");
    monitor.key(KeyEvent::from(KeyCode::Char('w')));
    let text = render(&mut monitor, 200, 40);
    assert!(text.contains("◐ x  running"), "{text}");
    assert!(!text.contains("w shows"), "{text}");
}

fn lit(graph: &Graph) -> Vec<(String, Light)> {
    graph
        .nodes
        .iter()
        .map(|node| (node.name.to_string(), node.light))
        .collect()
}

fn lit_edges(graph: &Graph) -> Vec<(String, String, Light)> {
    graph
        .edges
        .iter()
        .map(|edge| (edge.from.to_string(), edge.to.to_string(), edge.light))
        .collect()
}

#[test]
fn the_graph_lights_artifacts_with_the_six_states_of_the_tree() {
    use Light::{Active, Blocked, Error, Green, Red, Waiting};
    let started = json!({"startedAt":"2026-01-01T00:08:00Z"});
    // code-style ← cli ← docs: statuses, then the three lights.
    for (statuses, lights) in [
        // A RED upstream blocks everything after it.
        (["RED", "QUEUED", "QUEUED"], [Red, Blocked, Blocked]),
        // ERROR is its own light; downstream of it is blocked too, though the tree says
        // "waits for … (retry first)".
        (["ERROR", "QUEUED", "QUEUED"], [Error, Blocked, Blocked]),
        (["GREEN", "ERROR", "QUEUED"], [Green, Error, Blocked]),
        // Upstream in progress: downstream waits, which is not blocked.
        (["RUNNING", "QUEUED", "QUEUED"], [Active, Waiting, Waiting]),
        (
            ["GREEN", "WAITING_HUMAN", "QUEUED"],
            [Green, Active, Waiting],
        ),
        // Queued with nothing upstream holding it: runnable, still waiting.
        (["GREEN", "GREEN", "QUEUED"], [Green, Green, Waiting]),
        (["GREEN", "GREEN", "GREEN"], [Green, Green, Green]),
    ] {
        let nodes = chain(true, statuses.map(|status| (status, started.clone())));
        let graph = graph(&nodes);
        let names = ["code-style", "cli", "docs"].map(String::from);
        assert_eq!(
            lit(&graph),
            names.iter().cloned().zip(lights).collect::<Vec<_>>(),
            "{statuses:?}"
        );
        // Each edge takes its upstream Artifact's light.
        assert_eq!(
            lit_edges(&graph),
            vec![
                ("code-style".into(), "cli".into(), lights[0]),
                ("cli".into(), "docs".into(), lights[1]),
            ],
            "{statuses:?}"
        );
    }
}

#[test]
fn an_artifact_takes_the_first_light_of_its_evals_in_precedence_order() {
    use Light::{Active, Blocked, Error, Green, Red, Waiting};
    // app has two independent runtime Evals and two referencing runtime Evals, one on base and one on lib; app's
    // runtime evals are not gated.
    let definitions = definitions(
        &[(&["base"], &[]), (&["lib"], &[]), (&["app"], &[])],
        vec![
            runtime("base/check"),
            runtime("lib/check"),
            runtime("app/one"),
            runtime("app/two"),
            referenced("app/needs-base", &["base"]),
            referenced("app/needs-lib", &["lib"]),
        ],
    );
    let started = json!({"startedAt":"2026-01-01T00:08:00Z"});
    let app = |statuses: [&str; 6]| {
        let requests = [
            "base/check",
            "lib/check",
            "app/one",
            "app/two",
            "app/needs-base",
            "app/needs-lib",
        ]
        .into_iter()
        .zip(statuses)
        .map(|(eval, status)| request(eval, status, started.clone()))
        .collect();
        let (run, requests) = saved(definitions.clone(), true, requests);
        graph(&tree(&run, &requests, now())).nodes[2].light
    };
    let wait = "WAIT_DEPENDENCY";
    for (statuses, light) in [
        // RED before ERROR.
        (["GREEN", "GREEN", "RED", "ERROR", "GREEN", "GREEN"], Red),
        // ERROR before in progress.
        (
            ["GREEN", "GREEN", "ERROR", "RUNNING", "GREEN", "GREEN"],
            Error,
        ),
        (
            ["GREEN", "GREEN", "ERROR", "WAITING_HUMAN", "GREEN", "GREEN"],
            Error,
        ),
        // In progress before blocked: base RED blocks needs-base.
        (["RED", "GREEN", "RUNNING", "GREEN", wait, "GREEN"], Active),
        // Blocked before waiting: base RED blocks needs-base, lib running holds needs-lib.
        (["RED", "RUNNING", "GREEN", "GREEN", wait, wait], Blocked),
        // An ERROR upstream blocks too.
        (["ERROR", "RUNNING", "GREEN", "GREEN", wait, wait], Blocked),
        // Waiting before GREEN.
        (
            ["GREEN", "RUNNING", "GREEN", "GREEN", "GREEN", wait],
            Waiting,
        ),
        (
            ["GREEN", "GREEN", "GREEN", "GREEN", "GREEN", "GREEN"],
            Green,
        ),
    ] {
        assert_eq!(app(statuses), light, "{statuses:?}");
    }
}

#[test]
fn an_artifact_without_evals_is_green() {
    let definitions = definitions(
        &[(&["notes"], &[]), (&["app"], &[0])],
        vec![runtime("app/check")],
    );
    let (run, requests) = saved(
        definitions,
        true,
        vec![request("app/check", "GREEN", json!({}))],
    );
    let graph = graph(&tree(&run, &requests, now()));
    assert_eq!(graph.nodes[0].light, Light::Green);
}

#[test]
fn a_cycle_peer_neither_blocks_nor_holds_an_eval() {
    let definitions = definitions(
        &[(&["a", "b"], &[])],
        vec![
            referenced("a/cycle", &["b"]),
            runtime("a/input"),
            referenced("b/cycle", &["a"]),
        ],
    );
    let requests = vec![
        request("a/cycle", "RED", json!({})),
        request("a/input", "RUNNING", json!({})),
        request("b/cycle", "QUEUED", json!({})),
    ];
    let (run, requests) = saved(definitions, true, requests);
    let nodes = tree(&run, &requests, now());
    assert_eq!(view(&nodes, "a/cycle"), &EvalView::Failed { verdict: true });
    // Neither the peer's RED nor its running sibling holds b/cycle: it only awaits admission.
    assert!(matches!(
        view(&nodes, "b/cycle"),
        EvalView::InProgress(Activity::Queued(_))
    ));
    let graph = graph(&nodes);
    assert_eq!(
        graph
            .nodes
            .iter()
            .find(|node| node.name == "b")
            .unwrap()
            .light,
        Light::Waiting
    );
}
