use std::{collections::BTreeSet, io::Write, path::Path};

use serde::Serialize;

use super::session::{has_scope, record};
use crate::{auth::remote, store::cache_entries};

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Push {
    pub dry_run: bool,
    /// Sent and created, or with `--dry-run` not yet in the store.
    pub pushed: usize,
    pub existing: usize,
    /// Kept local: Human sign-offs without the human scope, or records over their limit.
    pub skipped: usize,
}

/// Re-send local GREEN/RED results, never mirrors; keys the store already has are no-ops.
/// Unlike verify, this explicit command fails on any remote failure.
pub async fn push(
    state: Option<&Path>,
    repo: Option<&Path>,
    dry_run: bool,
) -> Result<Push, String> {
    let remote = remote::remote(state, repo)?.ok_or(
        "No remote review store is configured; run `artifactize remote login URL` or set ARTIFACTIZE_REMOTE.",
    )?;
    let principal = remote.whoami().await.map_err(|failure| failure.message)?;
    if !has_scope(&principal, "publish") {
        return Err(format!(
            "Remote token {} lacks the publish scope.",
            principal.principal
        ));
    }
    let state = crate::store::state_dir(state)?;
    let mut report = Push {
        dry_run,
        ..Push::default()
    };
    let mut after = None;
    loop {
        let page = cache_entries::local(&state, after.take(), 100).await?;
        let Some(last) = page.last() else {
            break;
        };
        after = Some((
            last.stale_key.clone().expect("cached stale key"),
            last.eval_def_hash.clone(),
        ));
        let mut records = Vec::new();
        for execution in &page {
            match record(execution, &principal, remote.share) {
                Ok(record) => records.push(record),
                Err(reason) => {
                    report.skipped += 1;
                    let _ = writeln!(
                        std::io::stderr().lock(),
                        "Skipped {} {}: {reason}.",
                        execution.stale_key.as_deref().unwrap_or_default(),
                        execution.eval_def_hash
                    );
                }
            }
        }
        // With the read scope, keys the store already has are not sent again.
        let existing: BTreeSet<_> = if has_scope(&principal, "read") {
            let keys: Vec<_> = records
                .iter()
                .map(|record| (record.stale_key.clone(), record.eval_def_hash.clone()))
                .collect();
            remote
                .lookup(&keys)
                .await
                .map_err(|failure| failure.message)?
                .into_iter()
                .filter_map(|entry| {
                    Some((
                        entry["staleKey"].as_str()?.to_owned(),
                        entry["evalDefHash"].as_str()?.to_owned(),
                    ))
                })
                .collect()
        } else {
            BTreeSet::new()
        };
        for record in records {
            // A key published meanwhile answers `created: false`; the first writer wins.
            let created = !existing
                .contains(&(record.stale_key.clone(), record.eval_def_hash.clone()))
                && (dry_run
                    || remote
                        .publish(&record)
                        .await
                        .map_err(|failure| failure.message)?);
            if created {
                report.pushed += 1;
            } else {
                report.existing += 1;
            }
        }
    }
    Ok(report)
}
