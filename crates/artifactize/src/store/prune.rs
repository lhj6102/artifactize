use std::{
    fs,
    path::{Component, Path, PathBuf},
    time::Duration,
};

use rusqlite::OpenFlags;
use serde::Serialize;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use super::{DATABASE, STATE_SCHEMA_VERSION, receipts::check_files};
use crate::{process, workspace};

#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PruneReport {
    pub removed: Vec<PathBuf>,
    pub would_remove: Vec<PathBuf>,
    pub skipped_runs: Vec<String>,
    /// Agent sessions the size-bound collection deleted, oldest first.
    pub removed_sessions: Vec<String>,
    /// With `--dry-run`, the Agent sessions it would delete.
    pub would_remove_sessions: Vec<String>,
}

pub fn parse_duration(value: &str) -> Result<Duration, String> {
    let error = || "Use a whole-number duration with s, m, h, d or w (for example 7d).".to_owned();
    let split = value
        .find(|c: char| !c.is_ascii_digit())
        .ok_or_else(error)?;
    let multiplier = match &value[split..] {
        "s" => 1,
        "m" => 60,
        "h" => 3600,
        "d" => 86400,
        "w" => 604800,
        _ => return Err(error()),
    };
    let seconds = value[..split]
        .parse::<u64>()
        .ok()
        .and_then(|amount| amount.checked_mul(multiplier))
        .filter(|seconds| *seconds <= i64::MAX as u64)
        .ok_or_else(error)?;
    Ok(Duration::from_secs(seconds))
}

/// Prune only known scratch directories. Terminal Runs never reacquire an owner.
pub fn prune(
    state: &Path,
    repo: Option<&Path>,
    older_than: Option<Duration>,
    dry_run: bool,
) -> Result<PruneReport, String> {
    // Canonical like the repositories it is compared with: on Windows, a state path typed in
    // another case, or with 8.3 short names, would otherwise slip past `outside_workspace`.
    let state = workspace::canonical_target(&real_path(state)?).map_err(|e| e.to_string())?;
    let mut report = PruneReport::default();
    let mut repositories = Vec::new();
    if let Some(repo) = repo {
        repositories.push(workspace::canonical_target(repo).map_err(|e| e.to_string())?);
    }
    for repo in &repositories {
        workspace::outside_workspace(repo, &state).map_err(|e| e.to_string())?;
    }
    check_files(&state)?;
    // Saved Agent conversations past their size bound (limits.json agentSessions).
    let bounds = crate::limits::Limits::read(&state)?.agent_sessions();
    let sessions = crate::agent::session::collect(&state, bounds, dry_run)?;
    if dry_run {
        report.would_remove_sessions = sessions.removed;
    } else {
        report.removed_sessions = sessions.removed;
    }
    if !state
        .join(DATABASE)
        .try_exists()
        .map_err(|e| e.to_string())?
    {
        return Ok(report);
    }
    let mut db = rusqlite::Connection::open_with_flags(
        state.join(DATABASE),
        OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .map_err(|e| e.to_string())?;
    db.busy_timeout(super::SQLITE_BUSY_TIMEOUT)
        .map_err(|e| e.to_string())?;
    let transaction = db.transaction().map_err(|e| e.to_string())?;
    let version: u32 = transaction
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .map_err(|e| e.to_string())?;
    if version != STATE_SCHEMA_VERSION {
        return Err(super::schema_error(version));
    }
    let cutoff = older_than
        .map(|age| {
            time::Duration::try_from(age)
                .ok()
                .and_then(|age| OffsetDateTime::now_utc().checked_sub(age))
                .ok_or_else(|| "Prune duration is too large.".to_owned())
        })
        .transpose()?;
    let runs = {
        let mut statement = transaction
            .prepare(
                "SELECT id,repo,status,json_extract(data,'$.completedAt') FROM runs ORDER BY id",
            )
            .map_err(|e| e.to_string())?;
        statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                ))
            })
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?
    };
    let mut eligible = Vec::new();
    for (id, repo, status, completed) in runs {
        let repo = workspace::canonical_target(Path::new(&repo)).map_err(|e| e.to_string())?;
        workspace::outside_workspace(&repo, &state).map_err(|e| e.to_string())?;
        repositories.push(repo);
        let finished = completed
            .as_deref()
            .and_then(|date| OffsetDateTime::parse(date, &Rfc3339).ok());
        let requests_terminal: bool = transaction.query_row(
            "SELECT NOT EXISTS(SELECT 1 FROM requests WHERE run_id=? AND status NOT IN ('GREEN','RED','ERROR','INCOMPLETE','BLOCKED','BUDGET_EXHAUSTED'))", [&id], |row| row.get(0),
        ).map_err(|e| e.to_string())?;
        let owners = {
            let mut statement = transaction.prepare(
                "SELECT owner_pid,owner_start_time,status FROM executions WHERE json_extract(data,'$.provenance.runId')=?1 OR id IN (SELECT execution_id FROM requests WHERE run_id=?1)"
            ).map_err(|e| e.to_string())?;
            statement
                .query_map([&id], |row| {
                    Ok((
                        process::ChildIdentity {
                            pid: row.get(0)?,
                            start_time: row.get::<_, i64>(1)? as u64,
                        },
                        row.get::<_, String>(2)?,
                    ))
                })
                .map_err(|e| e.to_string())?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| e.to_string())?
        };
        let mut owned = false;
        for (owner, status) in owners {
            owned |= !matches!(status.as_str(), "GREEN" | "RED" | "ERROR")
                || process::is_alive(owner).map_err(|e| e.to_string())?;
        }
        if !segment(&id)
            || !matches!(status.as_str(), "GREEN" | "RED" | "ERROR" | "INCOMPLETE")
            || finished.is_none()
            || cutoff.is_some_and(|cutoff| finished.is_some_and(|date| date > cutoff))
            || !requests_terminal
            || owned
        {
            report.skipped_runs.push(id);
        } else {
            eligible.push(id);
        }
    }
    transaction.commit().map_err(|e| e.to_string())?;
    drop(db);

    let runs = state.join("runs");
    if !directory(&runs)? {
        return Ok(report);
    }
    let mut targets = Vec::new();
    for id in eligible {
        let path = runs.join(id);
        if directory(&path)? {
            collect(&path, &mut targets)?;
        }
    }
    // Validate the complete deletion set before removing anything; never follow links.
    for path in &targets {
        validate_tree(path, &runs, &repositories)?;
    }
    for path in targets {
        if dry_run {
            report.would_remove.push(path);
        } else {
            real_path(&path)?;
            validate_tree(&path, &runs, &repositories)?;
            fs::remove_dir_all(&path)
                .map_err(|e| format!("Cannot prune {}: {e}", path.display()))?;
            report.removed.push(path);
        }
    }
    Ok(report)
}

fn segment(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn prefixed(name: &str, prefix: &str) -> bool {
    name.strip_prefix(prefix).is_some_and(segment)
}

fn collect(parent: &Path, targets: &mut Vec<PathBuf>) -> Result<(), String> {
    let mut entries = fs::read_dir(parent)
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        let path = entry.path();
        if matches!(name, "output" | "tmp" | "home" | "cache" | "human-tools")
            || ["tool-output-", "tool-"]
                .iter()
                .any(|prefix| prefixed(name, prefix))
        {
            if directory(&path)? {
                targets.push(path);
            }
        } else if ["runtime-", "fingerprint-"]
            .iter()
            .any(|prefix| prefixed(name, prefix))
            && directory(&path)?
        {
            collect(&path, targets)?;
        }
    }
    Ok(())
}

fn directory(path: &Path) -> Result<bool, String> {
    match path.symlink_metadata() {
        Ok(metadata) if metadata.is_dir() => {
            reject_repository(path)?;
            Ok(true)
        }
        Ok(_) => Err(format!(
            "Prune requires a real directory, not a symlink or file: {}",
            path.display()
        )),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.to_string()),
    }
}

fn real_path(path: &Path) -> Result<PathBuf, String> {
    let absolute = std::path::absolute(path).map_err(|e| e.to_string())?;
    let mut current = PathBuf::new();
    for component in absolute.components() {
        if component == Component::ParentDir {
            return Err("Prune paths must not contain parent traversal.".into());
        }
        current.push(component);
        // A Windows prefix alone, such as `\\?\C:`, names the volume device, not a folder.
        if matches!(component, Component::Prefix(_) | Component::RootDir) {
            continue;
        }
        match current.symlink_metadata() {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(format!("Prune refuses symlinks: {}", current.display()));
            }
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
                return Err(error.to_string());
            }
            _ => {}
        }
    }
    Ok(current)
}

fn reject_repository(path: &Path) -> Result<(), String> {
    for marker in [".git", "artifactize.json"] {
        match path.join(marker).symlink_metadata() {
            Ok(_) => {
                return Err(format!(
                    "Prune refuses repository content: {}",
                    path.display()
                ));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.to_string()),
        }
    }
    Ok(())
}

fn validate_tree(path: &Path, runs: &Path, repositories: &[PathBuf]) -> Result<(), String> {
    if !path.starts_with(runs)
        || path == runs
        || repositories
            .iter()
            .any(|repo| path.starts_with(repo) || repo.starts_with(path))
    {
        return Err(format!(
            "Prune target is outside Run output or overlaps a repository: {}",
            path.display()
        ));
    }
    let metadata = path.symlink_metadata().map_err(|e| e.to_string())?;
    if metadata.file_type().is_symlink() {
        return Err(format!("Prune refuses symlinks: {}", path.display()));
    }
    if metadata.is_dir() {
        reject_repository(path)?;
        for entry in fs::read_dir(path).map_err(|e| e.to_string())? {
            validate_tree(
                &entry.map_err(|e| e.to_string())?.path(),
                runs,
                repositories,
            )?;
        }
    }
    Ok(())
}
