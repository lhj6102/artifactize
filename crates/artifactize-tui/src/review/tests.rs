use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{Terminal, backend::TestBackend};
use serde_json::{Value, json};

use super::*;
use crate::{
    config::HumanToolKind,
    tools::human::{Content, ToolResult},
};

const ID: &str = "run-1-3";

/// Saved claims and requests use explicit times; rendering measures their age at this instant.
pub(crate) fn now() -> OffsetDateTime {
    OffsetDateTime::parse(
        "2026-01-01T00:01:05Z",
        &time::format_description::well_known::Rfc3339,
    )
    .unwrap()
}

#[test]
fn spinner_cycles_every_hundred_milliseconds() {
    assert_eq!(view::spinner(Duration::from_millis(0)), "⠋");
    assert_eq!(view::spinner(Duration::from_millis(99)), "⠋");
    assert_eq!(view::spinner(Duration::from_millis(100)), "⠙");
    assert_eq!(view::spinner(Duration::from_millis(999)), "⠏");
    assert_eq!(view::spinner(Duration::from_millis(1000)), "⠋");
    assert_eq!(view::spinner(Duration::from_millis(1100)), "⠙");
}

fn definition(pass: Value, fail: Value) -> Value {
    json!({
        "repo":"repo",
        "eval":{
            "id":"release/signoff",
            "target":"release",
            "references":{},
            "deps":[],
            "declaration":{
                "id":"signoff",
                "title":"Approve",
                "profile":{"kind":"human"},
                "payload":{"instruction":"Approve the notes."},
                "passSchema":pass,
                "failSchema":fail,
            },
        },
        "artifacts":{
            "release":{
                "path":"release",
                "views":{
                    "agentTools":{},
                    "humanTools":{
                        "notes":{
                            "description":"Print the notes of {artifactName}.",
                            "kind":"output",
                            "command":"get",
                            "args":["{artifactPath}/notes.md"],
                        },
                        "open":{
                            "description":"Open {artifactName}.",
                            "kind":"launch",
                            "command":"show-release",
                            "args":["{artifactPath}"],
                        },
                    },
                },
            },
        },
    })
}

/// The review-demo schemas: GREEN needs `approved: const true`, RED a non-empty reason.
pub(crate) fn demo() -> Value {
    definition(
        json!({
            "type":"object",
            "properties":{"approved":{"const":true}},
            "required":["approved"],
            "additionalProperties":false,
        }),
        json!({
            "type":"object",
            "properties":{"reason":{"type":"string","minLength":1}},
            "required":["reason"],
            "additionalProperties":false,
        }),
    )
}

/// The demo request with builtin Human tools beside the `notes` command tool: `release` at
/// `release` references `guide`, a folder at `docs/guide`.
pub(crate) fn builtins() -> Value {
    let mut definition = demo();
    let artifacts = &mut definition["artifacts"];
    artifacts["release"]["views"]["humanTools"] = json!({
        "notes":{
            "description":"Print the notes of {artifactName}.",
            "kind":"output",
            "command":"get",
            "args":["{artifactPath}/notes.md"],
        },
        "open":{
            "builtin":"open",
            "description":"Open the notes of {artifactName}.",
            "kind":"launch",
            "args":["{artifactPath}/notes.md"],
        },
        "changes":{
            "builtin":"read",
            "description":"Read the changelog.",
            "kind":"output",
            "args":["CHANGELOG.md"],
        },
        "install":{
            "builtin":"section",
            "description":"Read the install steps.",
            "kind":"output",
            "args":["{guide}/README.md","Install"],
        },
        "cargo":{
            "builtin":"help",
            "description":"Show the build options.",
            "kind":"output",
            "args":["cargo","build"],
        },
        "assets":{
            "builtin":"list",
            "description":"List the guide.",
            "kind":"output",
            "args":["{guide}"],
        },
        "site":{
            "builtin":"open",
            "description":"Open the release page.",
            "kind":"launch",
            "args":["https://example.com/release"],
        },
    });
    artifacts["guide"] = json!({
        "path":"docs/guide",
        "kind":"folder",
        "views":{"agentTools":{},"humanTools":{}},
    });
    definition
}

pub(crate) fn claim(reviewer: &str) -> HumanClaim {
    HumanClaim {
        request_id: ID.parse().unwrap(),
        reviewer: reviewer.parse().unwrap(),
        claimed_at: "2026-01-01T00:00:30Z".parse().unwrap(),
    }
}

fn view(status: &str, reviewer: Option<&str>, definition: Value) -> RequestView {
    RequestView {
        request: serde_json::from_value(json!({
            "id":ID,
            "runId":"run-1",
            "evalId":"release/signoff",
            "target":"release",
            "title":"Approve",
            "profile":{"kind":"human"},
            "requestedProfile":{"kind":"human"},
            "evalDefHash":"dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd",
            "payload":{"instruction":"Approve the notes."},
            "references":{},
            "deps":[],
            "status":status,
            "result": if matches!(status, "GREEN" | "RED") { json!({"verdict":status}) } else { Value::Null },
            "error": if status == "ERROR" { json!("fixture error") } else { Value::Null },
            "completedAt": if matches!(status, "GREEN" | "RED" | "ERROR") { json!("2026-01-01T00:00:01Z") } else { Value::Null },
            "createdAt":"2026-01-01T00:00:00Z",
            "cwd":"repo",
            "humanDefinition":definition,
        }))
        .unwrap(),
        claim: reviewer.map(claim),
        execution: None,
        definition: None,
    }
}

pub(crate) fn opened(reviewer: Option<&str>, definition: Value) -> Review {
    let mut review = Review::new(
        "fixture-state".into(),
        Some("repo".into()),
        "alice".parse().unwrap(),
        Some(ID.parse().unwrap()),
    );
    review.shown = Shown::Human(Box::new(view("WAITING_HUMAN", reviewer, definition)));
    review
}

/// The same request as monitor opens it: a single-request review.
pub(crate) fn embedded(reviewer: Option<&str>, definition: Value) -> Review {
    let mut review = opened(reviewer, definition);
    let view = review.shown.request().cloned().unwrap();
    review.load_single(view);
    review
}

/// A completed request as monitor opens it.
pub(crate) fn settled(status: &str) -> Review {
    let mut review = embedded(None, demo());
    review.load_single(view(status, None, demo()));
    review
}

/// A claimed review editing a RED draft whose request then completes elsewhere and reloads.
pub(crate) fn settled_while_editing() -> Review {
    let mut review = embedded(Some("alice"), demo());
    review.control(Control::Red);
    review.paste_single("draft");
    assert!(review.editing());
    review.load_single(view("GREEN", None, demo()));
    review
}

fn press(review: &mut Review, code: KeyCode) -> Action {
    review.key(KeyEvent::from(code))
}

fn control(review: &mut Review, c: char) -> Action {
    review.key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL))
}

fn command(args: &[&str]) -> CommandLine {
    CommandLine {
        repo: "repo".into(),
        kind: HumanToolKind::Output,
        program: "get".into(),
        args: args.iter().map(|arg| (*arg).to_owned()).collect(),
        cwd: "repo/release".into(),
    }
}

fn printed(text: &str, is_error: bool) -> Result<ToolResult, String> {
    Ok(if is_error {
        ToolResult::error(text)
    } else {
        ToolResult::success(Content::Text { text: text.into() })
    })
}

/// One run of the notes tool in the review's Tool log, as the library logs it: its start, what
/// it printed and its exit.
fn logged(review: &mut Review, stdout: &str, stderr: &str, code: i32) {
    let run = review.sink.start(&ID.parse().unwrap(), "notes", "release");
    run.text(log::Stream::Stdout, stdout);
    run.text(log::Stream::Stderr, stderr);
    run.end(log::End::Exited {
        code: Some(code),
        text: format!("exit status: {code}"),
    });
}

fn screen(review: &mut Review) -> String {
    sized(review, 140, 40)
}

pub(crate) fn sized(review: &mut Review, width: u16, height: u16) -> String {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|frame| review.draw(frame)).unwrap();
    let buffer = terminal.backend().buffer();
    let rows = buffer.content().chunks(buffer.area.width as usize);
    let rows = rows.map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>());
    rows.collect::<Vec<_>>().join("\n")
}

/// Claim the open request through the claim job's outcome, as the driver would.
fn claimed(review: &mut Review) {
    assert_eq!(
        press(review, KeyCode::Char('c')),
        Action::Start(Job::Claim {
            id: ID.parse().unwrap()
        })
    );
    assert_eq!(
        review.finish(Outcome::Claimed {
            result: Ok(claim("alice"))
        }),
        Action::Refresh
    );
    assert!(review.owned());
}

fn form(review: &Review) -> &Form {
    match review.mode() {
        Mode::Form(form) => form,
        mode => panic!("not a form: {mode:?}"),
    }
}

#[test]
fn instruction_leads_and_technical_details_stay_folded_until_t() {
    let mut review = opened(None, demo());
    let tools = review.tools();
    assert_eq!(
        tools
            .iter()
            .map(|tool| (tool.name.as_str(), tool.kind, &tool.declared))
            .collect::<Vec<_>>(),
        [
            (
                "notes_release",
                HumanToolKind::Output,
                &Runs::Command("get {artifactPath}/notes.md".into())
            ),
            (
                "open_release",
                HumanToolKind::Launch,
                &Runs::Command("show-release {artifactPath}".into())
            ),
        ]
    );
    assert_eq!(tools[0].description, "Print the notes of release.");
    assert_eq!(
        shell(["tool", "-c", "echo 'hi' $THING", ""]),
        r#"tool -c "echo 'hi' $THING" """#
    );
    let text = screen(&mut review);
    for expected in [
        "release/signoff · Approve · CLAIM",
        "WAITING_HUMAN · unclaimed · waiting 1m 05s",
        "t technical ▸",
        "Approve the notes.",
        "Tools  notes_release (output) · open_release (launch)",
        "[Claim c]",
    ] {
        assert!(text.contains(expected), "{expected}\n{text}");
    }
    // Identifiers and raw schemas wait in the folded Technical section.
    for hidden in ["Request: run-1-3", "Repository: repo", "GREEN fields:"] {
        assert!(!text.contains(hidden), "{hidden}\n{text}");
    }
    press(&mut review, KeyCode::Char('t'));
    let text = screen(&mut review);
    for expected in [
        "t technical ▾",
        "Request: run-1-3",
        "Run: run-1",
        "Repository: repo",
        "Status: WAITING_HUMAN",
        "Claim: unclaimed; c claims it for alice",
        "GREEN fields: {",
        r#""const": true"#,
    ] {
        assert!(text.contains(expected), "{expected}\n{text}");
    }
    // The raw schemas are pretty JSON; Ctrl-PgDn scrolls to the rest.
    review.key(KeyEvent::new(KeyCode::PageDown, KeyModifiers::CONTROL));
    let text = screen(&mut review);
    for expected in ["RED fields: {", r#""minLength": 1"#] {
        assert!(text.contains(expected), "{expected}\n{text}");
    }
    // Tools focus shows the selected tool's command and description.
    press(&mut review, KeyCode::Char('t'));
    press(&mut review, KeyCode::Tab);
    assert_eq!(review.area(), Area::Tools);
    let text = screen(&mut review);
    for expected in [
        "notes_release  output",
        "$ get {artifactPath}/notes.md",
        "Print the notes of release.",
        "open_release  launch",
        "Unclaimed. c claims it for alice",
    ] {
        assert!(text.contains(expected), "{expected}\n{text}");
    }
    assert!(!text.contains("show-release"), "{text}");
}

#[test]
fn shared_requests_name_where_actions_go_in_the_metadata() {
    let mut review = opened(None, demo());
    let execution = |request: &str| {
        let provenance = json!({"repoPath":"origin","runId":"run-0","requestId":request,
            "evalId":"release/signoff","evalDefHash":"dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd","completedAt":null});
        serde_json::from_value(
            json!({"id":"execution-fixture","key":null,"fingerprint":null,
            "evalDefHash":"dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd","ownerPid":0,"ownerStartTime":0,"status":"WAITING_HUMAN",
            "result":null,"error":null,"errorCode":null,"profile":{"kind":"human"},"usage":null,
            "provenance":provenance,"startedAt":"2026-01-01T00:00:00Z","completedAt":null}),
        )
        .unwrap()
    };
    // The request's own execution is not shared.
    review.shown.request_mut().unwrap().execution = Some(execution(ID));
    let text = screen(&mut review);
    assert!(!text.contains("actions go to"), "{text}");
    review.shown.request_mut().unwrap().execution = Some(execution("run-0-9"));
    let text = screen(&mut review);
    assert!(text.contains("actions go to run-0-9 in origin"), "{text}");
}

#[test]
fn tools_need_a_claim_then_enter_runs_at_once_showing_the_resolved_command() {
    let mut review = opened(None, demo());
    press(&mut review, KeyCode::Tab);
    assert_eq!(press(&mut review, KeyCode::Enter), Action::None);
    assert!(screen(&mut review).contains("Claim this request before reviewing it."));
    claimed(&mut review);
    assert_eq!(review.taken, [ID.parse::<RequestId>().unwrap()]);
    // The Tools pane shows what Enter runs: the resolved command line once known.
    let resolved = |review: &mut Review| {
        review.commands = Some((
            ID.parse().unwrap(),
            [
                (
                    "notes_release".parse().unwrap(),
                    Ok(command(&["repo/release/notes.md"])),
                ),
                (
                    "open_release".parse().unwrap(),
                    Err("Tool executable path is unavailable.".into()),
                ),
            ]
            .into_iter()
            .collect(),
        ));
    };
    resolved(&mut review);
    let text = screen(&mut review);
    assert!(text.contains("$ get repo/release/notes.md"), "{text}");
    assert!(!text.contains("{artifactPath}"), "{text}");
    let run = |tool: &str| {
        Action::Start(Job::Run {
            id: ID.parse().unwrap(),
            tool: tool.parse().unwrap(),
            claim: false,
        })
    };
    // No confirmation: Enter starts the run job every time.
    assert_eq!(press(&mut review, KeyCode::Enter), run("notes_release"));
    let ran = review.finish(Outcome::Ran {
        tool: "notes_release".parse().unwrap(),
        claimed: None,
        result: printed("# Release notes\n", false),
    });
    assert_eq!(ran, Action::Refresh);
    logged(&mut review, "# Release notes\n", "", 0);
    let text = screen(&mut review);
    assert!(
        text.contains("Tool log")
            && text.contains(" · notes · release")
            && text.contains("# Release notes")
            && text.contains("■ exit 0"),
        "{text}"
    );
    assert!(text.contains("claimed by you (alice)"), "{text}");
    assert_eq!(press(&mut review, KeyCode::Enter), run("notes_release"));
    // A run drops the resolved commands until the next refresh resolves them again; a tool
    // that cannot be resolved shows its declaration and why.
    let text = screen(&mut review);
    assert!(text.contains("$ get {artifactPath}/notes.md"), "{text}");
    assert!(!text.contains("$ get repo/release/notes.md"), "{text}");
    resolved(&mut review);
    press(&mut review, KeyCode::Down);
    let text = screen(&mut review);
    assert!(
        text.contains("$ show-release {artifactPath}")
            && text.contains("Tool executable path is unavailable."),
        "{text}"
    );

    review.finish(Outcome::Ran {
        tool: "notes_release".parse().unwrap(),
        claimed: None,
        result: printed("Human tool exited unsuccessfully (exit status: 1).\n", true),
    });
    logged(&mut review, "", "notes are missing\n", 1);
    let text = screen(&mut review);
    assert!(
        text.contains("notes are missing") && text.contains("■ exit 1"),
        "{text}"
    );
    assert!(
        text.contains("notes_release failed; a tool error is not a verdict."),
        "{text}"
    );
    review.finish(Outcome::Ran {
        tool: "open_release".parse().unwrap(),
        claimed: None,
        result: Ok(ToolResult::success(Content::Launch { launched: true })),
    });
    assert!(screen(&mut review).contains("open_release launched."));
    review.finish(Outcome::Ran {
        tool: "notes_release".parse().unwrap(),
        claimed: None,
        result: Err("Human Artifact scope or eval declarations changed.".into()),
    });
    assert_eq!(review.mode(), &Mode::Request);
    assert!(screen(&mut review).contains("notes_release did not run."));
}

#[test]
fn requests_claimed_by_others_or_settled_are_read_only() {
    let mut review = opened(Some("bob"), demo());
    let text = screen(&mut review);
    assert!(text.contains("claimed by bob · read-only"), "{text}");
    assert!(
        text.contains("Claimed by bob. Read-only until released."),
        "{text}"
    );
    assert_eq!(press(&mut review, KeyCode::Char('c')), Action::None);
    assert!(screen(&mut review).contains("Claimed by bob; read-only for alice."));
    for key in [KeyCode::Char('g'), KeyCode::Char('r'), KeyCode::Char('u')] {
        assert_eq!(press(&mut review, key), Action::None);
        assert_eq!(review.mode(), &Mode::Request);
        assert!(screen(&mut review).contains("Claim this request before reviewing it."));
    }
    let mut review = opened(None, demo());
    review.shown = Shown::Human(Box::new(view("GREEN", None, demo())));
    for key in [KeyCode::Char('c'), KeyCode::Char('g')] {
        assert_eq!(press(&mut review, key), Action::None);
        assert_eq!(review.mode(), &Mode::Request);
    }
    let text = screen(&mut review);
    assert!(
        text.contains("release/signoff · Approve · completed") && text.contains("Completed result"),
        "{text}"
    );
    assert!(!text.contains("[Claim c]"), "{text}");
}

#[test]
fn review_keys_need_detail_focus_and_an_owned_request() {
    // On the list, review letters do nothing; c never claims there.
    let mut review = Review::new("fixture-state".into(), None, "alice".parse().unwrap(), None);
    review.set_listed(vec![view("WAITING_HUMAN", None, demo())]);
    for key in ['c', 'g', 'u', 'i', 't'] {
        assert_eq!(
            press(&mut review, KeyCode::Char(key)),
            Action::None,
            "{key}"
        );
        assert_eq!(review.focus(), Focus::List);
        assert!(review.notice.is_none());
    }
    for c in ['s', 'g', 'r'] {
        assert!(!matches!(control(&mut review, c), Action::Start(_)));
        assert_eq!(
            (review.focus(), review.mode()),
            (Focus::List, &Mode::Request)
        );
    }
    // In Detail, Ctrl-S, Ctrl-G and Ctrl-R start nothing before a claim.
    let mut review = opened(None, demo());
    // ? and ! pass to the caller outside a form; the standalone review has no use for them.
    for key in ['?', '!'] {
        assert_eq!(
            review.key_detail(KeyEvent::from(KeyCode::Char(key))),
            Handled::Pass
        );
        assert_eq!(press(&mut review, KeyCode::Char(key)), Action::None);
        assert_eq!(review.focus(), Focus::Detail);
    }
    for c in ['s', 'g', 'r'] {
        assert_eq!(control(&mut review, c), Action::None, "{c}");
        assert_eq!(review.mode(), &Mode::Request);
        assert!(screen(&mut review).contains("Claim this request before reviewing it."));
    }
    claimed(&mut review);
    assert_eq!(control(&mut review, 's'), Action::None);
    assert!(screen(&mut review).contains("Choose GREEN or RED first."));
    assert_eq!(control(&mut review, 'r'), Action::None);
    assert!(review.editing());
    assert_eq!(review.area(), Area::Fields);
    // While a field is edited, letters are text and focus cannot leave the Detail.
    for key in [
        KeyCode::Char('q'),
        KeyCode::Char('c'),
        KeyCode::Char('i'),
        KeyCode::Char('t'),
        KeyCode::Left,
        KeyCode::Tab,
    ] {
        assert_eq!(press(&mut review, key), Action::None);
        assert_eq!(review.focus(), Focus::Detail);
        assert!(review.editing());
        assert_eq!(review.area(), Area::Fields);
    }
    assert_eq!(form(&review).fields[0].display(), "qcit");
    assert_eq!(
        control(&mut review, 's'),
        Action::Start(Job::Submit {
            id: ID.parse().unwrap(),
            result: json!({"verdict":"RED","reason":"qcit"}).try_into().unwrap(),
            claim: false
        })
    );
    // Esc steps back one thing at a time: the form, then the Detail; it never quits.
    assert_eq!(press(&mut review, KeyCode::Esc), Action::None);
    assert!(!review.editing());
    assert_eq!(review.focus(), Focus::Detail);
    assert_eq!(press(&mut review, KeyCode::Esc), Action::Refresh);
    assert_eq!(review.focus(), Focus::List);
    assert_eq!(press(&mut review, KeyCode::Esc), Action::None);
    // The draft survives for the next open of the same request.
    review.set_listed(vec![view("WAITING_HUMAN", Some("alice"), demo())]);
    assert_eq!(press(&mut review, KeyCode::Enter), Action::Refresh);
    press(&mut review, KeyCode::Char('r'));
    assert_eq!(form(&review).fields[0].display(), "qcit");
}

#[test]
fn i_expands_the_instruction_and_tools_focus_widens_the_tools() {
    let mut review = opened(Some("alice"), demo());
    let instruction = (0..30)
        .map(|n| format!("criterion-{n}\n"))
        .collect::<String>();
    std::sync::Arc::make_mut(&mut review.shown.request_mut().unwrap().request).payload =
        serde_json::from_value(json!({"instruction":instruction})).unwrap();
    let hits = |review: &mut Review| {
        let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
        let mut hits = None;
        terminal
            .draw(|frame| hits = Some(review.draw_detail(frame, frame.area(), true)))
            .unwrap();
        hits.unwrap()
    };
    // In REVIEW the instruction folds to two rows above the tools and fields.
    let folded = hits(&mut review);
    assert_eq!(folded.instruction.height, 4);
    assert!(folded.fields.width > folded.tools.width);
    press(&mut review, KeyCode::Char('i'));
    assert_eq!(review.area(), Area::Instruction);
    let expanded = hits(&mut review);
    assert!(expanded.instruction.height > 20, "{expanded:?}");
    // Arrow keys scroll the focused instruction; i folds it again.
    press(&mut review, KeyCode::Down);
    assert_eq!(review.instruction_scroll, 1);
    press(&mut review, KeyCode::Char('i'));
    assert_eq!(review.area(), Area::Fields);
    press(&mut review, KeyCode::Tab);
    let tools = hits(&mut review);
    assert!(tools.tools.width > tools.fields.width, "{tools:?}");
    assert_eq!(
        tools.tool_rows.len(),
        4,
        "the selected tool shows two more rows"
    );
    // Ctrl-PgUp/PgDn keeps scrolling the instruction from any sub-area.
    review.key(KeyEvent::new(KeyCode::PageDown, KeyModifiers::CONTROL));
    assert_eq!(review.instruction_scroll, 11);
}

#[tokio::test]
async fn demo_schemas_fill_forms_and_submission_errors_return_to_them() {
    let state = crate::test_os::tempdir();
    let mut review = Review::new(
        state.path().join("state"),
        None,
        "alice".parse().unwrap(),
        Some(ID.parse().unwrap()),
    );
    review.shown = Shown::Human(Box::new(view("WAITING_HUMAN", None, demo())));
    claimed(&mut review);
    assert_eq!(press(&mut review, KeyCode::Char('g')), Action::None);
    let green = form(&review);
    assert_eq!(green.fields.len(), 1);
    assert_eq!(green.fields[0].input, Input::Fixed(json!(true)));
    let text = screen(&mut review);
    // A constant is filled in, not shown.
    assert!(
        text.contains("This verdict asks for nothing more") && !text.contains("approved*:"),
        "{text}"
    );
    assert!(text.contains("REVIEW (yours)"), "{text}");
    // Typing changes nothing.
    press(&mut review, KeyCode::Char('x'));
    let submit = |_verdict: crate::runtime::Verdict, result: Value| {
        Action::Start(Job::Submit {
            id: ID.parse().unwrap(),
            result: result.try_into().unwrap(),
            claim: false,
        })
    };
    assert_eq!(
        control(&mut review, 's'),
        submit(
            crate::runtime::Verdict::Green,
            json!({"verdict":"GREEN","approved":true})
        )
    );
    let error = "schema_mismatch: result must match the selected verdict's owner schema\n- instancePath \"\": \"approved\" is a required property";
    assert_eq!(
        review.finish(Outcome::Submitted {
            id: ID.parse().unwrap(),
            claimed: None,
            result: Err(error.into()),
        }),
        Action::Refresh
    );
    assert_eq!(form(&review).error.as_deref(), Some(error));
    assert_eq!(review.taken, [ID.parse::<RequestId>().unwrap()]);
    let text = screen(&mut review);
    assert!(
        text.contains("GREEN fields") && text.contains("instancePath"),
        "{text}"
    );

    press(&mut review, KeyCode::Esc);
    press(&mut review, KeyCode::Char('r'));
    for c in "Needs  work".chars() {
        press(&mut review, KeyCode::Char(c));
    }
    for _ in 0..5 {
        press(&mut review, KeyCode::Backspace);
    }
    for c in "work".chars() {
        press(&mut review, KeyCode::Char(c));
    }
    assert!(screen(&mut review).contains("reason*: Needs work▏   string, minLength 1"));
    assert_eq!(
        control(&mut review, 's'),
        submit(
            crate::runtime::Verdict::Red,
            json!({"verdict":"RED","reason":"Needs work"})
        )
    );
    let settled = view("RED", None, demo()).request.as_ref().clone();
    assert_eq!(
        review.finish(Outcome::Submitted {
            id: ID.parse().unwrap(),
            claimed: None,
            result: Ok(Box::new(settled)),
        }),
        Action::Refresh
    );
    assert_eq!(review.focus(), Focus::List);
    assert_eq!(review.mode(), &Mode::Request);
    assert!(review.taken.is_empty());
    assert!(screen(&mut review).contains("Submitted RED for release/signoff (run-1-3)."));
    // Nothing else waits, so the review returns to its caller.
    assert_eq!(review.refresh().await, Action::Quit);
}

#[test]
fn lists_of_flat_objects_are_groups_to_add_fill_and_remove_and_ctrl_e_does_nothing() {
    let groups = definition(
        json!({
            "type":"object",
            "properties":{"approved":{"const":true}},
            "required":["approved"],
        }),
        json!({
            "type":"object",
            "properties":{"findings":{"type":"array","minItems":1,"items":{
                "type":"object",
                "properties":{"line":{"type":"integer"},"problem":{"type":"string"}},
                "required":["problem"],
            }}},
            "required":["findings"],
        }),
    );
    let mut review = opened(Some("alice"), groups);
    press(&mut review, KeyCode::Char('r'));
    assert!(screen(&mut review).contains("findings 1 · line: "));
    // The constant of GREEN is filled in and has no row; RED's group rows come first.
    for c in "12".chars() {
        press(&mut review, KeyCode::Char(c));
    }
    press(&mut review, KeyCode::Down);
    review.paste("typo");
    press(&mut review, KeyCode::Down);
    assert!(review.detail_hints().contains("Enter add group"));
    press(&mut review, KeyCode::Enter);
    assert!(screen(&mut review).contains("findings 2 · line: "));
    review.paste("x");
    assert!(review.detail_hints().contains("Ctrl-D remove group"));
    assert_eq!(control(&mut review, 'e'), Action::None);
    assert_eq!(control(&mut review, 'd'), Action::None);
    assert!(!screen(&mut review).contains("findings 2 · line: "));
    assert_eq!(
        control(&mut review, 's'),
        Action::Start(Job::Submit {
            id: ID.parse().unwrap(),
            result: json!({"verdict":"RED","findings":[{"line":12,"problem":"typo"}]})
                .try_into()
                .unwrap(),
            claim: false
        })
    );
}

#[test]
fn a_recorded_schema_no_form_can_enter_is_refused_with_its_reason() {
    let nested = definition(
        json!({
            "type":"object",
            "properties":{"meta":{"type":"object","properties":{"ticket":{"type":"string"}}}},
        }),
        json!({"type":"object"}),
    );
    let mut review = opened(Some("alice"), nested);
    press(&mut review, KeyCode::Char('g'));
    assert_eq!(review.mode, Mode::Request);
    let (notice, error) = review.notice.clone().unwrap();
    assert!(error && notice.contains("properties.meta"), "{notice}");
}

#[test]
fn flat_forms_cover_booleans_choices_numbers_and_optional_fields() {
    let schema = json!({
        "type":"object",
        "additionalProperties":false,
        "required":["ok","level","count"],
        "properties":{
            "ok":{"type":"boolean","description":"Did it work?"},
            "level":{"enum":["low","high"]},
            "count":{"type":"integer","minimum":0},
            "score":{"type":"number"},
            "note":{"type":"string","maxLength":20},
            "seen":{"type":"boolean"},
        },
    });
    let mut form = Form::new(crate::runtime::Verdict::Green, Some(&schema)).unwrap();
    let names: Vec<_> = form
        .fields
        .iter()
        .map(|field| field.name.as_str())
        .collect();
    assert_eq!(names, ["count", "level", "note", "ok", "score", "seen"]);
    let ok = &form.fields[3];
    assert_eq!(ok.hint, "boolean — Did it work?");
    assert!(ok.required);
    let key = |form: &mut Form, code| form.key(form_key(KeyEvent::from(code)).0);
    for c in "3x".chars() {
        key(&mut form, KeyCode::Char(c));
    }
    assert_eq!(form.result().unwrap_err(), "count: enter an integer.");
    key(&mut form, KeyCode::Backspace);
    key(&mut form, KeyCode::Tab);
    key(&mut form, KeyCode::Left);
    assert_eq!(form.fields[1].display(), r#""high""#);
    key(&mut form, KeyCode::Down);
    key(&mut form, KeyCode::Down);
    key(&mut form, KeyCode::Char(' '));
    key(&mut form, KeyCode::Char(' '));
    assert_eq!(form.fields[3].input, Input::Bool(Some(false)));
    key(&mut form, KeyCode::Char(' '));
    assert_eq!(
        form.fields[3].input,
        Input::Bool(Some(true)),
        "required booleans stay set"
    );
    key(&mut form, KeyCode::Down);
    for c in "2.5".chars() {
        key(&mut form, KeyCode::Char(c));
    }
    key(&mut form, KeyCode::BackTab);
    key(&mut form, KeyCode::BackTab);
    key(&mut form, KeyCode::BackTab);
    assert_eq!(form.selected, 1);
    assert_eq!(
        form.result().unwrap(),
        json!({"verdict":"GREEN","count":3,"level":"high","ok":true,"score":2.5})
    );
    key(&mut form, KeyCode::Up);
    key(&mut form, KeyCode::Up);
    assert_eq!(form.selected, 5);
    key(&mut form, KeyCode::Char('y'));
    key(&mut form, KeyCode::Char(' '));
    key(&mut form, KeyCode::Char(' '));
    assert_eq!(
        form.fields[5].input,
        Input::Bool(None),
        "optional booleans can be unset"
    );

    for (schema, enterable) in [
        (
            json!({"properties":{"a":{"type":"string","pattern":"x"}}}),
            false,
        ),
        (
            json!({"properties":{"a":{"type":["string","null"]}}}),
            false,
        ),
        (json!({"properties":{"a":{"enum":[1,2]}}}), false),
        (
            json!({"properties":{"a":{"anyOf":[{"type":"string"}]}}}),
            false,
        ),
        (
            json!({"properties":{"a":{"type":"string"}},"minProperties":1}),
            false,
        ),
        (
            json!({"properties":{"a":{"const":"x","type":"string","title":"A"}}}),
            true,
        ),
        (json!({}), true),
    ] {
        assert_eq!(
            Form::new(crate::runtime::Verdict::Red, Some(&schema)).is_ok(),
            enterable,
            "{schema}"
        );
    }
    let empty = Form::new(crate::runtime::Verdict::Green, None).unwrap();
    assert_eq!(empty.result().unwrap(), json!({"verdict":"GREEN"}));
}

#[test]
fn quitting_keeps_or_releases_only_the_claims_this_session_took() {
    let mut review = opened(Some("alice"), demo());
    assert_eq!(press(&mut review, KeyCode::Char('q')), Action::Quit);
    assert_eq!(
        press(&mut review, KeyCode::Char('u')),
        Action::Start(Job::Release {
            ids: vec![ID.parse().unwrap()],
            quit: false
        })
    );
    review.finish(Outcome::Released {
        ids: vec![ID.parse().unwrap()],
        quit: false,
        failed: Vec::new(),
    });
    assert!(screen(&mut review).contains("Claim released."));

    let mut review = opened(None, demo());
    claimed(&mut review);
    // A form is set aside, not discarded, by the quit prompt.
    press(&mut review, KeyCode::Char('r'));
    press(&mut review, KeyCode::Char('x'));
    assert_eq!(control(&mut review, 'c'), Action::None);
    assert_eq!(review.mode(), &Mode::Leave);
    let text = screen(&mut review);
    assert!(text.contains("This session claimed 1 request(s) without submitting:"));
    assert!(text.contains("  run-1-3"));
    assert!(text.contains("k keep claims and quit"), "{text}");
    press(&mut review, KeyCode::Esc);
    assert_eq!(review.mode(), &Mode::Request);
    press(&mut review, KeyCode::Char('r'));
    assert_eq!(form(&review).fields[0].display(), "x");
    press(&mut review, KeyCode::Esc);
    press(&mut review, KeyCode::Char('q'));
    assert_eq!(review.mode(), &Mode::Leave);
    assert_eq!(
        control(&mut review, 'c'),
        Action::Quit,
        "Ctrl-C twice keeps the claim"
    );
    press(&mut review, KeyCode::Esc);
    press(&mut review, KeyCode::Char('q'));
    assert_eq!(press(&mut review, KeyCode::Char('k')), Action::Quit);
    let release = Action::Start(Job::Release {
        ids: vec![ID.parse().unwrap()],
        quit: true,
    });
    assert_eq!(press(&mut review, KeyCode::Char('u')), release);
    assert_eq!(
        review.finish(Outcome::Released {
            ids: vec![ID.parse().unwrap()],
            quit: true,
            failed: vec![(ID.parse().unwrap(), "database is locked".into())],
        }),
        Action::Refresh
    );
    assert_eq!(review.taken, [ID.parse::<RequestId>().unwrap()]);
    assert!(screen(&mut review).contains("run-1-3: database is locked"));
    assert_eq!(press(&mut review, KeyCode::Char('u')), release);
    assert_eq!(
        review.finish(Outcome::Released {
            ids: vec![ID.parse().unwrap()],
            quit: true,
            failed: Vec::new(),
        }),
        Action::Quit
    );
    assert!(review.taken.is_empty());
}

#[test]
fn a_running_job_only_scrolls_or_cancels() {
    let mut review = opened(Some("alice"), demo());
    let output = (0..30)
        .map(|index| format!("output-{index:03}\n"))
        .collect::<String>();
    logged(&mut review, &output, "", 0);
    drop(review.start(Job::Run {
        id: ID.parse().unwrap(),
        tool: "notes_release".parse().unwrap(),
        claim: false,
    }));
    assert!(review.busy());
    let text = screen(&mut review);
    assert!(
        text.contains("Running notes_release…") && text.contains("Esc or Ctrl-C cancel"),
        "{text}"
    );
    for key in [
        KeyCode::Char('g'),
        KeyCode::Char('q'),
        KeyCode::Enter,
        KeyCode::Left,
    ] {
        assert_eq!(press(&mut review, key), Action::None);
        assert_eq!(review.mode(), &Mode::Request);
        assert_eq!(review.focus(), Focus::Detail);
    }
    // The log follows its newest line; scrolled up it stays put while lines arrive, and
    // back at the bottom it follows again.
    assert!(
        text.contains("■ exit 0") && !text.contains("output-000"),
        "{text}"
    );
    for _ in 0..3 {
        press(&mut review, KeyCode::PageUp);
    }
    let text = screen(&mut review);
    assert!(
        text.contains("output-000") && !text.contains("■ exit 0"),
        "{text}"
    );
    logged(&mut review, "newest\n", "", 0);
    assert!(!screen(&mut review).contains("newest"));
    for _ in 0..10 {
        press(&mut review, KeyCode::PageDown);
    }
    logged(&mut review, "latest\n", "", 0);
    assert!(screen(&mut review).contains("latest"));
    let cancel = review.busy.as_ref().unwrap().cancel.clone();
    press(&mut review, KeyCode::Esc);
    assert!(cancel.is_cancelled());
    assert_eq!(
        review.focus(),
        Focus::Detail,
        "Esc cancels before stepping back"
    );
    review.finish(Outcome::Ran {
        tool: "notes_release".parse().unwrap(),
        claimed: None,
        result: printed("Human tool call was cancelled.", true),
    });
    assert!(!review.busy());
}

fn listed() -> Review {
    let mut review = Review::new("fixture-state".into(), None, "alice".parse().unwrap(), None);
    let mut other = view("WAITING_HUMAN", Some("bob"), demo());
    std::sync::Arc::make_mut(&mut other.request).id = "run-2-1".parse().unwrap();
    std::sync::Arc::make_mut(&mut other.request).eval_id = "docs/check".parse().unwrap();
    review.set_listed(vec![other, view("WAITING_HUMAN", Some("alice"), demo())]);
    review.refreshed = Some(
        OffsetDateTime::parse(
            "2026-01-01T00:01:00Z",
            &time::format_description::well_known::Rfc3339,
        )
        .unwrap(),
    );
    review
}

#[test]
fn list_opens_requests_and_esc_steps_back_without_quitting() {
    let mut review = listed();
    let text = screen(&mut review);
    assert!(text.contains("Waiting Human reviews (2)") && text.contains("waiting Human reviews"));
    assert!(
        text.contains("alice (you)") && text.contains("bob"),
        "{text}"
    );
    assert!(text.contains("repo"));
    // The selected request is previewed beside the list.
    assert!(text.contains("docs/check · Approve"), "{text}");
    assert!(text.contains("Enter: open review · Tab: tools"), "{text}");
    press(&mut review, KeyCode::Char('j'));
    assert!(screen(&mut review).contains("release/signoff · Approve"));
    assert_eq!(press(&mut review, KeyCode::Enter), Action::Refresh);
    assert_eq!(review.open.as_deref(), Some(ID));
    assert_eq!(review.focus(), Focus::Detail);
    let text = screen(&mut review);
    assert!(
        text.contains("artifactize review › waiting Human reviews › release/signoff"),
        "{text}"
    );
    assert!(text.contains("REVIEW (yours)"), "{text}");
    assert_eq!(press(&mut review, KeyCode::Esc), Action::Refresh);
    assert_eq!(review.focus(), Focus::List);
    assert_eq!(press(&mut review, KeyCode::Char('r')), Action::Refresh);
    assert_eq!(press(&mut review, KeyCode::Esc), Action::None);
    assert_eq!(press(&mut review, KeyCode::Left), Action::None);
    assert_eq!(press(&mut review, KeyCode::Char('q')), Action::Quit);
    // Opening another request starts it fresh.
    let mut review = listed();
    press(&mut review, KeyCode::Enter);
    let open = review.open.clone().unwrap();
    drop(review.sink.start(&open, "notes", "release"));
    review.drain_log();
    press(&mut review, KeyCode::Esc);
    press(&mut review, KeyCode::Enter);
    assert!(
        !review.log.is_empty(),
        "the same request keeps its Tool log"
    );
    press(&mut review, KeyCode::Esc);
    press(&mut review, KeyCode::Char('j'));
    press(&mut review, KeyCode::Right);
    assert_eq!(review.open.as_deref(), Some(ID));
    assert!(review.log.is_empty());
}

#[test]
fn a_tool_run_never_claims_once_the_claim_is_gone() {
    // Another terminal releases the claim; a refresh reloads it and Enter starts nothing.
    let mut review = opened(Some("alice"), demo());
    press(&mut review, KeyCode::Tab);
    review.show(view("WAITING_HUMAN", None, demo()));
    assert_eq!(press(&mut review, KeyCode::Enter), Action::None);
    assert!(screen(&mut review).contains("Claim this request before reviewing it."));
    // Claimed by someone else meanwhile: a click on the selected tool runs nothing either,
    // and an open form stays in the drafts for the next claim.
    let mut review = opened(Some("alice"), demo());
    press(&mut review, KeyCode::Char('r'));
    press(&mut review, KeyCode::Char('x'));
    press(&mut review, KeyCode::BackTab);
    review.show(view("WAITING_HUMAN", Some("bob"), demo()));
    assert_eq!(review.control(Control::RunTool), Action::None);
    assert_eq!(form(&review).fields[0].display(), "x");
    // A request settled meanwhile runs nothing.
    let mut review = opened(Some("alice"), demo());
    press(&mut review, KeyCode::Tab);
    review.show(view("GREEN", None, demo()));
    assert_eq!(press(&mut review, KeyCode::Enter), Action::None);
    // No job ever claims: an unclaimed submission is refused too.
    let mut review = opened(Some("alice"), demo());
    press(&mut review, KeyCode::Char('r'));
    press(&mut review, KeyCode::Char('x'));
    review.show(view("WAITING_HUMAN", None, demo()));
    assert_eq!(control(&mut review, 's'), Action::None);
    assert_eq!(
        review.notice.as_ref().map(|(notice, _)| notice.as_str()),
        Some("Claim this request before reviewing it.")
    );
}

#[test]
fn a_bracketed_paste_reaches_only_the_focused_field() {
    let mut review = opened(Some("alice"), demo());
    // Without a form, on the tools or from the list, a paste changes nothing.
    review.paste("ignored");
    press(&mut review, KeyCode::Char('r'));
    press(&mut review, KeyCode::BackTab);
    review.paste("ignored");
    press(&mut review, KeyCode::Tab);
    review.paste("needs 한글\nwork");
    assert_eq!(form(&review).fields[0].display(), "needs 한글\nwork");
    // One paste is one edit, never keys: q and Esc inside it neither quit nor step back.
    review.paste(" q\u{1b}");
    assert!(review.editing());
    assert_eq!(review.focus(), Focus::Detail);
    press(&mut review, KeyCode::Esc);
    press(&mut review, KeyCode::Esc);
    assert_eq!(review.focus(), Focus::List);
    review.paste("ignored");
    review.set_listed(vec![view("WAITING_HUMAN", Some("alice"), demo())]);
    assert_eq!(press(&mut review, KeyCode::Enter), Action::Refresh);
    press(&mut review, KeyCode::Char('r'));
    assert_eq!(
        form(&review).fields[0].display(),
        "needs 한글\nwork q\u{1b}"
    );
}

#[test]
fn a_refresh_never_drops_the_quit_prompt() {
    let mut review = opened(Some("alice"), demo());
    let other: RequestId = "run-2-1".parse().unwrap();
    review.taken = vec![ID.parse().unwrap(), other];
    review.show(view("GREEN", None, demo()));
    assert_eq!(control(&mut review, 'c'), Action::None);
    assert_eq!(review.mode(), &Mode::Leave);
    // The settled request reloads, as every refresh does; the prompt stays.
    review.show(view("GREEN", None, demo()));
    assert_eq!(review.mode(), &Mode::Leave);
    let text = screen(&mut review);
    assert!(
        text.contains("This session claimed 2 request(s) without submitting:")
            && text.contains("run-2-1"),
        "{text}"
    );
    assert_eq!(
        press(&mut review, KeyCode::Char('u')),
        Action::Start(Job::Release {
            ids: vec![ID.parse().unwrap(), "run-2-1".parse().unwrap()],
            quit: true
        })
    );
}

#[test]
fn control_characters_never_break_one_row_texts() {
    let mut review = listed();
    for view in &mut review.listed {
        assert!(
            format!("{}\n\tx", view.request.eval_id)
                .parse::<crate::types::EvalId>()
                .is_err()
        );
        std::sync::Arc::make_mut(&mut view.request).title = "Ap\nprove\u{1b}[31m".into();
    }
    let rows = |text: &str| text.lines().map(str::to_owned).collect::<Vec<_>>();
    // The list cells and the Preview title stay on their rows.
    let text = screen(&mut review);
    assert!(text.contains("docs/check"), "{text}");
    assert!(text.contains("docs/check · Ap prove[31m"), "{text}");
    assert!(!text.contains('\u{1b}'));
    for row in rows(&text) {
        assert_eq!(row.chars().count(), 140, "{text}");
    }
    // So do the Detail's title and the Compact list.
    press(&mut review, KeyCode::Enter);
    let text = screen(&mut review);
    assert!(text.contains("docs/check · Ap prove[31m · CLAIM"), "{text}");
    assert!(text.contains("? docs/check"), "{text}");
    for row in rows(&text) {
        assert_eq!(row.chars().count(), 140, "{text}");
    }
}

#[test]
fn review_uses_the_reactive_layout_of_monitor() {
    let mut review = listed();
    press(&mut review, KeyCode::Char('j'));
    for (width, side, full) in [
        (80, 16, 64),
        (99, 16, 83),
        (100, 28, 72),
        (120, 28, 92),
        (139, 28, 111),
        (140, 36, 104),
        (160, 36, 124),
    ] {
        // List focused: Full fills the body beside a fixed-width Detail Preview.
        let text = sized(&mut review, width, 30);
        assert_eq!((review.hits.list.x, review.hits.list.width), (0, full));
        assert_eq!(
            (review.hits.preview.x, review.hits.preview.width),
            (full, side)
        );
        assert_eq!(review.hits.preview.right(), width);
        assert!(
            text.contains("EVAL") && text.contains("REQUEST"),
            "{width}\n{text}"
        );
        assert!(text.contains("Enter: open"), "{width}\n{text}");
        assert!(!text.contains("· REVIEW (yours)"), "{width}\n{text}");
    }
    press(&mut review, KeyCode::Enter);
    for (width, side, full) in [
        (80, 16, 64),
        (99, 16, 83),
        (100, 28, 72),
        (120, 28, 92),
        (139, 28, 111),
        (140, 36, 104),
        (160, 36, 124),
    ] {
        // Detail focused: its immediate list neighbour stays Compact at every width.
        let text = sized(&mut review, width, 30);
        assert_eq!((review.hits.list.x, review.hits.list.width), (0, side));
        assert_eq!(
            (review.hits.detail.x, review.hits.detail.width),
            (side, full)
        );
        assert_eq!(review.hits.detail.right(), width);
        assert!(text.contains("· REVIEW (yours)"), "{width}\n{text}");
        assert!(!text.contains("Enter: open"), "{width}\n{text}");
        assert!(
            !text.contains("EVAL "),
            "the Compact list has no columns\n{text}"
        );
        let first = text.lines().nth(1).unwrap();
        assert!(
            first.starts_with('┌') && first.chars().nth(usize::from(side)) == Some('┌'),
            "{width}\n{text}"
        );
        assert!(
            text.contains("artifactize review › waiting Human reviews › release/signoff"),
            "{width}\n{text}"
        );
    }
}

fn click(review: &mut Review, rect: ratatui::layout::Rect) -> Action {
    review.mouse(crossterm::event::MouseEvent {
        kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
        column: rect.x + 1,
        row: rect.y,
        modifiers: KeyModifiers::NONE,
    })
}

#[test]
fn every_focus_names_the_way_to_the_tools_and_how_to_run_one() {
    // The list: the footer and the Preview name Enter and Tab; Tab opens the tools directly.
    let mut review = listed();
    let text = screen(&mut review);
    assert!(
        text.contains("↑↓ request · Enter open · Tab tools"),
        "{text}"
    );
    assert!(text.contains("Enter: open review · Tab: tools"), "{text}");
    assert_eq!(press(&mut review, KeyCode::Tab), Action::Refresh);
    assert_eq!(
        (review.focus(), review.area()),
        (Focus::Detail, Area::Tools)
    );

    // CLAIM: the tools line and the footer say how to reach and run them.
    let mut review = opened(None, demo());
    let text = screen(&mut review);
    assert!(text.contains("Tab or click: tools"), "{text}");
    assert!(
        text.contains("c claim · Tab tools, Enter run · i instruction"),
        "{text}"
    );
    press(&mut review, KeyCode::Tab);
    let text = screen(&mut review);
    assert!(text.contains("Tools (2) · ↑↓ Enter run"), "{text}");
    assert!(
        text.contains("↑↓ tool · c claim, then Enter run · Tab fields"),
        "{text}"
    );

    // REVIEW: the pane title names its key; while a field takes Tab, it is Shift-Tab.
    let mut review = opened(Some("alice"), demo());
    let text = screen(&mut review);
    assert!(text.contains("Tools (2) · Tab "), "{text}");
    assert!(
        text.contains("g GREEN · r RED · u release · Tab tools, Enter run"),
        "{text}"
    );
    press(&mut review, KeyCode::Char('r'));
    let text = screen(&mut review);
    assert!(text.contains("Tools (2) · Shift-Tab"), "{text}");
    assert!(text.contains("Shift-Tab tools, Enter run"), "{text}");
    press(&mut review, KeyCode::BackTab);
    assert!(screen(&mut review).contains("↑↓ tool · Enter run · Tab fields"));

    // Claimed by someone else: read-only, so no claim or run is offered.
    let review = opened(Some("bob"), demo());
    assert_eq!(
        review.detail_hints(),
        "Tab tools · i instruction · t technical · Esc back · q quit"
    );

    // Monitor's Human Detail shows the same key line for the same state.
    let monitor = embedded(Some("alice"), demo());
    assert_eq!(
        monitor.detail_hints(),
        "g GREEN · r RED · u release · Tab tools, Enter run · i instruction · t technical · \
         Esc back · q quit"
    );
}

#[test]
fn clicks_focus_select_and_run_tools_and_f2_turns_the_mouse_off() {
    let mut review = listed();
    screen(&mut review);
    // A click selects a list row, a click on the selected row opens it.
    let (row, index) = review.hits.rows[1];
    assert_eq!(index, 1);
    assert_eq!(click(&mut review, row), Action::None);
    assert_eq!(review.list.selected(), Some(1));
    assert_eq!(click(&mut review, row), Action::Refresh);
    assert_eq!(review.focus(), Focus::Detail);
    assert_eq!(review.open.as_deref(), Some(ID));
    // Claimed by alice: a click on a tool focuses and selects it; a second click runs it.
    screen(&mut review);
    let (row, index) = *review.hits.review.tool_rows.last().unwrap();
    assert_eq!(index, 1);
    assert_eq!(click(&mut review, row), Action::None);
    assert_eq!((review.area(), review.tool), (Area::Tools, 1));
    screen(&mut review);
    let (row, _) = *review
        .hits
        .review
        .tool_rows
        .iter()
        .find(|(_, index)| *index == 1)
        .unwrap();
    assert_eq!(
        click(&mut review, row),
        Action::Start(Job::Run {
            id: ID.parse().unwrap(),
            tool: "open_release".parse().unwrap(),
            claim: false,
        })
    );
    // The visible Run tool button runs the selected tool too.
    let (button, _) = *review
        .hits
        .review
        .buttons
        .iter()
        .find(|(_, control)| *control == Control::RunTool)
        .unwrap();
    assert!(matches!(
        click(&mut review, button),
        Action::Start(Job::Run { .. })
    ));
    // Close steps back to the list.
    let close = review.hits.review.close;
    assert_eq!(click(&mut review, close), Action::Refresh);
    assert_eq!(review.focus(), Focus::List);

    // Unclaimed: a click on the selected tool asks for the claim and starts nothing.
    let mut review = opened(None, demo());
    press(&mut review, KeyCode::Tab);
    screen(&mut review);
    let (row, _) = review.hits.review.tool_rows[0];
    assert_eq!(click(&mut review, row), Action::None);
    assert!(screen(&mut review).contains("Claim this request before reviewing it."));

    // F2 turns capture off: the header says so and clicks do nothing until F2 again.
    let mut review = listed();
    assert_eq!(press(&mut review, KeyCode::F(2)), Action::Capture(false));
    let text = screen(&mut review);
    assert!(text.contains("mouse off · F2"), "{text}");
    let (row, _) = review.hits.rows[1];
    assert_eq!(click(&mut review, row), Action::None);
    assert_eq!(review.list.selected(), Some(0));
    assert_eq!(press(&mut review, KeyCode::F(2)), Action::Capture(true));
    assert!(!screen(&mut review).contains("mouse off"));
}

/// The focused Tools pane rows of each `builtins()` tool, selected in name order: the tool and
/// its kind, what Enter does on which logical target, and the description.
pub(crate) fn builtin_rows(width: u16) -> Vec<Vec<&'static str>> {
    let narrow = width < 120;
    vec![
        vec![
            "assets_release  output",
            "  lists docs/guide/",
            "  List the guide.",
        ],
        vec![
            "cargo_release  output",
            "  prints cargo build --help",
            "  Show the build options.",
        ],
        vec![
            "changes_release  output",
            "  prints release/CHANGELOG.md",
            "  Read the changelog.",
        ],
        if narrow {
            vec![
                "install_release  output",
                r#"  prints section "Install" of"#,
                "    docs/guide/README.md",
                "  Read the install steps.",
            ]
        } else {
            vec![
                "install_release  output",
                r#"  prints section "Install" of docs/guide/README.md"#,
                "  Read the install steps.",
            ]
        },
        // A command tool keeps its declared command line.
        vec![
            "notes_release  output",
            "  $ get {artifactPath}/notes.md",
            "  Print the notes of release.",
        ],
        if narrow {
            vec![
                "open_release  launch",
                "  opens release/notes.md in its",
                "    default app",
                "  Open the notes of release.",
            ]
        } else {
            vec![
                "open_release  launch",
                "  opens release/notes.md in its default app",
                "  Open the notes of release.",
            ]
        },
        if narrow {
            vec![
                "site_release  launch",
                "  opens https://example.com/release",
                "    in the browser",
                "  Open the release page.",
            ]
        } else {
            vec![
                "site_release  launch",
                "  opens https://example.com/release in the browser",
                "  Open the release page.",
            ]
        },
    ]
}

/// Whether one pane row of `text` reads exactly `row`, up to its padding.
pub(crate) fn drawn_row(text: &str, row: &str) -> bool {
    text.lines()
        .any(|line| line.split('│').any(|cell| cell.trim_end() == row))
}

#[test]
fn builtin_tools_name_their_action_on_a_logical_target_at_80_and_120_columns() {
    for width in [80, 120] {
        let mut review = opened(None, builtins());
        press(&mut review, KeyCode::Tab);
        for rows in builtin_rows(width) {
            let text = sized(&mut review, width, 30);
            for row in rows {
                assert!(drawn_row(&text, row), "{width}: {row}\n{text}");
            }
            // Builtins never read as shell command lines or unresolved placeholders.
            for declared in [
                "$ open",
                "$ read",
                "$ list",
                "$ section",
                "$ help",
                "{guide}",
            ] {
                assert!(!text.contains(declared), "{declared}\n{text}");
            }
            press(&mut review, KeyCode::Down);
        }
        // The command tool reads as it did beside command tools only.
        let mut demo = opened(None, demo());
        press(&mut demo, KeyCode::Tab);
        let text = sized(&mut demo, width, 30);
        for row in &builtin_rows(width)[4] {
            assert!(drawn_row(&text, row), "{width}: {row}\n{text}");
        }
    }
}
