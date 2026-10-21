//! Which Runs a connection's writes touched, so that its change hint names them. TEMP
//! triggers note them in a TEMP table: both live only in the writing connection, and the
//! state database never holds them.
use crate::{changes::Change, types::RunId};
use rusqlite::{Connection, OptionalExtension};

/// `list` is set when a Run was created or removed: the list of Runs changed. The table has no
/// key: a writer's own conflict clause overrides any a trigger names, so rows repeat and are
/// merged when read.
const TOUCHED: &str = "CREATE TEMP TABLE IF NOT EXISTS touched(run TEXT, list INTEGER NOT NULL);
    CREATE TEMP TRIGGER IF NOT EXISTS touched_run_insert AFTER INSERT ON main.runs BEGIN INSERT INTO touched VALUES(NEW.id,1); END;
    CREATE TEMP TRIGGER IF NOT EXISTS touched_run_delete AFTER DELETE ON main.runs BEGIN INSERT INTO touched VALUES(OLD.id,1); END;
    CREATE TEMP TRIGGER IF NOT EXISTS touched_run_update AFTER UPDATE ON main.runs BEGIN INSERT INTO touched VALUES(NEW.id,0); END;
    CREATE TEMP TRIGGER IF NOT EXISTS touched_request_insert AFTER INSERT ON main.requests BEGIN INSERT INTO touched VALUES(NEW.run_id,0); END;
    CREATE TEMP TRIGGER IF NOT EXISTS touched_request_update AFTER UPDATE ON main.requests BEGIN INSERT INTO touched VALUES(NEW.run_id,0); END;
    CREATE TEMP TRIGGER IF NOT EXISTS touched_request_delete AFTER DELETE ON main.requests BEGIN INSERT INTO touched VALUES(OLD.run_id,0); END;
    CREATE TEMP TRIGGER IF NOT EXISTS touched_execution_insert AFTER INSERT ON main.executions BEGIN
        INSERT INTO touched SELECT DISTINCT run_id,0 FROM requests WHERE execution_id=NEW.id;
        INSERT INTO touched SELECT json_extract(NEW.data,'$.provenance.runId'),0 WHERE json_valid(NEW.data) AND json_extract(NEW.data,'$.provenance.runId') IS NOT NULL; END;
    CREATE TEMP TRIGGER IF NOT EXISTS touched_execution_update AFTER UPDATE ON main.executions BEGIN
        INSERT INTO touched SELECT DISTINCT run_id,0 FROM requests WHERE execution_id=NEW.id;
        INSERT INTO touched SELECT json_extract(NEW.data,'$.provenance.runId'),0 WHERE json_valid(NEW.data) AND json_extract(NEW.data,'$.provenance.runId') IS NOT NULL; END;
    CREATE TEMP TRIGGER IF NOT EXISTS touched_execution_delete AFTER DELETE ON main.executions BEGIN
        INSERT INTO touched SELECT DISTINCT run_id,0 FROM requests WHERE execution_id=OLD.id;
        INSERT INTO touched SELECT json_extract(OLD.data,'$.provenance.runId'),0 WHERE json_valid(OLD.data) AND json_extract(OLD.data,'$.provenance.runId') IS NOT NULL; END;";

/// Start noting touched Runs once the database has this schema; until then every write is
/// reported as a change of the state. Returns whether they are noted.
pub(super) fn install(db: &Connection) -> rusqlite::Result<bool> {
    let version: u32 = db.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if version != super::STATE_SCHEMA_VERSION {
        return Ok(false);
    }
    db.execute_batch(TOUCHED)?;
    Ok(true)
}

/// The hints for the writes since the last call, and forget them. A write no trigger tied to
/// a Run, a Run created or removed, or a note that cannot be read is a change of the state.
pub(super) fn take(db: &Connection) -> Vec<Change> {
    let touched = || -> rusqlite::Result<Option<Vec<RunId>>> {
        let exists = db
            .query_row(
                "SELECT 1 FROM temp.sqlite_master WHERE type='table' AND name='touched'",
                [],
                |_| Ok(()),
            )
            .optional()?;
        if exists.is_none() {
            return Ok(None);
        }
        let rows = db
            .prepare_cached("SELECT run,max(list) FROM temp.touched GROUP BY run")?
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, bool>(1)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        db.execute("DELETE FROM temp.touched", [])?;
        if rows.is_empty() || rows.iter().any(|(_, list)| *list) {
            return Ok(None);
        }
        Ok(rows
            .into_iter()
            .map(|(run, _)| run.parse().ok())
            .collect::<Option<Vec<RunId>>>())
    };
    match touched() {
        Ok(Some(runs)) => runs.into_iter().map(Change::RunInvalidated).collect(),
        Ok(None) | Err(_) => vec![Change::StateInvalidated],
    }
}
