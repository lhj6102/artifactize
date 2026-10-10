use super::*;
use std::path::PathBuf;
use tokio::{
    net::{TcpListener, TcpStream},
    process::{Child, Command},
};

fn temporary_state() -> tempfile::TempDir {
    tempfile::tempdir().unwrap()
}
async fn next(subscription: &mut Subscription) -> Change {
    tokio::time::timeout(Duration::from_secs(4), subscription.next())
        .await
        .unwrap()
}
async fn registration(subscription: &Subscription, previous: Option<&str>) -> String {
    // Wait for the real ACK, including a different hub epoch after owner death.
    let mut registered = subscription.inbox.registration.subscribe();
    tokio::time::timeout(Duration::from_secs(4), async {
        loop {
            if let Some(epoch) = registered.borrow_and_update().as_ref()
                && Some(epoch.as_str()) != previous
            {
                return epoch.clone();
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

#[test]
fn endpoint_names_keep_the_same_twenty_four_hex_character_directory_prefix() {
    let state = temporary_state();
    let endpoint = Endpoint::new(state.path()).unwrap();
    #[cfg(unix)]
    let directory = endpoint.address.parent().unwrap();
    #[cfg(unix)]
    {
        // SAFETY: geteuid has no preconditions; compare against the existing naming contract.
        let user = unsafe { libc::geteuid() };
        assert_eq!(
            directory,
            Path::new(if cfg!(target_os = "macos") {
                "/private/tmp"
            } else {
                "/tmp"
            })
            .join(format!(
                "artifactize-ipc-{user}-{}",
                &endpoint.identity[..24]
            ))
        );
        assert_eq!(endpoint.address.file_name().unwrap(), "hub.sock");
    }
    #[cfg(windows)]
    {
        // Pipe names keep the full identity; only their election directory is shortened.
        assert_eq!(
            endpoint.address,
            PathBuf::from(format!(
                r"\\.\pipe\artifactize-changes-{}",
                endpoint.identity
            ))
        );
    }
}

#[test]
fn dirty_sessions_are_bounded_and_overflow_coalesces_to_resync() {
    let mut dirty = Dirty::default();
    for index in 0..=MAX_DIRTY_SESSIONS {
        dirty.add(Change::SessionInvalidated(
            format!("session-{index}").parse().unwrap(),
        ));
    }
    assert!(dirty.sessions.is_empty());
    assert_eq!(dirty.pop(), Some(Change::Resync));
    assert_eq!(dirty.pop(), None);
    for _ in 0..1000 {
        dirty.add(Change::StateInvalidated);
    }
    assert_eq!(dirty.pop(), Some(Change::StateInvalidated));
    assert_eq!(dirty.pop(), None);
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
            version: VERSION + 1,
            identity: endpoint.identity.clone(),
            subscriber: true,
        },
    )
    .await
    .unwrap();
    assert!(
        tokio::time::timeout(DELIVERY_TIMEOUT, read_frame(&mut stream))
            .await
            .unwrap()
            .is_err()
    );
}

#[tokio::test]
async fn oversized_frames_are_refused_before_reading_the_payload() {
    let (mut writer, mut reader) = tokio::io::duplex(16);
    writer
        .write_u32((MAX_FRAME_BYTES + 1) as u32)
        .await
        .unwrap();
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
    let mut reloads = 1;
    for _ in 0..20 {
        reloads += usize::from(probe.changed().await);
    }
    assert_eq!(reloads, 1);
    db.execute("INSERT INTO fixture VALUES(1)", []).unwrap();
    assert!(probe.changed().await);
    assert!(!probe.changed().await);
}
#[cfg(unix)]
#[tokio::test]
async fn database_replacement_and_deletion_force_resync() {
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
}
#[cfg(unix)]
#[test]
fn unsafe_endpoint_directory_and_lock_are_refused_not_repaired() {
    use std::os::unix::fs::PermissionsExt;
    let state = temporary_state();
    let endpoint = Endpoint::new(state.path()).unwrap();
    let directory = endpoint.address.parent().unwrap();
    std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(endpoint.elect().is_err());
    assert_eq!(
        std::fs::metadata(directory).unwrap().permissions().mode() & 0o777,
        0o755
    );
    std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o700)).unwrap();
    let target = state.path().join("foreign-lock");
    std::fs::write(&target, "untouched").unwrap();
    std::os::unix::fs::symlink(&target, directory.join("owner.lock")).unwrap();
    assert!(endpoint.elect().is_err());
    assert_eq!(std::fs::read_to_string(target).unwrap(), "untouched");
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
    let connection = crate::store::receipts::NotifyingConnection::new(inner, state.path());
    let (entered, receiving) = tokio::sync::oneshot::channel();
    let (release, waiting) = std::sync::mpsc::channel();
    let caller = tokio::spawn(async move {
        connection
            .call(move |db| -> Result<(), rusqlite::Error> {
                let _ = entered.send(());
                waiting.recv().unwrap();
                db.execute("INSERT INTO fixture VALUES(1)", [])?;
                Ok(())
            })
            .await
    });
    receiving.await.unwrap();
    caller.abort();
    let _ = caller.await;
    release.send(()).unwrap();
    assert_eq!(next(&mut subscriber).await, Change::StateInvalidated);
    let db = rusqlite::Connection::open(state.path().join(crate::store::DATABASE)).unwrap();
    assert_eq!(
        db.query_row("SELECT count(*) FROM fixture", [], |row| row
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
}

#[tokio::test]
async fn a_slow_inbox_overflow_keeps_no_unbounded_session_history() {
    let state = temporary_state();
    let mut subscriber = Subscription::new(state.path()).await;
    registered(&mut subscriber).await;
    // Exercise the receiver-side bound without relying on OS socket buffer size.
    for index in 0..=MAX_DIRTY_SESSIONS {
        subscriber.inbox.add(Change::SessionInvalidated(
            format!("slow-{index}").parse().unwrap(),
        ));
    }
    assert_eq!(next(&mut subscriber).await, Change::Resync);
    assert!(subscriber.inbox.dirty.lock().unwrap().is_empty());
}

#[cfg(unix)]
#[tokio::test]
async fn probe_refuses_linked_sqlite_sidecars_before_opening_sqlite() {
    let state = temporary_state();
    let database = state.path().join(crate::store::DATABASE);
    rusqlite::Connection::open(&database).unwrap();
    let target = state.path().join("unrelated");
    std::fs::write(&target, "untouched").unwrap();
    std::os::unix::fs::symlink(&target, state.path().join("state.sqlite-shm")).unwrap();
    let mut probe = probe::Probe::new(state.path().into());
    assert!(probe.changed().await);
    assert_eq!(std::fs::read_to_string(target).unwrap(), "untouched");
}

// Child processes run this exact fixture test, never a provider, installed binary or default state.
#[tokio::test]
async fn multiprocess_fixture() {
    let Some(role) = std::env::var_os("ARTIFACTIZE_IPC_FIXTURE_ROLE") else {
        return;
    };
    let state = PathBuf::from(std::env::var_os("ARTIFACTIZE_IPC_FIXTURE_STATE").unwrap());
    let address = std::env::var("ARTIFACTIZE_IPC_FIXTURE_CONTROL").unwrap();
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
    }
}
async fn child(state: &Path, role: &str) -> (Child, TcpStream) {
    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "changes::tests::multiprocess_fixture",
            "--nocapture",
        ])
        .env("ARTIFACTIZE_IPC_FIXTURE_STATE", state)
        .env(
            "ARTIFACTIZE_IPC_FIXTURE_CONTROL",
            listener.local_addr().unwrap().to_string(),
        )
        .env("ARTIFACTIZE_IPC_FIXTURE_ROLE", role)
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let (control, _) = tokio::time::timeout(Duration::from_secs(6), listener.accept())
        .await
        .unwrap()
        .unwrap();
    (child, control)
}
async fn phase(control: &mut TcpStream, expected: u8) {
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(6), control.read_u8())
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
    assert!(writer.wait().await.unwrap().success());
    phase(&mut owner_control, 2).await;
    phase(&mut reader_control, 2).await;
    // No database commit accompanies hub death. The existing reader must reconnect.
    owner.kill().await.unwrap();
    owner.wait().await.unwrap();
    let mut replacement = Subscription::new(state.path()).await;
    registered(&mut replacement).await;
    phase(&mut reader_control, 3).await;
    Publisher::new(state.path()).notify(Change::SessionInvalidated("after-crash".parse().unwrap()));
    drain().await;
    phase(&mut reader_control, 4).await;
    assert!(reader.wait().await.unwrap().success());
}
