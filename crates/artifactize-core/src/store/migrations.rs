//! Ordered additive upgrades. The opening write transaction owns version and schema together.
use super::receipts::Error;
use rusqlite::{Connection, OpenFlags, Transaction, TransactionBehavior};
use std::path::Path;

/// Schema 6 is the first supported additive-upgrade baseline, not a converter for older layouts.
pub(super) const FIRST_VERSION: u32 = 6;
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
