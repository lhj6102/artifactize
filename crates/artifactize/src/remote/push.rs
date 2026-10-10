use std::{collections::BTreeSet, io::Write, path::Path};

use serde::Serialize;

use super::session::{has_scope, record};
use crate::{auth::remote, store::history};

/// Bound retained local execution payloads per push batch while paging the whole key history.
const PAGE_ENTRIES: usize = 100;

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

/// Send the latest local GREEN/RED record of each key, never a mirror; records the store
/// already holds are no-ops. Unlike verify, this explicit command fails on any remote failure.
pub async fn push(
    state: Option<&Path>,
    repo: Option<&Path>,
    dry_run: bool,
) -> Result<Push, String> {
    let remote = remote::remote(state, repo)?.ok_or(
        "No remote review store is configured; run `artifactize remote login URL` or set ARTIFACTIZE_REMOTE.",
    )?;
    let principal = remote.whoami().await.map_err(|failure| failure.message)?;
    if !has_scope(&principal, &crate::auth::remote::Scope::Publish) {
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
        let page = history::local(&state, after.take(), PAGE_ENTRIES).await?;
        let Some(last) = page.last() else {
            break;
        };
        after = Some(last.key.clone().expect("cached key"));
        let mut records = Vec::new();
        for execution in &page {
            match record(execution, &principal, remote.share) {
                Ok(record) => records.push(record),
                Err(reason) => {
                    report.skipped += 1;
                    let _ = writeln!(
                        std::io::stderr().lock(),
                        "Skipped {} ({}): {reason}.",
                        execution.key.as_deref().unwrap_or_default(),
                        execution.provenance.eval_id
                    );
                }
            }
        }
        // With the read scope, a record that is already the store's latest is not sent again.
        let existing: BTreeSet<_> = if has_scope(&principal, &crate::auth::remote::Scope::Read) {
            let keys: Vec<_> = records.iter().map(|record| record.key.clone()).collect();
            remote
                .lookup(&keys)
                .await
                .map_err(|failure| failure.message)?
                .into_iter()
                .filter_map(Result::ok)
                .map(|entry| (entry.key.to_string(), entry.execution_id.to_string()))
                .collect()
        } else {
            BTreeSet::new()
        };
        for record in records {
            // The store keeps each execution once; a resent record answers `created: false`.
            let created = !existing
                .contains(&(record.key.to_string(), record.execution_id.to_string()))
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
