//! Saved Agent conversation files for the unit tests of both crates: a session with a
//! header, and helpers that write and read it.
use crate::agent::session::{
    SessionRef, directory,
    document::Position,
    live::{self, Reader, Source},
    path,
};
use crate::store;
use serde_json::json;
use std::{fs, io::Write};

pub(crate) fn fixture() -> (tempfile::TempDir, Source) {
    let root = crate::test_os::tempdir();
    crate::test_os::create_private_dir_all(&directory(root.path()));
    let source = Source {
        state: root.path().into(),
        reference: SessionRef {
            producer: store::Producer::current().name,
            state: "state".parse().unwrap(),
            run_id: "run".parse().unwrap(),
            request_id: "request".parse().unwrap(),
            session_id: "session".parse().unwrap(),
        },
        saved: true,
    };
    write(&source, &header(&source));
    (root, source)
}
pub fn header(source: &Source) -> String {
    format!(
        "{}\n",
        json!({
            "kind":"review",
            "version":1,
            "sessionId":source.reference.session_id,
            "runId":source.reference.run_id,
            "requestId":source.reference.request_id,
            "producer":source.reference.producer,
            "state":source.reference.state,
            "futureMetadata":7,
        })
    )
}
pub(crate) fn write(source: &Source, text: &str) {
    let path = path(&source.state, &source.reference.session_id).unwrap();
    crate::test_os::rewrite_private_file(&path, text);
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
