//! Recorder effects and references follow its enum state, not independent optional fields.
use super::*;
use std::{fs, time::Duration};

fn fixture() -> (tempfile::TempDir, Saving, Request, crate::types::SessionId) {
    let root = crate::test_os::tempdir();
    let saving = Saving {
        state: root.path().into(),
        state_id: "fixture-state".parse().unwrap(),
        producer: "fixture@host".into(),
    };
    let request = crate::monitor::tests::request(
        "app/check",
        "RUNNING",
        json!({"profile":{"kind":"agent","backend":"openai","model":"fixture"}}),
    )
    .request;
    (root, saving, request, "fixture-session".parse().unwrap())
}

#[test]
fn disabled_and_pending_recorders_do_not_create_files_or_references_before_start() {
    let (_root, saving, request, id) = fixture();
    let mut off = Recorder::new(None, &request, &id);
    off.start(Header::default());
    off.message(1, &Message::user("not saved"), false);
    off.event(Kind::End(End::default()));
    assert!(matches!(off.state, Recording::Off));
    assert!(off.reference().is_none());
    assert!(!directory(&saving.state).exists());
    let mut pending = Recorder::new(Some(&saving), &request, &id);
    pending.event(Kind::Answer(Answer::Completed("before header".into())));
    assert!(matches!(pending.state, Recording::Pending(_)));
    assert!(pending.reference().is_none());
    assert!(!directory(&saving.state).exists());
}

#[test]
fn initial_header_is_lazy_authoritative_and_end_preserves_saved_reference() {
    let (_root, saving, request, id) = fixture();
    let mut recorder = Recorder::new(Some(&saving), &request, &id);
    assert_eq!(recorder.id(), id.as_str());
    recorder.start(Header {
        version: Some(999),
        session_id: Some("wrong".parse().unwrap()),
        producer: Some("wrong@host".into()),
        model: Some("fixture-model".parse().unwrap()),
        ..Header::default()
    });
    let reference = recorder.reference().unwrap().clone();
    assert_eq!(reference.request_id, request.id);
    assert_eq!(reference.run_id, request.run_id);
    assert_eq!(reference.state, saving.state_id);
    assert_eq!(reference.producer, saving.producer);
    assert_eq!(reference.session_id, id);
    recorder.start(Header {
        model: Some("ignored".parse().unwrap()),
        ..Header::default()
    });
    recorder.message(1, &Message::user("recorded"), false);
    recorder.event(Kind::End(End::Completed(
        serde_json::from_value(json!({"verdict":"GREEN"})).unwrap(),
    )));
    recorder.event(Kind::Answer(Answer::Completed("after end".into())));
    assert_eq!(recorder.reference(), Some(&reference));
    let conversation = Conversation::load(&path(&saving.state, &id).unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(conversation.events.len(), 4);
    assert_eq!(conversation.header().version, Some(VERSION));
    assert_eq!(
        conversation.header().model.as_deref(),
        Some("fixture-model")
    );
    assert_eq!(conversation.header().eval_id.as_deref(), Some("app/check"));
    assert!(
        conversation
            .events
            .iter()
            .all(|event| event.at.is_some() && event.send.is_none())
    );
    assert!(matches!(
        &conversation.events[3].kind,
        Kind::Answer(answer) if answer.text() == Some("after end")
    ));
}

#[test]
fn open_failure_disables_future_events_and_append_does_not_create_missing_files() {
    let (_root, saving, request, id) = fixture();
    fs::write(directory(&saving.state), "directory blocked").unwrap();
    let mut recorder = Recorder::new(Some(&saving), &request, &id);
    recorder.start(Header::default());
    assert!(matches!(recorder.state, Recording::Failed));
    assert!(recorder.reference().is_none());
    fs::remove_file(directory(&saving.state)).unwrap();
    recorder.start(Header::default());
    recorder.event(Kind::End(End::default()));
    assert!(!directory(&saving.state).exists());
    let missing = path(&saving.state, &id).unwrap();
    assert!(Recorder::append(&saving.state, &missing, &id, 1).is_err());
    assert!(!missing.exists());
}

#[test]
fn followup_has_send_number_no_new_header_and_no_new_saved_review_reference() {
    let (_root, saving, request, id) = fixture();
    let mut review = Recorder::new(Some(&saving), &request, &id);
    review.start(Header::default());
    let reference = review.reference().unwrap().clone();
    review.event(Kind::End(End::default()));
    drop(review);
    let path = path(&saving.state, &id).unwrap();
    let mut follow = Recorder::append(&saving.state, &path, &id, 7).unwrap();
    follow.start(Header::default());
    follow.event(Kind::Send(Send {
        text: Some("follow".into()),
        ..Send::default()
    }));
    follow.message(2, &Message::user("follow-up message"), false);
    follow.event(Kind::Answer(Answer::Completed("answer".into())));
    assert!(follow.reference().is_none());
    let conversation = Conversation::load(&path).unwrap().unwrap();
    assert_eq!(conversation.events.len(), 5);
    assert_eq!(
        conversation.header().request_id.as_ref(),
        Some(&reference.request_id)
    );
    assert!(
        conversation.events[..2]
            .iter()
            .all(|event| event.send.is_none())
    );
    assert!(
        conversation.events[2..]
            .iter()
            .all(|event| event.send == Some(7))
    );
}

fn fail_file() -> (tempfile::TempDir, File) {
    let root = crate::test_os::tempdir();
    let path = root.path().join("read-only-writer");
    fs::write(&path, "unchanged").unwrap();
    let file = File::open(path).unwrap();
    (root, file)
}

#[test]
fn first_header_write_failure_never_promotes_reference_and_later_failure_retains_it() {
    let (_root, saving, request, id) = fixture();
    let mut recorder = Recorder::new(Some(&saving), &request, &id);
    let Recording::Pending(pending) = std::mem::replace(&mut recorder.state, Recording::Off) else {
        panic!("pending");
    };
    let (_bad_root, file) = fail_file();
    recorder.start_file(file, pending.reference, Header::default());
    assert!(matches!(recorder.state, Recording::Failed));
    assert!(recorder.reference().is_none());
    recorder.event(Kind::End(End::default()));
    assert!(matches!(recorder.state, Recording::Failed));
    let mut review = Recorder::new(Some(&saving), &request, &id);
    review.start(Header::default());
    let reference = review.reference().unwrap().clone();
    let (_bad_root, file) = fail_file();
    review.state = Recording::Review {
        file,
        reference: reference.clone(),
    };
    review.event(Kind::End(End::default()));
    assert!(matches!(review.state, Recording::SavedFailure(_)));
    assert_eq!(review.reference(), Some(&reference));
    review.start(Header::default());
    review.event(Kind::Answer(Answer::default()));
    assert_eq!(review.reference(), Some(&reference));
    let (_bad_root, file) = fail_file();
    review.state = Recording::FollowUp { file, send: 3 };
    review.event(Kind::Answer(Answer::default()));
    assert!(matches!(review.state, Recording::Failed));
    assert!(review.reference().is_none());
}

#[tokio::test]
async fn only_successful_flushed_events_notify_ipc_and_failed_writes_never_notify() {
    use crate::changes::{Change, Subscription};
    let (_root, saving, request, id) = fixture();
    let mut subscription = Subscription::new(&saving.state).await;
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(4), subscription.next())
            .await
            .unwrap(),
        Change::Resync
    );
    let mut recorder = Recorder::new(Some(&saving), &request, &id);
    recorder.start(Header::default());
    crate::changes::drain().await;
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(4), subscription.next())
            .await
            .unwrap(),
        Change::SessionInvalidated(id.clone())
    );
    let path = path(&saving.state, &id).unwrap();
    assert!(Conversation::load(&path).unwrap().unwrap().events.len() == 1);
    recorder.event(Kind::Answer(Answer::Completed("flushed".into())));
    crate::changes::drain().await;
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(4), subscription.next())
            .await
            .unwrap(),
        Change::SessionInvalidated(id.clone())
    );
    assert!(matches!(
        &Conversation::load(&path).unwrap().unwrap().events[1].kind,
        Kind::Answer(answer) if answer.text() == Some("flushed")
    ));
    let reference = recorder.reference().unwrap().clone();
    let (_bad_root, file) = fail_file();
    recorder.state = Recording::Review { file, reference };
    recorder.event(Kind::End(End::default()));
    recorder.event(Kind::End(End::default()));
    crate::changes::drain().await;
    // Queue an independent sentinel after drain: an erroneous recorder notification
    // would arrive ahead of it. No absence timeout or scheduler assumption is needed.
    let sentinel: crate::types::SessionId = "after-failed-writes".parse().unwrap();
    crate::changes::Publisher::new(&saving.state)
        .notify(Change::SessionInvalidated(sentinel.clone()));
    crate::changes::drain().await;
    assert_eq!(
        subscription.next().await,
        Change::SessionInvalidated(sentinel)
    );
}
