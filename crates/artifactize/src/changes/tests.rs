use super::*;
use std::{path::PathBuf, process::Command};

fn temporary_state() -> tempfile::TempDir {
    tempfile::tempdir().unwrap()
}
async fn next(subscription: &mut Subscription) -> Change {
    tokio::time::timeout(Duration::from_secs(4), subscription.next())
        .await
        .unwrap()
}
async fn registered(subscription: &mut Subscription) {
    // Subscription::new may return a degraded baseline after a bounded connection timeout.
    // IPC-only tests need the actual Registered ACK, not just the fallback Resync.
    tokio::time::timeout(Duration::from_secs(4), async {
        while !subscription
            .inbox
            .connected
            .load(std::sync::atomic::Ordering::SeqCst)
        {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(next(subscription).await, Change::Resync);
    // Let the initial periodic probe finish without consuming later publication hints.
    tokio::time::sleep(Duration::from_millis(50)).await;
    while subscription.inbox.dirty.lock().unwrap().pop().is_some() {}
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
    drop(first);
    assert_eq!(next(&mut second).await, Change::Resync);
    // Wait for the reconnection handshake; its initial resync may coalesce with disconnect.
    tokio::time::sleep(RECONNECT + DELIVERY_TIMEOUT).await;
    while second.inbox.dirty.lock().unwrap().pop().is_some() {}
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
    let marker = PathBuf::from(std::env::var_os("ARTIFACTIZE_IPC_FIXTURE_MARKER").unwrap());
    if role == "writer" {
        Publisher::new(&state).notify(Change::SessionInvalidated(
            "multiprocess-session".parse().unwrap(),
        ));
        drain().await;
    } else {
        let mut subscription = Subscription::new(&state).await;
        registered(&mut subscription).await;
        std::fs::write(marker.with_extension("ready"), "registered").unwrap();
        loop {
            if next(&mut subscription).await
                == Change::SessionInvalidated("multiprocess-session".parse().unwrap())
            {
                std::fs::write(&marker, "observed").unwrap();
                break;
            }
        }
        if role == "owner" {
            std::future::pending::<()>().await;
        } else {
            loop {
                if next(&mut subscription).await
                    == Change::SessionInvalidated("after-crash".parse().unwrap())
                {
                    std::fs::write(marker.with_extension("reconnected"), "observed after crash")
                        .unwrap();
                    break;
                }
            }
        }
    }
}
fn child(state: &Path, marker: &Path, role: &str) -> std::process::Child {
    Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "changes::tests::multiprocess_fixture",
            "--nocapture",
        ])
        .env("ARTIFACTIZE_IPC_FIXTURE_STATE", state)
        .env("ARTIFACTIZE_IPC_FIXTURE_MARKER", marker)
        .env("ARTIFACTIZE_IPC_FIXTURE_ROLE", role)
        .spawn()
        .unwrap()
}
async fn exists(path: &Path) {
    tokio::time::timeout(Duration::from_secs(6), async {
        while !path.exists() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
}
#[tokio::test]
async fn multiprocess_two_readers_short_writer_and_crashed_hub() {
    let state = temporary_state();
    let owner_marker = state.path().join("owner");
    let reader_marker = state.path().join("reader");
    let mut owner = child(state.path(), &owner_marker, "owner");
    exists(&owner_marker.with_extension("ready")).await;
    let mut reader = child(state.path(), &reader_marker, "reader");
    exists(&reader_marker.with_extension("ready")).await;
    let mut writer = child(state.path(), &state.path().join("writer"), "writer");
    assert!(writer.wait().unwrap().success());
    exists(&owner_marker).await;
    exists(&reader_marker).await;
    // No database commit accompanies hub death. The existing reader must reconnect.
    owner.kill().unwrap();
    owner.wait().unwrap();
    let mut replacement = Subscription::new(state.path()).await;
    registered(&mut replacement).await;
    tokio::time::sleep(RECONNECT + DELIVERY_TIMEOUT).await;
    Publisher::new(state.path()).notify(Change::SessionInvalidated("after-crash".parse().unwrap()));
    drain().await;
    exists(&reader_marker.with_extension("reconnected")).await;
    assert!(reader.wait().unwrap().success());
}
