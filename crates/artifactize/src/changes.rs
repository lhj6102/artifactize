//! Best-effort, state-scoped invalidation hints. SQLite and session files remain authoritative.
//! The first subscriber temporarily hosts a hub; there is no daemon and writers never elect one.
mod probe;
#[cfg(test)]
mod tests;
mod transport;

use crate::types::SessionId;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
    sync::{Arc, Mutex, OnceLock, Weak},
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    sync::{Notify, broadcast, oneshot},
    task::JoinSet,
};
use tokio_util::sync::CancellationToken;
use transport::Endpoint;

/// Reconcile missed commits and database replacement without reloading idle snapshots.
pub const RECONCILE: Duration = Duration::from_secs(5);
/// A dead hub is replaced promptly, without a spin loop or a permanent process.
const RECONNECT: Duration = Duration::from_millis(500);
/// Bound startup and short-command shutdown; notification availability cannot block correctness.
const DELIVERY_TIMEOUT: Duration = Duration::from_millis(300);
/// Bound hostile frames before allocation (IDs are at most 200 bytes).
const MAX_FRAME_BYTES: usize = 1024;
/// Bound per-process publishers, per-hub connections, broadcast backlog and session dirty sets.
const MAX_STATES: usize = 32;
const MAX_CLIENTS: usize = 64;
const MAX_BACKLOG: usize = 64;
const MAX_DIRTY_SESSIONS: usize = 64;
/// Version 1 is the first ephemeral invalidation wire format, independent of the state
/// database schema and durable session-event format. The Hello handshake rejects any
/// unsupported version instead of interpreting incompatible hints; readers reconcile safely.
const VERSION: u32 = 1;

/// Hints carry no records, credentials, scheduler decisions or remote-store events.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Change {
    StateInvalidated,
    SessionInvalidated(SessionId),
    /// Initial registration, reconnect, gap, overflow or database replacement: read afresh.
    Resync,
}

#[derive(Default)]
struct Dirty {
    resync: bool,
    state: bool,
    sessions: BTreeSet<SessionId>,
}
impl Dirty {
    fn add(&mut self, change: Change) {
        if self.resync {
            return;
        }
        match change {
            Change::Resync => {
                self.resync = true;
                self.state = false;
                self.sessions.clear();
            }
            Change::StateInvalidated => self.state = true,
            Change::SessionInvalidated(id) => {
                self.sessions.insert(id);
                if self.sessions.len() > MAX_DIRTY_SESSIONS {
                    self.add(Change::Resync);
                }
            }
        }
    }
    fn pop(&mut self) -> Option<Change> {
        if std::mem::take(&mut self.resync) {
            return Some(Change::Resync);
        }
        if std::mem::take(&mut self.state) {
            return Some(Change::StateInvalidated);
        }
        self.sessions.pop_first().map(Change::SessionInvalidated)
    }
    fn is_empty(&self) -> bool {
        !self.resync && !self.state && self.sessions.is_empty()
    }
}
struct Inbox {
    #[cfg(test)]
    registration: tokio::sync::watch::Sender<Option<String>>,
    dirty: Mutex<Dirty>,
    wake: Notify,
}
impl Default for Inbox {
    fn default() -> Self {
        Self {
            #[cfg(test)]
            registration: tokio::sync::watch::channel(None).0,
            dirty: Mutex::default(),
            wake: Notify::new(),
        }
    }
}
impl Inbox {
    fn add(&self, change: Change) {
        self.dirty.lock().unwrap().add(change);
        self.wake.notify_one();
    }
    async fn next(&self) -> Change {
        loop {
            // Register before checking: a hint arriving between check and await is retained.
            let wake = self.wake.notified();
            if let Some(change) = self.dirty.lock().unwrap().pop() {
                return change;
            }
            wake.await;
        }
    }
}

/// Register before reading the baseline. Dirties arriving during that read stay queued.
/// Dropping the subscription stops its connection, probe and any hub it owns.
pub struct Subscription {
    inbox: Arc<Inbox>,
    cancel: CancellationToken,
}
impl Subscription {
    pub async fn new(state: &Path) -> Self {
        let inbox = Arc::new(Inbox::default());
        let cancel = CancellationToken::new();
        let (ready, registered) = oneshot::channel();
        let state = state.to_path_buf();
        tokio::spawn(watch(state, inbox.clone(), cancel.clone(), ready));
        // Keep cancellation ownership even if the caller abandons registration midway.
        let subscription = Self { inbox, cancel };
        // An unavailable or unsafe endpoint degrades, never prevents a baseline read.
        let _ = registered.await;
        subscription
    }
    pub async fn next(&mut self) -> Change {
        self.inbox.next().await
    }
    /// Schedulers and non-session UIs can ignore session-only hints without reloading state.
    pub async fn next_state(&mut self) -> Change {
        loop {
            let change = self.next().await;
            if !matches!(change, Change::SessionInvalidated(_)) {
                return change;
            }
        }
    }
}
impl Drop for Subscription {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

#[derive(Default)]
struct Outbox {
    dirty: Dirty,
    submitted: u64,
    delivered: u64,
}
struct Publishing {
    endpoint: Endpoint,
    pending: Mutex<Outbox>,
    wake: Notify,
    drained: Notify,
}
/// Cloneable synchronous producer, also safe to invoke on the SQLite worker thread.
#[derive(Clone, Default)]
pub(crate) struct Publisher(Option<Arc<Publishing>>);
static PUBLISHERS: OnceLock<Mutex<BTreeMap<String, Weak<Publishing>>>> = OnceLock::new();
impl Publisher {
    pub(crate) fn new(state: &Path) -> Self {
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            return Self::default();
        };
        let Ok(endpoint) = Endpoint::new(state) else {
            return Self::default();
        };
        let mut publishers = PUBLISHERS.get_or_init(Default::default).lock().unwrap();
        publishers.retain(|_, publisher| publisher.strong_count() != 0);
        if let Some(publisher) = publishers.get(&endpoint.identity).and_then(Weak::upgrade) {
            return Self(Some(publisher));
        }
        if publishers.len() >= MAX_STATES {
            return Self::default();
        }
        let publisher = Arc::new(Publishing {
            endpoint,
            pending: Mutex::new(Outbox::default()),
            wake: Notify::new(),
            drained: Notify::new(),
        });
        publishers.insert(
            publisher.endpoint.identity.clone(),
            Arc::downgrade(&publisher),
        );
        runtime.spawn(publish(publisher.clone()));
        Self(Some(publisher))
    }
    pub(crate) fn notify(&self, change: Change) {
        if let Some(publisher) = &self.0 {
            let mut pending = publisher.pending.lock().unwrap();
            pending.dirty.add(change);
            pending.submitted = pending.submitted.wrapping_add(1);
            drop(pending);
            publisher.wake.notify_one();
        }
    }
}

/// Drain short-lived commands centrally. Only bounded hints are sent; a failed delivery is safe.
pub(crate) async fn drain() {
    let publishers: Vec<_> = PUBLISHERS
        .get()
        .map(|publishers| {
            publishers
                .lock()
                .unwrap()
                .values()
                .filter_map(Weak::upgrade)
                .collect()
        })
        .unwrap_or_default();
    let wait = async {
        for publisher in publishers {
            let target = publisher.pending.lock().unwrap().submitted;
            loop {
                let drained = publisher.drained.notified();
                if publisher.pending.lock().unwrap().delivered >= target {
                    break;
                }
                drained.await;
            }
        }
    };
    let _ = tokio::time::timeout(DELIVERY_TIMEOUT, wait).await;
}

#[derive(Serialize, Deserialize)]
enum Frame {
    Hello {
        version: u32,
        identity: String,
        subscriber: bool,
    },
    Registered {
        epoch: String,
    },
    Publish(Change),
    Delivered,
    Hint {
        epoch: String,
        sequence: u64,
        change: Change,
    },
}
async fn read_frame<R: AsyncRead + Unpin>(reader: &mut R) -> std::io::Result<Frame> {
    let bytes = reader.read_u32().await? as usize;
    if bytes == 0 || bytes > MAX_FRAME_BYTES {
        return Err(std::io::ErrorKind::InvalidData.into());
    }
    let mut data = vec![0; bytes];
    reader.read_exact(&mut data).await?;
    serde_json::from_slice(&data).map_err(std::io::Error::other)
}
async fn write_frame<W: AsyncWrite + Unpin>(writer: &mut W, frame: &Frame) -> std::io::Result<()> {
    let data = serde_json::to_vec(frame).map_err(std::io::Error::other)?;
    if data.len() > MAX_FRAME_BYTES {
        return Err(std::io::ErrorKind::InvalidData.into());
    }
    writer.write_u32(data.len() as u32).await?;
    writer.write_all(&data).await?;
    writer.flush().await
}
async fn connect(
    endpoint: &Endpoint,
    subscriber: bool,
) -> std::io::Result<(transport::Stream, String)> {
    let mut stream = endpoint.connect().await?;
    write_frame(
        &mut stream,
        &Frame::Hello {
            version: VERSION,
            identity: endpoint.identity.clone(),
            subscriber,
        },
    )
    .await?;
    match read_frame(&mut stream).await? {
        Frame::Registered { epoch } if epoch.len() <= crate::types::MAX_ID_BYTES => {
            Ok((stream, epoch))
        }
        _ => Err(std::io::ErrorKind::InvalidData.into()),
    }
}
async fn publish(publisher: Arc<Publishing>) {
    loop {
        let wake = publisher.wake.notified();
        if publisher.pending.lock().unwrap().dirty.is_empty() {
            tokio::select! {
                _ = wake => {},
                _ = tokio::time::sleep(RECONNECT) => {
                    // The task must not retain a publisher forever after its writers disappear.
                    if Arc::strong_count(&publisher) == 1 {
                        break;
                    }
                    continue;
                }
            }
        }
        let (mut dirty, target) = {
            let mut pending = publisher.pending.lock().unwrap();
            (std::mem::take(&mut pending.dirty), pending.submitted)
        };
        // No retries when no reader exists: future registration and the probe recover all state.
        let delivery = async {
            let (mut stream, _) = connect(&publisher.endpoint, false).await?;
            while let Some(change) = dirty.pop() {
                write_frame(&mut stream, &Frame::Publish(change)).await?;
                if !matches!(read_frame(&mut stream).await?, Frame::Delivered) {
                    return Err(std::io::ErrorKind::InvalidData.into());
                }
            }
            Ok::<_, std::io::Error>(())
        };
        let delivery = tokio::time::timeout(DELIVERY_TIMEOUT, delivery).await;
        eprintln!("publish result: {delivery:?}");
        publisher.pending.lock().unwrap().delivered = target;
        publisher.drained.notify_one();
    }
}

async fn watch(
    state: std::path::PathBuf,
    inbox: Arc<Inbox>,
    cancel: CancellationToken,
    ready: oneshot::Sender<()>,
) {
    let mut ready = Some(ready);
    let endpoint = Endpoint::new(&state).ok();
    let mut hub = None;
    let mut probe = probe::Probe::new(state);
    // The baseline probe below already covers startup. An immediate interval tick
    // could obscure a racing publication with a redundant Resync.
    let mut reconcile =
        tokio::time::interval_at(tokio::time::Instant::now() + RECONCILE, RECONCILE);
    reconcile.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    // Establish the cheap persistent baseline before the caller reads its snapshot.
    let _ = probe.changed().await;
    loop {
        if cancel.is_cancelled() {
            break;
        }
        if let Some(endpoint) = &endpoint {
            if hub
                .as_ref()
                .is_some_and(|task: &tokio::task::JoinHandle<()>| task.is_finished())
            {
                hub = None;
            }
            if hub.is_none()
                && let Ok(Some(owner)) = endpoint.elect()
                && let Ok(listener) = endpoint.listen()
            {
                let endpoint = endpoint.clone();
                let token = cancel.child_token();
                hub = Some(tokio::spawn(async move {
                    let result = serve(endpoint, listener, owner, token).await;
                    eprintln!("hub ended: {result:?}");
                }));
            }
            let connection = tokio::time::timeout(DELIVERY_TIMEOUT, connect(endpoint, true));
            tokio::select! {
                _ = cancel.cancelled() => break,
                result = connection => {
                    if let Ok(Ok((stream, epoch))) = result {
                        inbox.add(Change::Resync);
                        #[cfg(test)]
                        inbox.registration.send_replace(Some(epoch.clone()));
                        if let Some(ready) = ready.take() {
                            let _ = ready.send(());
                        }
                        let receive = receive(stream, epoch, inbox.clone());
                        tokio::pin!(receive);
                        loop {
                            tokio::select! {
                                _ = cancel.cancelled() => break,
                                result = &mut receive => {
                                    eprintln!("subscriber disconnected: {result:?}");
                                    #[cfg(test)]
                                    inbox.registration.send_replace(None);
                                    inbox.add(Change::Resync);
                                    break;
                                }
                                _ = reconcile.tick() => {
                                    if probe.changed().await {
                                        inbox.add(Change::Resync);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        if let Some(ready) = ready.take() {
            inbox.add(Change::Resync);
            let _ = ready.send(());
        }
        tokio::select! {
            _ = cancel.cancelled() => break,
            _ = reconcile.tick() => {
                let _ = probe.changed().await;
                // No secure transport: low-frequency authoritative reconciliation also
                // recovers session-only writes, which SQLite data_version cannot observe.
                inbox.add(Change::Resync);
            },
            _ = tokio::time::sleep(RECONNECT) => {},
        }
    }
    if let Some(task) = hub {
        task.abort();
        let _ = task.await;
    }
}
async fn receive(
    mut stream: transport::Stream,
    epoch: String,
    inbox: Arc<Inbox>,
) -> std::io::Result<()> {
    let mut previous = None;
    loop {
        match read_frame(&mut stream).await? {
            Frame::Hint {
                epoch: incoming,
                sequence,
                change,
            } if incoming == epoch => {
                if previous.is_some_and(|previous: u64| sequence != previous.wrapping_add(1)) {
                    inbox.add(Change::Resync);
                }
                previous = Some(sequence);
                inbox.add(change);
            }
            _ => return Err(std::io::ErrorKind::InvalidData.into()),
        }
    }
}

#[derive(Clone)]
struct Hint {
    sequence: u64,
    change: Change,
}
async fn serve(
    endpoint: Endpoint,
    mut listener: transport::Listener,
    _owner: std::fs::File,
    cancel: CancellationToken,
) -> std::io::Result<()> {
    let epoch = crate::agent::uuid().map_err(std::io::Error::other)?;
    let (sender, _) = broadcast::channel(MAX_BACKLOG);
    let sequence = Arc::new(Mutex::new(0_u64));
    let mut clients = JoinSet::new();
    loop {
        tokio::select! {
            _ = cancel.cancelled() => break,
            _ = clients.join_next(), if !clients.is_empty() => {},
            result = listener.accept(), if clients.len() < MAX_CLIENTS => {
                let stream = result?;
                let (identity, epoch, sender, sequence) = (
                    endpoint.identity.clone(),
                    epoch.clone(),
                    sender.clone(),
                    sequence.clone(),
                );
                clients.spawn(async move {
                    let result = client(stream, identity, epoch, sender, sequence).await;
                    eprintln!("hub client ended: {result:?}");
                });
            }
        }
    }
    clients.abort_all();
    while clients.join_next().await.is_some() {}
    Ok(())
}
async fn client(
    mut stream: transport::Stream,
    identity: String,
    epoch: String,
    sender: broadcast::Sender<Hint>,
    sequence: Arc<Mutex<u64>>,
) -> std::io::Result<()> {
    let hello = tokio::time::timeout(DELIVERY_TIMEOUT, read_frame(&mut stream)).await??;
    let Frame::Hello {
        version: VERSION,
        identity: incoming,
        subscriber,
    } = hello
    else {
        return Err(std::io::ErrorKind::InvalidData.into());
    };
    if incoming != identity {
        return Err(std::io::ErrorKind::PermissionDenied.into());
    }
    // Register before acknowledging. Broadcast retains any hint that races the baseline read.
    let mut receiver = sender.subscribe();
    tokio::time::timeout(
        DELIVERY_TIMEOUT,
        write_frame(
            &mut stream,
            &Frame::Registered {
                epoch: epoch.clone(),
            },
        ),
    )
    .await??;
    if subscriber {
        let (mut reader, mut writer) = tokio::io::split(stream);
        let outgoing = async {
            loop {
                let hint = match receiver.recv().await {
                    Ok(hint) => hint,
                    Err(broadcast::error::RecvError::Lagged(_)) => Hint {
                        sequence: *sequence.lock().unwrap(),
                        change: Change::Resync,
                    },
                    Err(_) => return Ok::<_, std::io::Error>(()),
                };
                // A slow client loses only hints, not memory or the hub's other clients.
                tokio::time::timeout(
                    DELIVERY_TIMEOUT,
                    write_frame(
                        &mut writer,
                        &Frame::Hint {
                            epoch: epoch.clone(),
                            sequence: hint.sequence,
                            change: hint.change,
                        },
                    ),
                )
                .await??;
            }
        };
        tokio::select! { result = outgoing => result, _ = reader.read_u8() => Ok(()) }
    } else {
        loop {
            let frame = tokio::time::timeout(DELIVERY_TIMEOUT, read_frame(&mut stream)).await??;
            let Frame::Publish(change) = frame else {
                return Err(std::io::ErrorKind::InvalidData.into());
            };
            {
                // Serialize sequence assignment and broadcast, including concurrent writers.
                let mut sequence = sequence.lock().unwrap();
                *sequence = sequence.wrapping_add(1);
                let _ = sender.send(Hint {
                    sequence: *sequence,
                    change,
                });
            }
            tokio::time::timeout(
                DELIVERY_TIMEOUT,
                write_frame(&mut stream, &Frame::Delivered),
            )
            .await??;
        }
    }
}
