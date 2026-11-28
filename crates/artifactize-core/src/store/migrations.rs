//! Ordered additive upgrades. The opening write transaction owns version and schema together.
use super::receipts::Error;
use rusqlite::{Connection, OpenFlags, Transaction, TransactionBehavior};
use std::path::Path;

/// Schema 6 is the first supported additive-upgrade baseline, not a converter for older layouts.
pub(super) const FIRST_VERSION: u32 = 6;
/// Private drafts and Chat metadata are added together so the experimental branch has one upgrade.
/// Chat messages use the existing Agent recorder; the active-turn owner permits crash recovery.
pub(super) const WORKSPACES: &str = "CREATE TABLE review_drafts(
    request_id TEXT NOT NULL REFERENCES requests(id) ON DELETE CASCADE,
    reviewer TEXT NOT NULL,
    data TEXT NOT NULL,
    PRIMARY KEY(request_id,reviewer)
);
CREATE TABLE chat_sessions(
    id TEXT PRIMARY KEY,
    worktree TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    data TEXT NOT NULL
);
CREATE INDEX chat_by_worktree ON chat_sessions(worktree,updated_at DESC,id DESC);
CREATE TABLE chat_active_turns(
    session_id TEXT PRIMARY KEY REFERENCES chat_sessions(id) ON DELETE CASCADE,
    owner_pid INTEGER NOT NULL,
    owner_start_time INTEGER NOT NULL,
    data TEXT NOT NULL
);";

/// Schema v12 table for single-transaction ACP permission CAS. Kept separate from `WORKSPACES`
/// so a `10 -> 11 -> 12` upgrade never attempts to create it twice.
pub(super) const CHAT_PERMISSIONS: &str = "CREATE TABLE chat_pending_permissions(
    session_id TEXT NOT NULL REFERENCES chat_sessions(id) ON DELETE CASCADE,
    owner_token TEXT NOT NULL,
    permission_id TEXT NOT NULL,
    rpc_request_id TEXT NOT NULL,
    title TEXT NOT NULL,
    tool_kind TEXT NOT NULL,
    summary TEXT NOT NULL,
    detail TEXT,
    options_json TEXT NOT NULL,
    status TEXT NOT NULL,
    selected_option_id TEXT,
    created_at TEXT NOT NULL,
    resolved_at TEXT,
    PRIMARY KEY(session_id, owner_token, permission_id)
);
CREATE INDEX chat_permissions_open ON chat_pending_permissions(session_id, status)
    WHERE status = 'PENDING';";

pub(super) fn apply(transaction: &Transaction<'_>) -> Result<(), Error> {
    let mut version: u32 =
        transaction.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if version == 0 || version == super::STATE_SCHEMA_VERSION {
        return Ok(());
    }
    if !(FIRST_VERSION..=super::STATE_SCHEMA_VERSION).contains(&version) {
        return Err(Error::Invalid(super::schema_error(version)));
    }
    while version < super::STATE_SCHEMA_VERSION {
        match version {
            FIRST_VERSION => {
                transaction.execute_batch("ALTER TABLE runs ADD COLUMN request_key TEXT;
                    CREATE UNIQUE INDEX run_request_key ON runs(repo,request_key) WHERE request_key IS NOT NULL;")?;
                version += 1;
            }
            7 => {
                transaction.execute_batch("ALTER TABLE executions ADD COLUMN profile_identity TEXT;
                    UPDATE executions SET profile_identity=json_extract(data,'$.profile') WHERE key IS NOT NULL AND json_valid(data);
                    DROP INDEX active_key;
                    CREATE UNIQUE INDEX active_key ON executions(key,profile_identity) WHERE key IS NOT NULL AND status IN ('RUNNING','WAITING_HUMAN');")?;
                version += 1;
            }
            8 => {
                // Definitions move to their own row; a record that does not read keeps its text.
                transaction.execute_batch("CREATE TABLE run_definitions(run_id TEXT PRIMARY KEY REFERENCES runs(id), data TEXT NOT NULL);
                    INSERT INTO run_definitions(run_id,data) SELECT id,json_extract(data,'$.definitions') FROM runs WHERE json_valid(data) AND json_type(data,'$.definitions')='object';
                    UPDATE runs SET data=json_remove(data,'$.definitions') WHERE id IN (SELECT run_id FROM run_definitions);
                    ALTER TABLE runs ADD COLUMN revision INTEGER NOT NULL DEFAULT 0;
                    ALTER TABLE requests ADD COLUMN revision INTEGER NOT NULL DEFAULT 0;
                    ALTER TABLE executions ADD COLUMN revision INTEGER NOT NULL DEFAULT 0;")?;
                transaction.execute_batch(super::receipts::REVISIONS)?;
                version += 1;
            }
            9 => {
                // A Run that ended with only sign-offs left is YELLOW, no longer SIGNOFFS_PENDING.
                transaction.execute_batch("UPDATE runs SET data=json_set(data,'$.status','YELLOW') WHERE status='SIGNOFFS_PENDING' AND json_valid(data);
                    UPDATE runs SET status='YELLOW' WHERE status='SIGNOFFS_PENDING';")?;
                version += 1;
            }
            10 => {
                transaction.execute_batch(WORKSPACES)?;
                version += 1;
            }
            11 => {
                transaction.execute_batch(CHAT_PERMISSIONS)?;
                version += 1;
            }
            12 => {
                // Results are saved as `{ verdict, kind, issues }` from now on; earlier rows keep
                // their untagged form, which readers decode for history only.
                version += 1;
            }
            version => return Err(Error::Invalid(super::schema_error(version))),
        }
        transaction.pragma_update(None, "user_version", version)?;
    }
    Ok(())
}

/// Existing-state readers upgrade through the same write preflight without creating a database.
pub(super) fn open_existing(state: &Path) -> Result<(), String> {
    let upgrade = || -> Result<(), Error> {
        let mut db = Connection::open_with_flags(
            state.join(super::DATABASE),
            OpenFlags::SQLITE_OPEN_READ_WRITE,
        )?;
        super::wait_when_busy(&db)?;
        let transaction = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        apply(&transaction)?;
        transaction.commit()?;
        Ok(())
    };
    upgrade().map_err(|error| format!("State upgrade requires write access: {error}"))
}
