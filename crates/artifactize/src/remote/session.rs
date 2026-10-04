use std::{
    collections::{BTreeSet, HashMap},
    io::Write,
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use tokio::sync::OnceCell;

use super::Record;
use crate::{
    auth::remote::{self, Failure, Principal, Remote, Share},
    store::{Execution, Receipts, Request},
};

/// A key is looked up again at most this often, however often verify polls.
const REFRESH: Duration = Duration::from_secs(1);

/// One process's use of the remote review store.
///
/// Outages (network errors, timeouts, 5xx) fail open: one warning, then the process
/// continues without the remote. Every other failure is an error.
pub struct Session {
    remote: Remote,
    principal: OnceCell<Option<Principal>>,
    offline: AtomicBool,
    looked_up: Mutex<HashMap<(String, String), Instant>>,
}

fn warn(message: &str) {
    let _ = writeln!(std::io::stderr().lock(), "{message}");
}

impl Session {
    /// Resolve the configuration offline; `None` when no remote is configured.
    pub fn open(state: Option<&Path>, repo: Option<&Path>) -> Result<Option<Arc<Self>>, String> {
        Ok(remote::remote(state, repo)?.map(|remote| {
            Arc::new(Self {
                remote,
                principal: OnceCell::new(),
                offline: AtomicBool::new(false),
                looked_up: Mutex::default(),
            })
        }))
    }

    fn failed<T: Default>(&self, failure: Failure) -> Result<T, String> {
        if !failure.unavailable {
            return Err(failure.message);
        }
        if !self.offline.swap(true, Ordering::SeqCst) {
            warn(&format!(
                "{}; continuing without the remote review store.",
                failure.message.trim_end_matches('.')
            ));
        }
        Ok(T::default())
    }

    /// The token's principal and scopes, asked once; `None` while the remote is unavailable.
    async fn principal(&self) -> Result<Option<&Principal>, String> {
        if self.offline.load(Ordering::SeqCst) {
            return Ok(None);
        }
        let principal = self
            .principal
            .get_or_try_init(|| async {
                match self.remote.whoami().await {
                    Ok(principal) => Ok(Some(principal)),
                    Err(failure) => self.failed(failure),
                }
            })
            .await?;
        Ok(principal.as_ref())
    }

    /// Read-only: remote results for these keys as self-contained executions, not yet stored.
    pub async fn lookup(&self, keys: &[(String, String)]) -> Result<Vec<Execution>, String> {
        if keys.is_empty() {
            return Ok(Vec::new());
        }
        let Some(principal) = self.principal().await? else {
            return Ok(Vec::new());
        };
        if !has_scope(principal, "read") {
            return Ok(Vec::new());
        }
        let entries = match self.remote.lookup(keys).await {
            Ok(entries) => entries,
            Err(failure) => return self.failed(failure),
        };
        let requested: BTreeSet<_> = keys
            .iter()
            .map(|(fingerprint, hash)| (fingerprint.as_str(), hash.as_str()))
            .collect();
        let mut executions = Vec::new();
        for entry in entries {
            // A record that does not validate is skipped; reviewing locally is always correct.
            let mirrored = serde_json::from_value::<Record>(entry)
                .map_err(|e| e.to_string())
                .and_then(|record| {
                    if !requested.contains(&(&*record.fingerprint, &*record.eval_def_hash)) {
                        return Err("the store returned an unrequested key".into());
                    }
                    record.mirror(self.remote.url.as_str())
                });
            match mirrored {
                Ok(execution) => executions.push(execution),
                Err(error) => warn(&format!(
                    "Ignoring an invalid remote review record: {error}"
                )),
            }
        }
        Ok(executions)
    }

    /// Mirror remote results for keys without a local entry into the local cache, where the
    /// normal reuse path finds them. Each key is looked up at most once per second.
    pub async fn refresh(
        &self,
        receipts: &Receipts,
        keys: Vec<(String, String)>,
    ) -> Result<(), String> {
        let now = Instant::now();
        let keys: Vec<_> = {
            let mut looked_up = self.looked_up.lock().expect("lookup times");
            keys.into_iter()
                .filter(|key| {
                    let due = looked_up
                        .get(key)
                        .is_none_or(|at| now.duration_since(*at) >= REFRESH);
                    if due {
                        looked_up.insert(key.clone(), now);
                    }
                    due
                })
                .collect()
        };
        if keys.is_empty() || self.offline.load(Ordering::SeqCst) {
            return Ok(());
        }
        let missing = receipts.uncached(keys).await?;
        for execution in self.lookup(&missing).await? {
            receipts.mirror_execution(&execution).await?;
        }
        Ok(())
    }

    /// Publish a request's result when its own settle just published the local cache entry:
    /// a GREEN/RED with a fingerprint, never a mirror, an error or a forced review.
    pub async fn publish_request(
        &self,
        receipts: &Receipts,
        request: &Request,
    ) -> Result<(), String> {
        let (Some(fingerprint), Some(execution_id)) = (&request.fingerprint, &request.execution_id)
        else {
            return Ok(());
        };
        match receipts
            .published_execution(fingerprint, &request.eval_def_hash, execution_id)
            .await?
        {
            Some(execution) => self.publish(&execution).await,
            None => Ok(()),
        }
    }

    /// Publish a result whose local settle published its cache entry. Nothing is sent while
    /// the remote is unavailable or without the publish scope; Human results need `human`.
    async fn publish(&self, execution: &Execution) -> Result<(), String> {
        let Some(principal) = self.principal().await? else {
            return Ok(());
        };
        if !has_scope(principal, "publish") {
            return Ok(());
        }
        let record = match record(execution, principal, self.remote.share) {
            Ok(record) => record,
            Err(reason) => {
                warn(&format!("This result stays local: {reason}."));
                return Ok(());
            }
        };
        match self.remote.publish(&record).await {
            Ok(_) => Ok(()),
            Err(failure) => self.failed(failure),
        }
    }
}

pub(super) fn has_scope(principal: &Principal, name: &str) -> bool {
    principal.scopes.iter().any(|scope| scope == name)
}

/// The record a publishing token may send for a local result, or why it stays local.
pub(super) fn record(
    execution: &Execution,
    principal: &Principal,
    share: Share,
) -> Result<Record, String> {
    if execution.profile["kind"] == "human" && !has_scope(principal, "human") {
        return Err(format!(
            "remote token {} lacks the human scope",
            principal.principal
        ));
    }
    let record = Record::new(execution, share == Share::Full)?;
    // `.` and `..` cannot be a URL path segment.
    if matches!(record.fingerprint.as_str(), "." | "..") {
        return Err(format!(
            "fingerprint {} cannot be a URL path segment",
            record.fingerprint
        ));
    }
    Ok(record)
}
