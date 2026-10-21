//! A saved Run is two rows. `runs.data` holds its changing state, and `run_definitions` holds
//! its definitions, which are fixed for the Run's life and can be hundreds of megabytes for a
//! catalog of Artifacts. The definitions are written once, with the Run; saving the Run again
//! writes only its state, and only a reader that opens the Run decodes its definitions.
use super::{Run, Unreadable, definitions::Definitions, receipts::Error, unreadable::decode};
use crate::types::RunId;
use rusqlite::{OptionalExtension, params};

/// The Run's state as `runs.data` keeps it: the whole Run but its definitions.
pub(super) fn state_text(run: &Run) -> Result<String, serde_json::Error> {
    let mut state = run.clone();
    state.definitions = Definitions::default();
    serde_json::to_string(&state)
}

/// Save a new Run: its state row and, when it has them, its definitions.
pub(super) fn insert(db: &rusqlite::Connection, run: &Run) -> Result<(), Error> {
    db.execute(
        "INSERT INTO runs(id,repo,status,data,request_key) VALUES (?,?,?,?,?)",
        params![
            run.id,
            crate::platform::path_text(&run.repo_path),
            run.status(),
            state_text(run)?,
            run.request_key
        ],
    )?;
    if run.definitions.graph().is_some() {
        db.execute(
            "INSERT INTO run_definitions(run_id,data) VALUES (?,?)",
            params![run.id, serde_json::to_string(&run.definitions)?],
        )?;
    }
    Ok(())
}

/// Save the Run's changed state. Its definitions were written with the Run and stay.
pub(super) fn update(db: &rusqlite::Connection, run: &Run) -> Result<(), Error> {
    db.execute(
        "UPDATE runs SET status=?,data=? WHERE id=?",
        params![run.status(), state_text(run)?, run.id],
    )?;
    Ok(())
}

/// Decode a Run from its saved state and definitions, or none when it was saved without any.
pub(super) fn whole(id: &str, state: &str, definitions: Option<&str>) -> Result<Run, Unreadable> {
    let mut run: Run = decode("run", id, state)?;
    if let Some(definitions) = definitions {
        run.definitions = decode("run", id, definitions)?;
    }
    Ok(run)
}

/// The Run's definitions, or none when it was saved without any.
pub(super) fn definitions(
    db: &rusqlite::Connection,
    id: &RunId,
) -> Result<Result<Definitions, Unreadable>, Error> {
    let data: Option<String> = db
        .query_row(
            "SELECT data FROM run_definitions WHERE run_id=?",
            [id],
            |row| row.get(0),
        )
        .optional()?;
    Ok(data.map_or_else(
        || Ok(Definitions::default()),
        |data| decode("run", id, &data),
    ))
}

/// The whole Run: its state and its definitions.
pub(super) fn load(db: &rusqlite::Connection, id: &RunId) -> Result<Run, Error> {
    let (state, definitions): (String, Option<String>) = db.query_row(
        "SELECT r.data,d.data FROM runs r LEFT JOIN run_definitions d ON d.run_id=r.id WHERE r.id=?",
        [id],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    whole(id, &state, definitions.as_deref())
        .map_err(|unreadable| Error::Invalid(unreadable.to_string()))
}

#[cfg(test)]
mod tests;
