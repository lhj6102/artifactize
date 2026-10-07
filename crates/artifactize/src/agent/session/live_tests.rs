use super::{
    document::{self, Mode, Move, Position, Scroll},
    live::{self, Reader, Resolution, Source},
    *,
};
use crate::{platform, store};
use serde_json::json;
use std::{fs, io::Write};

pub(crate) fn fixture() -> (tempfile::TempDir, Source) {
    let root = tempfile::tempdir().unwrap();
    platform::create_private_dir_all(&directory(root.path())).unwrap();
    let source = Source {
        state: root.path().into(),
        reference: SessionRef {
            producer: store::Producer::current().name,
            state: "state".into(),
            run_id: "run".parse().unwrap(),
            request_id: "request".parse().unwrap(),
            session_id: "session".parse().unwrap(),
        },
        saved: true,
    };
    write(&source, &header(&source));
    (root, source)
}
fn header(source: &Source) -> String {
    format!(
        "{}\n",
        json!({"kind":"review","version":1,"sessionId":source.reference.session_id,"runId":source.reference.run_id,"requestId":source.reference.request_id,"producer":source.reference.producer,"state":source.reference.state,"futureMetadata":7})
    )
}
pub(crate) fn write(source: &Source, text: &str) {
    let path = path(&source.state, &source.reference.session_id).unwrap();
    let mut file = platform::private_options()
        .write(true)
        .create(true)
        .truncate(true)
        .open(path)
        .unwrap();
    file.write_all(text.as_bytes()).unwrap();
    platform::restrict_file(&file).unwrap();
}
pub(crate) fn append(source: &Source, text: &str) {
    fs::OpenOptions::new()
        .append(true)
        .open(path(&source.state, &source.reference.session_id).unwrap())
        .unwrap()
        .write_all(text.as_bytes())
        .unwrap();
}
pub(crate) fn answer(text: &str) -> String {
    format!("{}\n", json!({"kind":"answer","text":text}))
}
pub(crate) fn finish(
    reader: &mut Reader,
    width: usize,
    height: usize,
    mut position: Position,
) -> live::Window {
    for _ in 0..10000 {
        let window = reader.step(width, height, position);
        if !window.loading {
            return window;
        }
        position = window.position;
    }
    panic!("bounded indexing did not finish");
}

#[test]
fn split_json_newline_and_utf8_tail_wait_for_complete_records_and_end_is_not_terminal() {
    let (_root, source) = fixture();
    let mut reader = Reader::new(source.clone());
    finish(&mut reader, 80, 20, Position::Bottom);
    let bytes = answer("late 한글 e\u{301}").into_bytes();
    let split = bytes.iter().position(|byte| *byte >= 128).unwrap() + 1;
    let mut file = fs::OpenOptions::new()
        .append(true)
        .open(path(&source.state, "session").unwrap())
        .unwrap();
    file.write_all(&bytes[..split]).unwrap();
    let before = finish(&mut reader, 80, 20, Position::Bottom);
    assert_eq!(reader.records_len(), 1);
    assert!(!before.rows.join("\n").contains("late"));
    file.write_all(&bytes[split..bytes.len() - 1]).unwrap();
    finish(&mut reader, 80, 20, Position::Bottom);
    assert_eq!(reader.records_len(), 1);
    file.write_all(b"\n").unwrap();
    assert!(
        finish(&mut reader, 80, 20, Position::Bottom)
            .rows
            .join("\n")
            .contains("late 한글 e\u{301}")
    );
    append(
        &source,
        "{\"kind\":\"end\"}\n{\"kind\":\"send\",\"text\":\"later send\"}\n",
    );
    assert!(
        finish(&mut reader, 80, 30, Position::Bottom)
            .rows
            .join("\n")
            .contains("later send")
    );
}

#[test]
fn typed_policy_rejects_complete_malformed_unknown_foreign_and_accepts_legacy_metadata() {
    for invalid in [
        "{bad}\n",
        "{\"kind\":\"unknown\"}\n",
        "{\"kind\":\"message\",\"message\":17}\n",
        "malformed\n",
    ] {
        let (_root, source) = fixture();
        append(&source, invalid);
        let mut reader = Reader::new(source);
        let error = finish(&mut reader, 30, 10, Position::Bottom);
        assert!(
            error
                .status
                .unwrap()
                .contains("Invalid saved session event")
        );
        let bytes = reader.bytes_read;
        for _ in 0..10 {
            assert!(!reader.step(30, 10, Position::Bottom).loading);
        }
        assert!(
            reader.bytes_read - bytes <= 10 * 1024,
            "an unchanged malformed file was reindexed"
        );
    }
    let (_root, source) = fixture();
    write(&source, "{\"kind\":\"review\",\"sessionId\":\"other\"}\n");
    let error = finish(&mut Reader::new(source.clone()), 30, 10, Position::Bottom);
    assert!(error.status.unwrap().contains("identity"));
    write(&source, "{\"kind\":\"review\",\"futureMetadata\":7}\n");
    assert!(
        finish(&mut Reader::new(source), 30, 10, Position::Bottom)
            .status
            .is_none()
    );
}

#[test]
fn huge_small_records_bounded_batches_cache_and_append_byte_accounting() {
    let (_root, source) = fixture();
    let attempt = format!(
        "{}\n",
        json!({"kind":"attempt","turn":1,"attempt":1,"usage":{},"padding":"x".repeat(500)})
    );
    append(&source, &attempt.repeat(40000));
    append(&source, &answer("history end"));
    let size = fs::metadata(path(&source.state, "session").unwrap())
        .unwrap()
        .len();
    assert!(size > 16 * 1024 * 1024);
    let mut reader = Reader::new(source.clone());
    let first = reader.step(80, 30, Position::Bottom);
    assert!(first.loading);
    assert!(reader.records_len() <= 128);
    let window = finish(&mut reader, 80, 30, Position::Bottom);
    assert!(window.status.is_none(), "{:?}", window.status);
    assert!(window.rows.join("\n").contains("history end"));
    assert!(
        reader.bytes_read < size + 4 * 1024 * 1024,
        "hidden records were repeatedly paged in"
    );
    assert_eq!(reader.decoded_events, 40002);
    let before = reader.bytes_read;
    for n in 0..30 {
        append(&source, &answer(&format!("small append {n}")));
        finish(&mut reader, 80, 10, Position::Bottom);
    }
    assert!(
        reader.bytes_read - before < 100000,
        "small append reread large history"
    );
    assert!(finish(&mut reader, 80, 1, Position::Row(0)).rows[0].contains("Session"));
}

#[test]
fn normal_large_tool_result_record_decodes_once_and_disk_rows_page_in_above_u16() {
    use rig_core::message::{Message, ToolCall, ToolFunction, ToolName, ToolResultContent};
    let (_root, source) = fixture();
    let results = (0..4)
        .map(|n| {
            ToolCall::from_wire(
                format!("call-{n}"),
                ToolFunction::new(ToolName::new("read").unwrap(), json!({})),
            )
            .result(vec![ToolResultContent::text("x".repeat(5 * 1024 * 1024))])
        })
        .collect();
    let event = Event {
        at: None,
        send: None,
        kind: Kind::Message(MessageEvent {
            turn: 1,
            message: Message::tool_results(results),
            repair: false,
            is_error: vec![false; 4],
            question: None,
        }),
    };
    let bytes = serde_json::to_string(&event).unwrap();
    assert!(bytes.len() > 16 * 1024 * 1024);
    append(&source, &(bytes + "\n"));
    let mut reader = Reader::new(source.clone());
    let window = finish(&mut reader, 7, 10, Position::Bottom);
    assert!(window.status.is_none(), "{:?}", window.status);
    assert!(window.total > 65535);
    assert_eq!(reader.decoded_events, 2);
    reader.commit(7);
    for position in [Position::Row(0), Position::Bottom, Position::Row(70000)] {
        assert!(finish(&mut reader, 7, 10, position).status.is_none());
    }
    append(&source, &answer("late after large record"));
    assert!(
        finish(&mut reader, 40, 10, Position::Bottom)
            .rows
            .join("\n")
            .contains("late after large record")
    );
    assert_eq!(reader.decoded_events, 3);
}

#[test]
fn truncate_replacement_same_size_and_gc_reset_even_when_paused() {
    let (_root, source) = fixture();
    append(&source, &answer("aaaa"));
    let mut reader = Reader::new(source.clone());
    finish(&mut reader, 80, 2, Position::Bottom);
    write(&source, &(header(&source) + &answer("bbbb")));
    let same = finish(&mut reader, 80, 10, Position::Row(0));
    assert!(same.reset);
    assert!(same.rows.join("\n").contains("bbbb"));
    write(&source, &header(&source));
    assert!(finish(&mut reader, 80, 10, Position::Row(5)).reset);
    let path = path(&source.state, "session").unwrap();
    let replacement = path.with_extension("new");
    fs::write(&replacement, header(&source) + &answer("replacement")).unwrap();
    platform::restrict_file(&fs::File::open(&replacement).unwrap()).unwrap();
    fs::rename(replacement, &path).unwrap();
    assert!(finish(&mut reader, 80, 20, Position::Bottom).reset);
    fs::remove_file(path).unwrap();
    let removed = reader.step(80, 20, Position::Bottom);
    assert!(removed.reset);
    assert_eq!(removed.total, 0);
    assert!(removed.status.unwrap().contains("session GC"));
}

#[cfg(unix)]
#[test]
fn links_fifo_permissions_and_directory_links_are_refused() {
    use std::os::unix::{
        ffi::OsStrExt,
        fs::{PermissionsExt, symlink},
    };
    let (root, source) = fixture();
    let file = path(&source.state, "session").unwrap();
    fs::set_permissions(&file, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(
        Reader::new(source.clone())
            .step(80, 20, Position::Bottom)
            .status
            .unwrap()
            .contains("private")
    );
    fs::remove_file(&file).unwrap();
    symlink("/etc/passwd", &file).unwrap();
    assert!(
        Reader::new(source.clone())
            .step(80, 20, Position::Bottom)
            .status
            .is_some()
    );
    fs::remove_file(&file).unwrap();
    let name = std::ffi::CString::new(file.as_os_str().as_bytes()).unwrap();
    // SAFETY: name is a valid NUL-terminated fixture path and mode is a valid permission mask.
    assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
    assert!(
        Reader::new(source.clone())
            .step(80, 20, Position::Bottom)
            .status
            .unwrap()
            .contains("regular")
    );
    fs::remove_file(file).unwrap();
    fs::remove_dir(directory(root.path())).unwrap();
    let elsewhere = root.path().join("elsewhere");
    platform::create_private_dir_all(&elsewhere).unwrap();
    symlink(elsewhere, directory(root.path())).unwrap();
    assert!(
        Reader::new(source)
            .step(80, 20, Position::Bottom)
            .status
            .is_some()
    );
}

#[test]
fn layout_matches_actual_renderer_for_cjk_combining_halfwidth_and_long_json() {
    use ratatui::{
        buffer::Buffer,
        layout::Rect,
        text::Line,
        widgets::{Paragraph, Widget},
    };
    let text = "한글e\u{301}ｶﾞｶﾞAB\n{\"long\":\"xxxxxxxxxxxxxxxxx\"}\n";
    for width in [1, 2, 3, 7, 20] {
        let ranges = document::rows(text, width);
        let rows = ranges
            .iter()
            .map(|range| document::row(text, range, width))
            .collect::<Vec<_>>();
        let mut buffer = Buffer::empty(Rect::new(0, 0, width as u16, rows.len() as u16));
        Paragraph::new(
            rows.iter()
                .map(|row| Line::from(row.as_str()))
                .collect::<Vec<_>>(),
        )
        .render(buffer.area, &mut buffer);
        for (y, row) in rows.iter().enumerate() {
            use ratatui::buffer::CellWidth;
            let mut x = 0;
            let mut rendered = String::new();
            while x < width {
                let symbol = buffer[(x as u16, y as u16)].symbol();
                rendered.push_str(symbol);
                x += usize::from(symbol.cell_width().max(1));
            }
            assert_eq!(rendered.trim_end(), row.trim_end());
        }
    }
    let rows = document::rows("ｶﾞｶﾞ", 2);
    assert_eq!(rows.len(), 2);
    assert_eq!(document::row("ｶﾞｶﾞ", &rows[0], 2), "ｶﾞ");
}

#[test]
fn controller_empty_exact_viewport_and_more_than_u16_rows() {
    let mut scroll = Scroll::default();
    for total in [0, 10] {
        scroll.update(total, 10, None);
        scroll.apply(Move::Top);
        assert!(scroll.at_bottom());
        assert_eq!(scroll.mode, Mode::Following);
    }
    scroll.update(100000, 10, None);
    assert_eq!(scroll.top, 99990);
    scroll.apply(Move::Up(3));
    assert_eq!(scroll.top, 99987);
    assert_eq!(scroll.mode, Mode::Paused);
    scroll.update(100100, 10, None);
    assert_eq!(scroll.top, 99987);
    scroll.apply(Move::Bottom);
    assert_eq!(scroll.top, 100090);
    assert_eq!(scroll.mode, Mode::Following);
    scroll.apply(Move::Top);
    assert_eq!(scroll.top, 0);
    assert_eq!(scroll.mode, Mode::Paused);
}

#[tokio::test]
async fn resolution_running_foreign_and_remote_never_uses_remote_path() {
    let root = tempfile::tempdir().unwrap();
    let state = root.path().join("state");
    let repo = root.path().join("repo");
    fs::create_dir(&repo).unwrap();
    let receipts = store::Receipts::open(&state, &repo).await.unwrap();
    let mut view = crate::monitor::tests::request(
        "app/check",
        "RUNNING",
        json!({"profile":{"kind":"agent","backend":"openai","model":"fixture"},"sessionId":"session"}),
    );
    let Resolution::Local(source) = live::resolve(&state, &view).await.unwrap() else {
        panic!("running local");
    };
    assert_eq!(source.reference.request_id, view.request.id);
    assert!(!source.saved);
    view.request.session = Some(source.reference.clone());
    assert!(matches!(
        live::resolve(&state, &view).await.unwrap(),
        Resolution::Local(Source { saved: true, .. })
    ));
    view.request.session.as_mut().unwrap().state = "foreign-state".into();
    assert!(matches!(
        live::resolve(&state, &view).await.unwrap(),
        Resolution::Unavailable(_)
    ));
    view.request.session = None;
    view.request.producer = Some(store::Producer {
        name: "foreign@host".into(),
        version: "fixture".into(),
        session: None,
    });
    assert!(matches!(
        live::resolve(&state, &view).await.unwrap(),
        Resolution::Unavailable(_)
    ));
    assert!(receipts.state_id().await.unwrap().len() > 1);
}

#[tokio::test]
async fn reused_original_and_execution_reference_resolve_the_original_identity() {
    let root = tempfile::tempdir().unwrap();
    let state = root.path().join("state");
    let repo = root.path().join("repo");
    fs::create_dir(&repo).unwrap();
    let receipts = store::Receipts::open(&state, &repo).await.unwrap();
    let run: store::Run = serde_json::from_value(json!({"id":"run-1","repoPath":repo,"stateDir":state,"status":"RUNNING","createdAt":"2026-01-01T00:00:00Z","selection":{"kind":"all"},"validation":null})).unwrap();
    let mut original = crate::monitor::tests::request(
        "app/original",
        "RUNNING",
        json!({"profile":{"kind":"agent","backend":"openai","model":"fixture"}}),
    );
    let provenance: store::Provenance = serde_json::from_value(json!({"repoPath":repo,"runId":"run-1","requestId":original.request.id,"evalId":"app/original","evalDefHash":"hash","completedAt":null})).unwrap();
    let mut follower = crate::monitor::tests::request(
        "app/follower",
        "QUEUED",
        json!({"profile":{"kind":"agent","backend":"openai","model":"fixture"},"provenance":provenance}),
    );
    receipts
        .create_run(&run, &[original.request.clone(), follower.request.clone()])
        .await
        .unwrap();
    assert!(matches!(
        live::resolve(&state, &follower).await.unwrap(),
        Resolution::Unavailable(_)
    ));
    original.request.session_id = Some("original-session".parse().unwrap());
    receipts.save_request(&original.request).await.unwrap();
    let Resolution::Local(source) = live::resolve(&state, &follower).await.unwrap() else {
        panic!("running follower must resolve owner");
    };
    assert_eq!(source.reference.request_id, original.request.id);
    assert_eq!(source.reference.session_id.as_str(), "original-session");
    let mut producer = store::Producer::current();
    producer.session = Some(source.reference.clone());
    follower.request.provenance = None;
    follower.execution = Some(serde_json::from_value(json!({"id":"execution-fixture","key":null,"fingerprint":null,"evalDefHash":"hash","ownerPid":0,"ownerStartTime":0,"status":"RUNNING","result":null,"error":null,"errorCode":null,"profile":{"kind":"agent","backend":"openai","model":"fixture"},"usage":null,"provenance":provenance,"startedAt":"2026-01-01T00:00:00Z","completedAt":null,"producer":producer})).unwrap());
    let Resolution::Local(execution_source) = live::resolve(&state, &follower).await.unwrap()
    else {
        panic!("execution reference");
    };
    assert_eq!(execution_source.reference, source.reference);
    assert!(execution_source.saved);
    follower.request.session = Some(SessionRef {
        state: "remote-state".into(),
        producer: "remote@host".into(),
        ..source.reference
    });
    assert!(matches!(
        live::resolve(&state, &follower).await.unwrap(),
        Resolution::Unavailable(_)
    ));
}
