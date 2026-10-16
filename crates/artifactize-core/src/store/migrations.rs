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
        db.busy_timeout(super::SQLITE_BUSY_TIMEOUT)?;
        let transaction = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        apply(&transaction)?;
        transaction.commit()?;
        Ok(())
    };
    upgrade().map_err(|error| format!("State upgrade requires write access: {error}"))
}
