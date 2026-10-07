use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{Terminal, backend::TestBackend};
use serde_json::{Value, json};

use super::*;

const ID: &str = "run-1-3";

#[test]
fn spinner_frames_keep_the_existing_hundred_millisecond_cadence_and_cast_order() {
    assert_eq!(view::spinner(Duration::from_millis(0)), "⠋");
    assert_eq!(view::spinner(Duration::from_millis(99)), "⠋");
    assert_eq!(view::spinner(Duration::from_millis(100)), "⠙");
    assert_eq!(view::spinner(Duration::from_millis(999)), "⠏");
    assert_eq!(view::spinner(Duration::from_millis(1000)), "⠋");
    let previous_frames = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
    let large = Duration::MAX;
    assert_eq!(
        view::spinner(large),
        previous_frames[(large.as_millis() / 100) as usize % previous_frames.len()]
    );
}

fn definition(pass: Value, fail: Value) -> Value {
    json!({
        "repo":"/repo",
        "eval":{"id":"release/signoff","target":"release","references":{},"deps":[],
            "declaration":{"id":"signoff","title":"Approve","profile":{"kind":"human"},
                "payload":{"instruction":"Approve the notes."},"passSchema":pass,"failSchema":fail}},
        "artifacts":{"release":{"path":"release","views":{"agentTools":{},"humanTools":{
            "notes":{"description":"Print the notes of {artifactName}.","kind":"output","command":"cat","args":["{artifactPath}/notes.md"]},
            "open":{"description":"Open {artifactName}.","kind":"launch","command":"xdg-open","args":["{artifactPath}"]}
        }}}}
    })
}

/// The review-demo schemas: GREEN needs `approved: const true`, RED a non-empty reason.
pub(crate) fn demo() -> Value {
    definition(
        json!({"type":"object","properties":{"approved":{"const":true}},"required":["approved"],"additionalProperties":false}),
        json!({"type":"object","properties":{"reason":{"type":"string","minLength":1}},"required":["reason"],"additionalProperties":false}),
    )
}

pub(crate) fn claim(reviewer: &str) -> HumanClaim {
    HumanClaim {
        request_id: ID.parse().unwrap(),
        reviewer: reviewer.into(),
        claimed_at: "2026-01-01T00:00:30Z".into(),
    }
}

fn view(status: &str, reviewer: Option<&str>, definition: Value) -> RequestView {
    RequestView {
        request: serde_json::from_value(json!({
            "id":ID,"runId":"run-1","evalId":"release/signoff","target":"release","title":"Approve",
            "profile":{"kind":"human"},"requestedProfile":{"kind":"human"},"evalDefHash":"hash",
            "payload":{"instruction":"Approve the notes."},"references":{},"deps":[],"status":status,
            "createdAt":"2026-01-01T00:00:00Z","cwd":"/repo","humanDefinition":definition
        }))
        .unwrap(),
        claim: reviewer.map(claim),
        execution: None,
        definition: None,
    }
}

pub(crate) fn opened(reviewer: Option<&str>, definition: Value) -> Review {
    let mut review = Review::new(
        "/state".into(),
        Some("/repo".into()),
        "alice".into(),
        Some(ID.into()),
    );
    review.request = Some(view("WAITING_HUMAN", reviewer, definition));
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
        repo: "/repo".into(),
        kind: HumanToolKind::Output,
        program: "cat".into(),
        args: args.iter().map(|arg| (*arg).to_owned()).collect(),
        cwd: "/repo/release".into(),
    }
}

fn printed(text: &str, is_error: bool) -> Result<ToolResult, String> {
    Ok(ToolResult {
        content: vec![Content::Text { text: text.into() }],
        is_error,
    })
}

fn screen(review: &mut Review) -> String {
    let mut terminal = Terminal::new(TestBackend::new(140, 40)).unwrap();
    terminal.draw(|frame| review.draw(frame)).unwrap();
    let buffer = terminal.backend().buffer();
    let rows = buffer.content().chunks(buffer.area.width as usize);
    let rows = rows.map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>());
    rows.collect::<Vec<_>>().join("\n")
}

fn form(review: &Review) -> &Form {
    match review.mode() {
        Mode::Form(form) => form,
        mode => panic!("not a form: {mode:?}"),
    }
}

#[test]
fn tools_and_details_come_from_the_saved_definition_without_claiming() {
    let mut review = opened(None, demo());
    let tools = review.tools();
    assert_eq!(
        tools
            .iter()
            .map(|tool| (tool.name.as_str(), tool.kind, tool.declared.as_str()))
            .collect::<Vec<_>>(),
        [
            (
                "notes_release",
                HumanToolKind::Output,
                "cat {artifactPath}/notes.md"
            ),
            (
                "open_release",
                HumanToolKind::Launch,
                "xdg-open {artifactPath}"
            ),
        ]
    );
    assert_eq!(tools[0].description, "Print the notes of release.");
    assert_eq!(
        shell(["sh", "-c", "echo 'hi' $HOME", ""]),
        r#"sh -c 'echo '\''hi'\'' $HOME' ''"#
    );
    let text = screen(&mut review);
    for expected in [
        "release/signoff · Approve",
        "Request: run-1-3",
        "Run: run-1",
        "Repository: /repo",
        "Status: WAITING_HUMAN",
        "Claim: unclaimed; running a tool or submitting claims it for alice",
        "Instruction: Approve the notes.",
        r#"GREEN fields: {"additionalProperties":false,"properties":{"approved":{"const":true}}"#,
        r#"RED fields: {"additionalProperties":false,"properties":{"reason":{"minLength":1"#,
        "notes_release (output)",
        "$ cat {artifactPath}/notes.md",
        "open_release (launch)",
    ] {
        assert!(text.contains(expected), "{expected}\n{text}");
    }
}

#[test]
fn first_run_of_each_command_line_is_confirmed_then_claims_once() {
    let mut review = opened(None, demo());
    let inspect = Action::Start(Job::Inspect {
        id: ID.into(),
        tool: "notes_release".into(),
    });
    assert_eq!(press(&mut review, KeyCode::Enter), inspect);
    let notes = command(&["/repo/release/notes.md"]);
    let inspected = |command: &CommandLine| Outcome::Inspected {
        tool: "notes_release".into(),
        result: Ok(command.clone()),
    };
    assert_eq!(review.finish(inspected(&notes)), Action::None);
    let text = screen(&mut review);
    for expected in [
        "Run notes_release for the first time in this session?",
        "Kind: output",
        "Repository: /repo",
        "Directory: /repo/release",
        "Command: cat /repo/release/notes.md",
    ] {
        assert!(text.contains(expected), "{expected}\n{text}");
    }
    assert_eq!(press(&mut review, KeyCode::Char('n')), Action::None);
    assert_eq!(review.mode(), &Mode::Request);
    assert_eq!(press(&mut review, KeyCode::Enter), inspect);
    review.finish(inspected(&notes));
    assert!(matches!(review.mode(), Mode::Confirm { .. }));
    assert_eq!(
        press(&mut review, KeyCode::Char('y')),
        Action::Start(Job::Run {
            id: ID.into(),
            tool: "notes_release".into(),
            claim: true
        })
    );
    let ran = review.finish(Outcome::Ran {
        tool: "notes_release".into(),
        claimed: Some(claim("alice")),
        result: printed("# Release notes\n", false),
    });
    assert_eq!(ran, Action::Refresh);
    assert_eq!(review.taken, [ID]);
    review.request = Some(view("WAITING_HUMAN", Some("alice"), demo()));
    let text = screen(&mut review);
    assert!(text.contains("Output · notes_release") && text.contains("# Release notes"));
    assert!(text.contains("Claim: claimed by you (alice)"), "{text}");

    // Confirmed and claimed: the next run neither asks nor claims again.
    assert_eq!(press(&mut review, KeyCode::Enter), inspect);
    assert_eq!(
        review.finish(inspected(&notes)),
        Action::Start(Job::Run {
            id: ID.into(),
            tool: "notes_release".into(),
            claim: false
        })
    );
    // A different command line (for example from another repository) asks again.
    review.finish(inspected(&command(&["/other/notes.md"])));
    assert!(matches!(review.mode(), Mode::Confirm { .. }));
    press(&mut review, KeyCode::Esc);

    review.finish(Outcome::Ran {
        tool: "notes_release".into(),
        claimed: None,
        result: printed("Human tool exited unsuccessfully (exit status: 1).\n", true),
    });
    let text = screen(&mut review);
    assert!(
        text.contains("Output · notes_release · tool error"),
        "{text}"
    );
    assert!(text.contains("exited unsuccessfully"), "{text}");
    assert!(
        text.contains("notes_release failed; a tool error is not a verdict."),
        "{text}"
    );
    review.finish(Outcome::Ran {
        tool: "open_release".into(),
        claimed: None,
        result: Ok(ToolResult {
            content: vec![Content::Launch { launched: true }],
            is_error: false,
        }),
    });
    assert!(screen(&mut review).contains("open_release launched."));
    review.finish(Outcome::Inspected {
        tool: "notes_release".into(),
        result: Err("Human Artifact scope or eval declarations changed.".into()),
    });
    assert_eq!(review.mode(), &Mode::Request);
    assert!(screen(&mut review).contains("notes_release: Human Artifact scope"));
}

#[test]
fn requests_claimed_by_others_or_settled_are_read_only() {
    let mut review = opened(Some("bob"), demo());
    assert!(
        screen(&mut review).contains("Claim: claimed by bob at 2026-01-01T00:00:30Z; read-only")
    );
    for key in [KeyCode::Enter, KeyCode::Char('s'), KeyCode::Char('u')] {
        assert_eq!(press(&mut review, key), Action::None);
        assert_eq!(review.mode(), &Mode::Request);
        assert!(screen(&mut review).contains("Claimed by bob; read-only for alice."));
    }
    let mut review = opened(None, demo());
    review.request = Some(view("GREEN", None, demo()));
    assert_eq!(press(&mut review, KeyCode::Char('s')), Action::None);
    assert!(screen(&mut review).contains("The request is GREEN; there is nothing to review."));
    let mut review = opened(None, demo());
    assert_eq!(press(&mut review, KeyCode::Char('u')), Action::None);
    assert!(screen(&mut review).contains("The request is not claimed."));
}

#[tokio::test]
async fn demo_schemas_fill_forms_and_submission_errors_return_to_them() {
    let state = tempfile::tempdir().unwrap();
    let mut review = Review::new(
        state.path().join("state"),
        None,
        "alice".into(),
        Some(ID.into()),
    );
    review.request = Some(view("WAITING_HUMAN", None, demo()));
    press(&mut review, KeyCode::Char('s'));
    assert!(screen(&mut review).contains("Submit which verdict for release/signoff?"));
    assert_eq!(press(&mut review, KeyCode::Char('g')), Action::None);
    let green = form(&review);
    assert_eq!(green.fields.len(), 1);
    assert_eq!(green.fields[0].input, Input::Fixed(json!(true)));
    assert!(screen(&mut review).contains("approved*: true (fixed)"));
    // A fixed field ignores typing.
    press(&mut review, KeyCode::Char('x'));
    let submit = |result: Value, claim| {
        Action::Start(Job::Submit {
            id: ID.into(),
            result,
            claim,
        })
    };
    assert_eq!(
        press(&mut review, KeyCode::Enter),
        submit(json!({"verdict":"GREEN","approved":true}), true)
    );
    let error = "schema_mismatch: result must match the selected verdict's owner schema\n- instancePath \"\": \"approved\" is a required property";
    assert_eq!(
        review.finish(Outcome::Submitted {
            id: ID.into(),
            claimed: Some(claim("alice")),
            result: Err(error.into()),
        }),
        Action::Refresh
    );
    assert_eq!(form(&review).error.as_deref(), Some(error));
    assert_eq!(review.taken, [ID]);
    let text = screen(&mut review);
    assert!(
        text.contains("GREEN fields") && text.contains("instancePath"),
        "{text}"
    );

    press(&mut review, KeyCode::Esc);
    press(&mut review, KeyCode::Char('s'));
    press(&mut review, KeyCode::Char('r'));
    for c in "Needs  work".chars() {
        press(&mut review, KeyCode::Char(c));
    }
    press(&mut review, KeyCode::Backspace);
    press(&mut review, KeyCode::Backspace);
    press(&mut review, KeyCode::Backspace);
    press(&mut review, KeyCode::Backspace);
    press(&mut review, KeyCode::Backspace);
    for c in "work".chars() {
        press(&mut review, KeyCode::Char(c));
    }
    assert!(screen(&mut review).contains("reason*: Needs work▏   string, minLength 1"));
    assert_eq!(
        press(&mut review, KeyCode::Enter),
        submit(json!({"verdict":"RED","reason":"Needs work"}), true)
    );
    let settled = view("RED", None, demo()).request;
    assert_eq!(
        review.finish(Outcome::Submitted {
            id: ID.into(),
            claimed: None,
            result: Ok(Box::new(settled)),
        }),
        Action::Refresh
    );
    assert_eq!(review.mode(), &Mode::List);
    assert!(review.taken.is_empty());
    assert!(screen(&mut review).contains("Submitted RED for release/signoff (run-1-3)."));
    // Nothing else waits, so the review returns to its caller.
    assert_eq!(review.refresh().await, Action::Quit);
}

#[test]
fn other_schemas_and_the_form_request_open_the_editor() {
    let nested = definition(
        json!({"type":"object","properties":{"approved":{"const":true},"checks":{"type":"array","items":{"type":"string"}},
            "meta":{"type":"object","properties":{"ticket":{"type":"string"},"size":{"type":"integer"}}}},
            "required":["approved"],"additionalProperties":false}),
        json!({"type":"object","properties":{"reason":{"$ref":"#/$defs/text"}},"$defs":{"text":{"type":"string"}}}),
    );
    let mut review = opened(None, nested);
    press(&mut review, KeyCode::Char('s'));
    let template = json!({"approved":true,"checks":[],"meta":{"size":0,"ticket":""}});
    let Action::Edit(text) = press(&mut review, KeyCode::Char('g')) else {
        panic!("a nested schema opens the editor");
    };
    assert_eq!(serde_json::from_str::<Value>(&text).unwrap(), template);
    for (edited, error) in [
        (
            Err("The editor exited unsuccessfully".into()),
            "editor exited",
        ),
        (Ok(" \n".into()), "was empty"),
        (Ok("[]".into()), "must be a JSON object"),
        (Ok("{".into()), "not valid JSON"),
        (
            Ok(r#"{"verdict":"RED"}"#.into()),
            "not inside the owner fields",
        ),
    ] {
        assert_eq!(review.edited(edited), Action::None);
        assert!(form(&review).error.as_ref().unwrap().contains(error));
    }
    assert!(screen(&mut review).contains("Owner fields as JSON"));
    assert_eq!(
        review.edited(Ok(r#"{"approved":true,"checks":["a"]}"#.into())),
        Action::Start(Job::Submit {
            id: ID.into(),
            result: json!({"approved":true,"checks":["a"],"verdict":"GREEN"}),
            claim: true
        })
    );
    // From the form: `e` reopens the edited JSON; RED's $ref is not a flat form either.
    assert_eq!(
        press(&mut review, KeyCode::Char('e')),
        Action::Edit(r#"{"approved":true,"checks":["a"]}"#.into())
    );
    press(&mut review, KeyCode::Esc);
    press(&mut review, KeyCode::Char('s'));
    assert!(matches!(
        press(&mut review, KeyCode::Char('r')),
        Action::Edit(_)
    ));

    // Ctrl-E turns a flat form into JSON with the values entered so far.
    let mut review = opened(None, demo());
    press(&mut review, KeyCode::Char('s'));
    press(&mut review, KeyCode::Char('r'));
    press(&mut review, KeyCode::Char('x'));
    let Action::Edit(draft) = control(&mut review, 'e') else {
        panic!("Ctrl-E opens the editor");
    };
    assert_eq!(
        serde_json::from_str::<Value>(&draft).unwrap(),
        json!({"reason":"x"})
    );
    assert_eq!(form(&review).json.as_deref(), Some(draft.as_str()));
}

#[test]
fn flat_forms_cover_booleans_choices_numbers_and_optional_fields() {
    let schema = json!({"type":"object","additionalProperties":false,"required":["ok","level","count"],"properties":{
        "ok":{"type":"boolean","description":"Did it work?"},
        "level":{"enum":["low","high"]},
        "count":{"type":"integer","minimum":0},
        "score":{"type":"number"},
        "note":{"type":"string","maxLength":20},
        "seen":{"type":"boolean"}
    }});
    let mut form = Form::new("GREEN", Some(&schema));
    assert!(form.json.is_none());
    let names: Vec<_> = form
        .fields
        .iter()
        .map(|field| field.name.as_str())
        .collect();
    assert_eq!(names, ["count", "level", "note", "ok", "score", "seen"]);
    let ok = &form.fields[3];
    assert_eq!(ok.hint, "boolean — Did it work?");
    assert!(ok.required);
    let key = |form: &mut Form, code| form.key(KeyEvent::from(code));
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

    for (schema, flat) in [
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
            Form::new("RED", Some(&schema)).json.is_none(),
            flat,
            "{schema}"
        );
    }
    let empty = Form::new("GREEN", None);
    assert_eq!(empty.result().unwrap(), json!({"verdict":"GREEN"}));
    assert_eq!(
        template(
            &json!({"properties":{"a":{"enum":["x"]},"b":{"type":["integer","null"]},"c":{"$ref":"#/x"}}})
        ),
        json!({"a":"x","b":0,"c":null})
    );
}

#[test]
fn quitting_keeps_or_releases_only_the_claims_this_session_took() {
    let mut review = opened(Some("alice"), demo());
    assert_eq!(press(&mut review, KeyCode::Char('q')), Action::Quit);
    assert_eq!(
        press(&mut review, KeyCode::Char('u')),
        Action::Start(Job::Release {
            ids: vec![ID.into()],
            quit: false
        })
    );
    review.finish(Outcome::Released {
        ids: vec![ID.into()],
        quit: false,
        failed: Vec::new(),
    });
    assert!(screen(&mut review).contains("Claim released."));

    let mut review = opened(None, demo());
    review.finish(Outcome::Ran {
        tool: "notes_release".into(),
        claimed: Some(claim("alice")),
        result: printed("notes", false),
    });
    assert_eq!(press(&mut review, KeyCode::Char('q')), Action::None);
    assert_eq!(review.mode(), &Mode::Leave);
    let text = screen(&mut review);
    assert!(text.contains("This session claimed 1 request(s) without submitting:"));
    assert!(text.contains("  run-1-3"));
    press(&mut review, KeyCode::Esc);
    assert_eq!(review.mode(), &Mode::Request);
    assert_eq!(control(&mut review, 'c'), Action::None);
    assert_eq!(
        control(&mut review, 'c'),
        Action::Quit,
        "Ctrl-C twice keeps the claim"
    );
    press(&mut review, KeyCode::Esc);
    press(&mut review, KeyCode::Char('q'));
    assert_eq!(press(&mut review, KeyCode::Char('k')), Action::Quit);
    let release = Action::Start(Job::Release {
        ids: vec![ID.into()],
        quit: true,
    });
    assert_eq!(press(&mut review, KeyCode::Char('u')), release);
    assert_eq!(
        review.finish(Outcome::Released {
            ids: vec![ID.into()],
            quit: true,
            failed: vec![(ID.into(), "database is locked".into())],
        }),
        Action::Refresh
    );
    assert_eq!(review.taken, [ID]);
    assert!(screen(&mut review).contains("run-1-3: database is locked"));
    assert_eq!(press(&mut review, KeyCode::Char('u')), release);
    assert_eq!(
        review.finish(Outcome::Released {
            ids: vec![ID.into()],
            quit: true,
            failed: Vec::new(),
        }),
        Action::Quit
    );
    assert!(review.taken.is_empty());
}

#[test]
fn a_running_job_only_scrolls_or_cancels() {
    let mut review = opened(None, demo());
    drop(review.start(Job::Run {
        id: ID.into(),
        tool: "notes_release".into(),
        claim: true,
    }));
    assert!(review.busy());
    let text = screen(&mut review);
    assert!(
        text.contains("Running notes_release…") && text.contains("Esc cancel"),
        "{text}"
    );
    for key in [KeyCode::Char('s'), KeyCode::Char('q'), KeyCode::Enter] {
        assert_eq!(press(&mut review, key), Action::None);
        assert_eq!(review.mode(), &Mode::Request);
    }
    press(&mut review, KeyCode::PageDown);
    assert_eq!(review.scroll, 10);
    let cancel = review.busy.as_ref().unwrap().cancel.clone();
    press(&mut review, KeyCode::Esc);
    assert!(cancel.is_cancelled());
    review.finish(Outcome::Ran {
        tool: "notes_release".into(),
        claimed: None,
        result: printed("Human tool call was cancelled.", true),
    });
    assert!(!review.busy());
}

#[test]
fn list_opens_requests_and_returns() {
    let mut review = Review::new("/state".into(), None, "alice".into(), None);
    let mut other = view("WAITING_HUMAN", Some("bob"), demo());
    other.request.id = "run-2-1".parse().unwrap();
    review.set_waiting(vec![other, view("WAITING_HUMAN", Some("alice"), demo())]);
    review.refreshed = Some(OffsetDateTime::now_utc());
    let text = screen(&mut review);
    assert!(text.contains("Waiting Human reviews (2)") && text.contains("all repositories"));
    assert!(
        text.contains("alice (you)") && text.contains("bob"),
        "{text}"
    );
    assert!(text.contains("/repo"));
    press(&mut review, KeyCode::Char('j'));
    assert_eq!(press(&mut review, KeyCode::Enter), Action::Refresh);
    assert_eq!(review.open.as_deref(), Some(ID));
    assert_eq!(review.mode(), &Mode::Request);
    assert_eq!(press(&mut review, KeyCode::Esc), Action::Refresh);
    assert_eq!((review.mode(), review.open.as_deref()), (&Mode::List, None));
    assert_eq!(press(&mut review, KeyCode::Char('r')), Action::Refresh);
    assert_eq!(press(&mut review, KeyCode::Esc), Action::Quit);
}
