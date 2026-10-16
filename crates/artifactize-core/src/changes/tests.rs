use super::*;
use std::path::PathBuf;
use tokio::net::{TcpListener, TcpStream};

/// A watchdog reports a stuck fixture; it never establishes ordering or expiry.
const WAIT: Duration = Duration::from_secs(6);
/// The invalidation contract retains at most 64 distinct sessions, independently of its storage.
const SESSION_HINT_CAPACITY: u64 = 64;

/// The IPC namespace lives outside the state directory on every platform. This fixture
/// owns both, including election locks, socket names and deliberately unsafe test links.
struct TestState {
    root: tempfile::TempDir,
    endpoint: PathBuf,
}

impl TestState {
    fn path(&self) -> &Path {
        self.root.path()
    }
}

impl Drop for TestState {
    fn drop(&mut self) {
        // Remove only this unique state's namespace. remove_dir_all unlinks an owner.lock
        // symlink without following its target, and needs no permission "repair" first.
        let removed = std::fs::remove_dir_all(&self.endpoint);
        if !std::thread::panicking() {
            removed.unwrap_or_else(|error| {
                panic!(
                    "IPC fixture cleanup failed for {}: {error}",
                    crate::platform::path_text(&self.endpoint)
                )
            });
        }
    }
}

fn temporary_state() -> TestState {
    let root = crate::test_os::tempdir();
    let endpoint = Endpoint::new(root.path()).unwrap();
    TestState {
        root,
        endpoint: endpoint.directory().to_owned(),
    }
}
async fn next(subscription: &mut Subscription) -> Change {
    tokio::time::timeout(crate::test_os::patience(WAIT), subscription.next())
        .await
        .unwrap()
}
async fn registration(subscription: &Subscription, previous: Option<&str>) -> String {
    // Wait for the real ACK, including a different hub epoch after owner death.
    let mut registered = subscription.inbox.registration.subscribe();
    tokio::time::timeout(crate::test_os::patience(WAIT), async {
        loop {
            if let Some(epoch) = registered.borrow_and_update().as_ref()
                && Some(epoch.as_str()) != previous
            {
                return epoch.to_string();
            }
            registered.changed().await.unwrap();
        }
    })
    .await
    .unwrap()
}
async fn registered(subscription: &mut Subscription) {
    registration(subscription, None).await;
    assert_eq!(next(subscription).await, Change::Resync);
}

async fn received(subscription: &Subscription, sequence: u64) {
    let mut hints = subscription.inbox.received.subscribe();
    tokio::time::timeout(crate::test_os::patience(WAIT), async {
        loop {
            if hints
                .borrow_and_update()
                .is_some_and(|last| last >= sequence)
            {
                break;
            }
            hints.changed().await.unwrap();
        }
    })
    .await
    .unwrap();
}

async fn publish_hint(writer: &mut transport::Stream, change: Change) {
    write_frame(writer, &Frame::Publish(change)).await.unwrap();
    assert!(matches!(
        read_frame(writer).await.unwrap(),
        Frame::Delivered
    ));
}

#[tokio::test]
async fn independent_states_deliver_only_their_own_session_hints() {
    let first = temporary_state();
    let second = temporary_state();
    let mut first_reader = Subscription::new(first.path()).await;
    registered(&mut first_reader).await;
    let mut second_reader = Subscription::new(second.path()).await;
    registered(&mut second_reader).await;
    Publisher::new(first.path()).notify(Change::SessionInvalidated("first-state".parse().unwrap()));
    Publisher::new(second.path())
        .notify(Change::SessionInvalidated("second-state".parse().unwrap()));
    drain().await;
    assert_eq!(
        next(&mut first_reader).await,
        Change::SessionInvalidated("first-state".parse().unwrap())
    );
    assert_eq!(
        next(&mut second_reader).await,
        Change::SessionInvalidated("second-state".parse().unwrap())
    );
    first_reader.close().await.unwrap();
    second_reader.close().await.unwrap();
}
#[tokio::test]
async fn unsupported_wire_version_is_refused_before_registration() {
    let state = temporary_state();
    let mut subscriber = Subscription::new(state.path()).await;
    registered(&mut subscriber).await;
    let endpoint = Endpoint::new(state.path()).unwrap();
    let mut stream = endpoint.connect().await.unwrap();
    write_frame(
        &mut stream,
        &Frame::Hello {
            // Only version 1 belongs to the public invalidation protocol.
            version: 2,
            identity: endpoint.identity.clone(),
            subscriber: true,
        },
    )
    .await
    .unwrap();
    assert!(
        tokio::time::timeout(crate::test_os::patience(WAIT), read_frame(&mut stream))
            .await
            .unwrap()
            .is_err()
    );
    subscriber.close().await.unwrap();
}

#[tokio::test]
async fn disconnected_connections_do_not_stop_registered_subscribers() {
    let state = temporary_state();
    let mut subscriber = Subscription::new(state.path()).await;
    registered(&mut subscriber).await;
    let endpoint = Endpoint::new(state.path()).unwrap();
    let epoch = registration(&subscriber, None).await;
    let (mut writer, current) = connect(&endpoint, false).await.unwrap();
    assert_eq!(current, epoch);
    // Close before the hub validates the peer or reads Hello. Darwin may report
    // ENOTCONN during peer lookup; existing connections must remain usable.
    // Register the writer first so Windows has installed its next pipe instance.
    drop(endpoint.connect().await.unwrap());
    write_frame(&mut writer, &Frame::Publish(Change::StateInvalidated))
        .await
        .unwrap();
    assert!(matches!(
        read_frame(&mut writer).await.unwrap(),
        Frame::Delivered
    ));
    assert_eq!(next(&mut subscriber).await, Change::StateInvalidated);
    drop(writer);
    subscriber.close().await.unwrap();
}

#[tokio::test]
async fn oversized_frames_are_refused_before_reading_the_payload() {
    let (mut writer, mut reader) = tokio::io::duplex(16);
    // The wire contract rejects payload lengths beyond 1 KiB, even with no payload sent.
    writer.write_u32(1025).await.unwrap();
    assert_eq!(
        read_frame(&mut reader).await.err().unwrap().kind(),
        std::io::ErrorKind::InvalidData
    );
}
#[tokio::test]
async fn registered_before_snapshot_preserves_racing_dirty() {
    let state = temporary_state();
    let mut subscriber = Subscription::new(state.path()).await;
    registered(&mut subscriber).await;
    let publisher = Publisher::new(state.path());
    // This represents a commit while the reader is still loading its baseline.
    publisher.notify(Change::SessionInvalidated(
        "racing-session".parse().unwrap(),
    ));
    drain().await;
    assert_eq!(
        next(&mut subscriber).await,
        Change::SessionInvalidated("racing-session".parse().unwrap())
    );
    subscriber.close().await.unwrap();
}
#[tokio::test]
async fn two_readers_observe_short_writer_and_hub_owner_exit_without_db_change() {
    let state = temporary_state();
    let mut first = Subscription::new(state.path()).await;
    registered(&mut first).await;
    let mut second = Subscription::new(state.path()).await;
    registered(&mut second).await;
    let publisher = Publisher::new(state.path());
    publisher.notify(Change::StateInvalidated);
    drain().await;
    assert_eq!(next(&mut first).await, Change::StateInvalidated);
    assert_eq!(next(&mut second).await, Change::StateInvalidated);
    let epoch = registration(&second, None).await;
    drop(first);
    registration(&second, Some(&epoch)).await;
    // Disconnect and registration may coalesce; consume only after the new ACK.
    assert_eq!(next(&mut second).await, Change::Resync);
    publisher.notify(Change::SessionInvalidated(
        "after-owner-exit".parse().unwrap(),
    ));
    drain().await;
    assert_eq!(
        next(&mut second).await,
        Change::SessionInvalidated("after-owner-exit".parse().unwrap())
    );
    second.close().await.unwrap();
}
#[tokio::test]
async fn subscribed_fixture_joins_shutdown_before_removing_namespace_on_success_and_unwind() {
    use futures_util::FutureExt;
    for unwind in [false, true] {
        let root = crate::test_os::tempdir();
        let database = root.path().join(crate::store::DATABASE);
        rusqlite::Connection::open(&database).unwrap();
        let namespace = Endpoint::new(root.path()).unwrap().directory().to_owned();
        let result = std::panic::AssertUnwindSafe(super::test_support::subscribed(
            root.path(),
            move |subscription| {
                Box::pin(async move {
                    assert_eq!(next(subscription).await, Change::Resync);
                    assert!(!unwind, "fixture assertion unwind");
                })
            },
        ))
        .catch_unwind()
        .await;
        assert_eq!(result.is_err(), unwind);
        assert!(crate::test_os::database_released(&database).unwrap());
        assert!(
            !namespace.exists(),
            "the external IPC namespace was removed"
        );
        root.close().unwrap();
    }
}

#[test]
fn database_release_probe_tracks_idle_readers_among_unrelated_descriptors() {
    use crate::test_os::database_released;
    use std::fs;

    let root = crate::test_os::tempdir();
    let database = root.path().join("database.sqlite");
    rusqlite::Connection::open(&database).unwrap();
    let alias = root.path().join("alias.sqlite");
    crate::test_os::hard_link(&database, &alias);
    let contents = fs::read(&database).unwrap();
    let unrelated = fs::File::create(root.path().join("unrelated")).unwrap();
    let listener = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();

    for path in [&database, &alias] {
        for observed in [&database, &alias] {
            assert!(database_released(observed).unwrap());
        }
        // Opening read-only without executing a statement holds no SQLite lock.
        // Both names must identify the held file, not a different database.
        let reader =
            rusqlite::Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
                .unwrap();
        for observed in [&database, &alias] {
            assert!(
                !database_released(observed).unwrap(),
                "an idle reader through {path:?} must be detected through {observed:?}"
            );
        }
        reader.close().unwrap();
        for observed in [&database, &alias] {
            assert!(database_released(observed).unwrap());
            assert_eq!(fs::read(observed).unwrap(), contents);
        }
    }

    drop(listener);
    drop(unrelated);
    root.close().unwrap();
}

#[test]
fn database_release_probe_reports_missing_database() {
    let root = crate::test_os::tempdir();
    assert_eq!(
        crate::test_os::database_released(&root.path().join("missing.sqlite"))
            .unwrap_err()
            .kind(),
        std::io::ErrorKind::NotFound
    );
    root.close().unwrap();
}

#[tokio::test]
async fn closed_subscription_releases_database_and_files_before_returning() {
    let state = temporary_state();
    let database = state.path().join(crate::store::DATABASE);
    let writer = rusqlite::Connection::open(&database).unwrap();
    writer
        .execute_batch("PRAGMA journal_mode=WAL; CREATE TABLE fixture(value INTEGER);")
        .unwrap();
    let subscription = Subscription::new(state.path()).await;
    writer.close().unwrap();
    tokio::time::timeout(crate::test_os::patience(WAIT), subscription.close())
        .await
        .expect("subscription shutdown completes")
        .unwrap();

    // Even an idle read-only handle must be gone, without changing SQLite or its names.
    // Do not await again between completed shutdown and observing/reclaiming the files.
    assert!(crate::test_os::database_released(&database).unwrap());
    std::fs::remove_dir_all(state.path()).unwrap();
    assert!(
        !state.path().exists(),
        "closed state can be removed immediately"
    );
}

#[tokio::test]
async fn closing_one_reader_preserves_other_subscriptions() {
    let state = temporary_state();
    let mut owner = Subscription::new(state.path()).await;
    registered(&mut owner).await;
    let mut reader = Subscription::new(state.path()).await;
    registered(&mut reader).await;
    reader.close().await.unwrap();
    let publisher = Publisher::new(state.path());
    publisher.notify(Change::StateInvalidated);
    drain().await;
    assert_eq!(next(&mut owner).await, Change::StateInvalidated);
    owner.close().await.unwrap();
}

#[tokio::test]
async fn idle_probe_never_reloads_and_missed_commit_is_detected_on_persistent_connection() {
    let state = temporary_state();
    let database = state.path().join(crate::store::DATABASE);
    let db = rusqlite::Connection::open(&database).unwrap();
    db.execute_batch("PRAGMA journal_mode=WAL; CREATE TABLE fixture(value INTEGER);")
        .unwrap();
    let mut probe = probe::Probe::new(state.path().into());
    assert!(probe.changed().await);
    for _ in 0..3 {
        assert!(
            !probe.changed().await,
            "idle observation requested a reload"
        );
    }
    db.execute("INSERT INTO fixture VALUES(1)", []).unwrap();
    assert!(probe.changed().await);
    assert!(!probe.changed().await);
    probe.close().await.unwrap();
}
#[tokio::test]
async fn database_replacement_and_deletion_force_resync() {
    // Some operating systems forbid replacement while the observer holds the file open.
    if !crate::test_os::can_replace_open_database() {
        return;
    }
    let state = temporary_state();
    let database = state.path().join(crate::store::DATABASE);
    rusqlite::Connection::open(&database)
        .unwrap()
        .execute_batch("CREATE TABLE fixture(value);")
        .unwrap();
    let mut probe = probe::Probe::new(state.path().into());
    assert!(probe.changed().await);
    assert!(!probe.changed().await);
    std::fs::rename(&database, state.path().join("old.sqlite")).unwrap();
    rusqlite::Connection::open(&database)
        .unwrap()
        .execute_batch("CREATE TABLE replacement(value);")
        .unwrap();
    assert!(probe.changed().await);
    std::fs::remove_file(&database).unwrap();
    assert!(probe.changed().await);
    assert!(!probe.changed().await);
    probe.close().await.unwrap();
}
#[test]
fn unsafe_endpoint_directory_and_lock_are_refused_not_repaired() {
    let state = temporary_state();
    let endpoint = Endpoint::new(state.path()).unwrap();
    let target = state.path().join("foreign-lock");
    std::fs::write(&target, "untouched").unwrap();
    if crate::test_os::symlink_file(&target, endpoint.directory().join("owner.lock")).is_some() {
        assert!(endpoint.elect().is_err());
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "untouched");
    }
    crate::test_os::share_dir(endpoint.directory());
    assert!(endpoint.elect().is_err());
    assert!(!crate::test_os::private_dir(endpoint.directory()));
}

#[test]
fn state_fixture_removes_external_endpoint_without_following_links_on_unwind() {
    let foreign = crate::test_os::tempdir();
    let target = foreign.path().join("foreign-lock");
    std::fs::write(&target, "untouched").unwrap();
    for panicking in [false, true] {
        let state = temporary_state();
        let root = state.path().to_owned();
        let endpoint = state.endpoint.clone();
        let _ = crate::test_os::symlink_file(&target, endpoint.join("owner.lock"));
        crate::test_os::share_dir(&endpoint);
        let result = std::panic::catch_unwind(|| {
            let state = state;
            if panicking {
                panic!("scripted IPC fixture unwind");
            }
            drop(state);
        });
        assert_eq!(result.is_err(), panicking);
        assert!(!endpoint.exists(), "fixture retained its IPC namespace");
        assert!(!root.exists(), "fixture retained its state directory");
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "untouched");
    }
}

/// Release the blocking SQLite fixture even if its awaiting test unwinds.
struct WorkerRelease(std::sync::mpsc::Sender<()>);

impl Drop for WorkerRelease {
    fn drop(&mut self) {
        let _ = self.0.send(());
    }
}

#[tokio::test]
async fn committed_worker_notifies_even_after_awaiting_caller_is_cancelled() {
    let state = temporary_state();
    let inner = tokio_rusqlite::Connection::open(state.path().join(crate::store::DATABASE))
        .await
        .unwrap();
    inner
        .call(|db| db.execute_batch("CREATE TABLE fixture(value INTEGER);"))
        .await
        .unwrap();
    let mut subscriber = Subscription::new(state.path()).await;
    registered(&mut subscriber).await;
    let connection = crate::store::receipts::NotifyingConnection::new(inner.clone(), state.path());
    let (entered, receiving) = tokio::sync::oneshot::channel();
    let (release, waiting) = std::sync::mpsc::channel();
    let release = WorkerRelease(release);
    let caller = tokio_util::task::AbortOnDropHandle::new(tokio::spawn(async move {
        connection
            .call(move |db| -> Result<(), rusqlite::Error> {
                let _ = entered.send(());
                waiting.recv().unwrap();
                db.execute("INSERT INTO fixture VALUES(1)", [])?;
                Ok(())
            })
            .await
    }));
    receiving.await.unwrap();
    caller.abort();
    let _ = caller.await;
    drop(release);
    assert_eq!(next(&mut subscriber).await, Change::StateInvalidated);
    let db = rusqlite::Connection::open_with_flags(
        state.path().join(crate::store::DATABASE),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    assert_eq!(
        db.query_row("SELECT count(*) FROM fixture", [], |row| row
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
    drop(db);
    inner.close().await.unwrap();
    subscriber.close().await.unwrap();
}

#[tokio::test]
async fn a_slow_subscriber_overflow_resyncs_then_accepts_fresh_hints() {
    let state = temporary_state();
    let mut subscriber = Subscription::new(state.path()).await;
    registered(&mut subscriber).await;
    let endpoint = Endpoint::new(state.path()).unwrap();
    let (mut writer, _) = connect(&endpoint, false).await.unwrap();
    for index in 0..=SESSION_HINT_CAPACITY {
        publish_hint(
            &mut writer,
            Change::SessionInvalidated(format!("slow-{index}").parse().unwrap()),
        )
        .await;
    }
    // The hub's Delivered ACK does not mean this subscriber has received the last hint.
    received(&subscriber, SESSION_HINT_CAPACITY + 1).await;
    assert_eq!(next(&mut subscriber).await, Change::Resync);
    publish_hint(
        &mut writer,
        Change::SessionInvalidated("after-overflow".parse().unwrap()),
    )
    .await;
    assert_eq!(
        next(&mut subscriber).await,
        Change::SessionInvalidated("after-overflow".parse().unwrap())
    );
    drop(writer);
    subscriber.close().await.unwrap();
}

#[tokio::test]
async fn repeated_hints_coalesce_without_losing_later_changes() {
    let state = temporary_state();
    let mut subscriber = Subscription::new(state.path()).await;
    registered(&mut subscriber).await;
    let endpoint = Endpoint::new(state.path()).unwrap();
    let (mut writer, _) = connect(&endpoint, false).await.unwrap();
    for change in [
        Change::StateInvalidated,
        Change::StateInvalidated,
        Change::SessionInvalidated("repeated-session".parse().unwrap()),
        Change::SessionInvalidated("repeated-session".parse().unwrap()),
    ] {
        publish_hint(&mut writer, change).await;
    }
    received(&subscriber, 4).await;
    assert_eq!(next(&mut subscriber).await, Change::StateInvalidated);
    assert_eq!(
        next(&mut subscriber).await,
        Change::SessionInvalidated("repeated-session".parse().unwrap())
    );
    publish_hint(
        &mut writer,
        Change::SessionInvalidated("fresh-session".parse().unwrap()),
    )
    .await;
    assert_eq!(
        next(&mut subscriber).await,
        Change::SessionInvalidated("fresh-session".parse().unwrap())
    );
    drop(writer);
    subscriber.close().await.unwrap();
}

#[tokio::test]
async fn probe_refuses_linked_sqlite_sidecars_before_opening_sqlite() {
    let state = temporary_state();
    let database = state.path().join(crate::store::DATABASE);
    rusqlite::Connection::open(&database).unwrap();
    let target = state.path().join("unrelated");
    std::fs::write(&target, "untouched").unwrap();
    if crate::test_os::symlink_file(&target, state.path().join("state.sqlite-shm")).is_none() {
        return;
    }
    let mut probe = probe::Probe::new(state.path().into());
    assert!(probe.changed().await);
    // Refusal keeps requesting reconciliation; a successful idle open would return false.
    assert!(probe.changed().await);
    assert_eq!(std::fs::read_to_string(target).unwrap(), "untouched");
    probe.close().await.unwrap();
}

// Child processes run this exact fixture test, never a provider, installed binary or default state.
#[tokio::test]
async fn multiprocess_fixture() {
    let Some(role) = crate::platform::environment::var("ARTIFACTIZE_IPC_FIXTURE_ROLE") else {
        return;
    };
    let state =
        PathBuf::from(crate::platform::environment::var("ARTIFACTIZE_IPC_FIXTURE_STATE").unwrap());
    let address =
        crate::platform::environment::var_text("ARTIFACTIZE_IPC_FIXTURE_CONTROL").unwrap();
    let mut control = TcpStream::connect(address).await.unwrap();
    if role == "writer" {
        let publisher = Publisher::new(&state);
        publisher.notify(Change::SessionInvalidated(
            "multiprocess-session".parse().unwrap(),
        ));
        drain().await;
    } else {
        let mut subscription = Subscription::new(&state).await;
        registered(&mut subscription).await;
        let epoch = registration(&subscription, None).await;
        control.write_u8(1).await.unwrap();
        loop {
            if next(&mut subscription).await
                == Change::SessionInvalidated("multiprocess-session".parse().unwrap())
            {
                control.write_u8(2).await.unwrap();
                break;
            }
        }
        if role == "owner" {
            std::future::pending::<()>().await;
        } else {
            registration(&subscription, Some(&epoch)).await;
            assert_eq!(next(&mut subscription).await, Change::Resync);
            control.write_u8(3).await.unwrap();
            loop {
                if next(&mut subscription).await
                    == Change::SessionInvalidated("after-crash".parse().unwrap())
                {
                    control.write_u8(4).await.unwrap();
                    break;
                }
            }
        }
        subscription.close().await.unwrap();
    }
}
async fn child(state: &Path, role: &str) -> (crate::test_os::TestChild, TcpStream) {
    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let child = crate::test_os::spawn_test(
        crate::test_os::self_test("changes::tests::multiprocess_fixture")
            .env("ARTIFACTIZE_IPC_FIXTURE_STATE", state)
            .env(
                "ARTIFACTIZE_IPC_FIXTURE_CONTROL",
                listener.local_addr().unwrap().to_string(),
            )
            .env("ARTIFACTIZE_IPC_FIXTURE_ROLE", role),
    );
    let (control, _) = tokio::time::timeout(crate::test_os::patience(WAIT), listener.accept())
        .await
        .unwrap()
        .unwrap();
    (child, control)
}
async fn phase(control: &mut TcpStream, expected: u8) {
    assert_eq!(
        tokio::time::timeout(crate::test_os::patience(WAIT), control.read_u8())
            .await
            .unwrap()
            .unwrap(),
        expected
    );
}
#[tokio::test]
async fn multiprocess_two_readers_short_writer_and_crashed_hub() {
    let state = temporary_state();
    let (mut owner, mut owner_control) = child(state.path(), "owner").await;
    phase(&mut owner_control, 1).await;
    let (mut reader, mut reader_control) = child(state.path(), "reader").await;
    phase(&mut reader_control, 1).await;
    let (mut writer, _) = child(state.path(), "writer").await;
    assert!(writer.wait().await.success());
    phase(&mut owner_control, 2).await;
    phase(&mut reader_control, 2).await;
    // No database commit accompanies hub death. The existing reader must reconnect.
    owner.kill().unwrap();
    owner.wait().await;
    let mut replacement = Subscription::new(state.path()).await;
    registered(&mut replacement).await;
    phase(&mut reader_control, 3).await;
    Publisher::new(state.path()).notify(Change::SessionInvalidated("after-crash".parse().unwrap()));
    drain().await;
    phase(&mut reader_control, 4).await;
    assert!(reader.wait().await.success());
    replacement.close().await.unwrap();
}
