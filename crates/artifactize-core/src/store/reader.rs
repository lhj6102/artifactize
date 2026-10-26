//! A persistent, read-only monitor worker. Each job ends its deferred transaction before
//! projection/rendering. It keeps only the opened Run: its definitions, which never change once
//! saved, and the views of its requests by the revisions of the rows they were read from.
use std::{
    collections::BTreeMap,
    fs::File,
    path::{Path, PathBuf},
};

use super::{
    CatalogRun, DATABASE, RequestView, Run, RunSummary, RunView,
    receipts::{Error, schema_initialized},
    requests::{self, DefinitionIndex, Query},
};
use crate::{
    platform,
    types::{RequestId, RunId},
    workspace::{canonical_target, outside_workspace},
};
use rusqlite::{Connection, OptionalExtension};

#[cfg(test)]
mod tests;

/// The SELECT statements this thread's readers ran, for tests that count what a read costs.
#[cfg(any(test, feature = "test-support"))]
mod statements {
    use std::cell::RefCell;

    thread_local! { static STATEMENTS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) }; }

    pub(super) fn trace(event: rusqlite::trace::TraceEvent<'_>) {
        let rusqlite::trace::TraceEvent::Stmt(statement, _) = event else {
            return;
        };
        let sql = statement.expanded_sql().unwrap();
        if sql.starts_with("SELECT") || sql.starts_with("WITH") {
            STATEMENTS.with(|statements| statements.borrow_mut().push(sql.to_owned()));
        }
    }

    pub(super) fn take() -> Vec<String> {
        STATEMENTS.with(|statements| std::mem::take(&mut *statements.borrow_mut()))
    }
}

struct Open {
    file: File,
    identity: platform::FileIdentity,
    connection: Connection,
    version: Option<i64>,
}
#[derive(Default)]
pub(crate) struct Reader {
    open: Option<Open>,
    cache: Cache,
}
#[derive(Default)]
pub(crate) struct Cache {
    changed: bool,
    catalog_seen: bool,
    identity: StateIdentity,
    /// The Run records the last catalog read could not decode.
    unreadable_runs: Vec<super::Unreadable>,
    selected: Option<Selected>,
}
#[derive(Default)]
enum StateIdentity {
    #[default]
    Unread,
    Missing,
    Known(crate::types::StateId),
}
/// The opened Run as last read.
struct Selected {
    /// The revision of the Run's row its state was read at.
    revision: i64,
    run: Run,
    index: DefinitionIndex,
    rows: BTreeMap<RequestId, Kept>,
    /// The cache's share in the Run's memory; last, so it goes after what it covers.
    footprint: crate::platform::Footprint,
}

/// What a request row and the rows it shows were at, so a later read decodes only what changed:
/// the request, its execution and the requests holding a Human claim on that execution.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Marks {
    request: i64,
    execution: Option<(crate::types::ExecutionId, i64)>,
    claims: Vec<(RequestId, i64)>,
}

/// A request's view, or why its records do not read, at `marks`.
struct Kept {
    marks: Marks,
    view: Option<RequestView>,
    unreadable: Vec<super::Unreadable>,
}
impl Reader {
    /// Called only on a blocking worker: permission/schema/file checks never block UI input.
    pub fn snapshot<T>(
        &mut self,
        state: &Path,
        read: impl FnOnce(&Connection, &mut Cache, &Path) -> Result<T, Error>,
    ) -> Result<T, String> {
        self.snapshot_optional(state, read)?
            .ok_or_else(|| "Run not found.".into())
    }
    pub fn snapshot_optional<T>(
        &mut self,
        state: &Path,
        read: impl FnOnce(&Connection, &mut Cache, &Path) -> Result<T, Error>,
    ) -> Result<Option<T>, String> {
        let state = canonical_target(state).map_err(|error| error.to_string())?;
        super::receipts::regular_files(&state)?;
        let file = platform::open_no_follow(File::options().read(true), &state.join(DATABASE));
        let file = match file {
            Ok(file) => file,
            Err(error) => {
                self.open = None;
                self.cache = Cache::default();
                return Err(error.to_string());
            }
        };
        let identity = platform::file_identity(&file).map_err(|error| error.to_string())?;
        let same = self.open.as_ref().is_some_and(|open| {
            open.identity == identity && platform::file_identity(&open.file).ok() == Some(identity)
        });
        if !same {
            super::receipts::check_files(&state)?;
            let connection = Connection::open_with_flags(
                state.join(DATABASE),
                rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
            )
            .map_err(|error| error.to_string())?;
            super::wait_when_busy(&connection).map_err(|error| error.to_string())?;
            let check = platform::open_no_follow(File::options().read(true), &state.join(DATABASE))
                .map_err(|error| error.to_string())?;
            if platform::file_identity(&check).map_err(|error| error.to_string())? != identity {
                self.open = None;
                self.cache = Cache::default();
                return Err("State database changed while opening it; retry the snapshot.".into());
            }
            self.cache = Cache::default();
            self.open = Some(Open {
                file,
                identity,
                connection,
                version: None,
            });
        }
        let open = self.open.as_mut().expect("opened above");
        #[cfg(any(test, feature = "test-support"))]
        open.connection.trace_v2(
            rusqlite::trace::TraceEventCodes::SQLITE_TRACE_STMT,
            Some(statements::trace),
        );
        let transaction = open
            .connection
            .transaction()
            .map_err(|error| error.to_string())?;
        if !schema_initialized(&transaction).map_err(|error| error.to_string())? {
            self.cache = Cache::default();
            return Ok(None);
        }
        // The schema read has established this transaction's snapshot. Compare data_version
        // only on this persistent connection, never with the subscription's separate probe.
        let version = transaction
            .pragma_query_value(None, "data_version", |row| row.get::<_, i64>(0))
            .map_err(|error| error.to_string())?;
        self.cache.changed = open.version != Some(version);
        let result = match read(&transaction, &mut self.cache, &state) {
            Ok(result) => result,
            Err(error) => {
                self.cache = Cache::default();
                return Err(error.to_string());
            }
        };
        if let Err(error) = transaction.commit() {
            self.cache = Cache::default();
            return Err(error.to_string());
        }
        open.version = Some(version);
        Ok(Some(result))
    }
    /// Recheck after projection too: a snapshot from a replaced pathname is obsolete even
    /// when the scope and Run selection did not change while its worker was running.
    pub fn database(&self) -> Option<platform::FileIdentity> {
        self.open.as_ref().map(|open| open.identity)
    }
    pub fn require_current(&mut self, state: &Path) -> Result<(), String> {
        let current = platform::open_no_follow(File::options().read(true), &state.join(DATABASE))
            .ok()
            .and_then(|file| platform::file_identity(&file).ok());
        let same = self.open.as_ref().is_some_and(|open| {
            current == Some(open.identity) && platform::file_identity(&open.file).ok() == current
        });
        if same {
            return Ok(());
        }
        self.reset();
        Err("State database changed while loading the snapshot; retry it.".into())
    }
    #[cfg(any(test, feature = "test-support"))]
    pub fn traced_statements(&self) -> Vec<String> {
        statements::take()
    }
    pub fn reset(&mut self) {
        self.cache = Cache::default();
    }
}
impl Cache {
    pub fn identity(&mut self, db: &Connection) -> Result<Option<crate::types::StateId>, Error> {
        match &self.identity {
            StateIdentity::Known(identity) => return Ok(Some(identity.clone())),
            StateIdentity::Missing if !self.changed => return Ok(None),
            _ => {}
        }
        let identity = match db
            .query_row("SELECT value FROM state_meta WHERE name='id'", [], |row| {
                row.get::<_, crate::types::StateId>(0)
            })
            .optional()
        {
            Ok(identity) => identity,
            Err(
                error @ (rusqlite::Error::FromSqlConversionFailure(..)
                | rusqlite::Error::InvalidColumnType(..)),
            ) => {
                // Corrupt identity metadata affects session resolution, not otherwise readable Runs.
                use std::io::Write;
                let _ = writeln!(
                    std::io::stderr().lock(),
                    "Unreadable state identity: {error}"
                );
                None
            }
            Err(error) => return Err(error.into()),
        };
        self.identity = identity
            .clone()
            .map_or(StateIdentity::Missing, StateIdentity::Known);
        Ok(identity)
    }
    pub fn changed(&self) -> bool {
        self.changed
    }
    pub fn has_catalog(&self) -> bool {
        self.catalog_seen
    }
    pub fn catalog(&mut self, db: &Connection, state: &Path) -> Result<Vec<CatalogRun>, Error> {
        let catalog = super::catalog::in_snapshot(db, state, None)?;
        self.unreadable_runs = catalog.unreadable;
        self.catalog_seen = true;
        Ok(catalog.runs)
    }
    /// The catalog rows of the Runs `ids` alone, and those of their records that do not read.
    /// A Run that no longer exists has no row.
    pub fn catalog_rows(
        &mut self,
        db: &Connection,
        state: &Path,
        ids: &[RunId],
    ) -> Result<super::catalog::Catalog, Error> {
        super::catalog::in_snapshot(db, state, Some(ids))
    }
    /// The opened Run is `id`: a later [`Cache::selected`] of it reads only what changed.
    pub fn holds(&self, id: &RunId) -> bool {
        self.selected
            .as_ref()
            .is_some_and(|selected| &selected.run.id == id)
    }
    /// A share in the opened Run's memory, for whoever keeps what [`Cache::selected`] returned.
    pub fn footprint(&self) -> Option<crate::platform::Footprint> {
        self.selected
            .as_ref()
            .map(|selected| selected.footprint.clone())
    }
    /// Let the opened Run go: nothing shows it any more.
    pub fn close(&mut self) {
        self.selected = None;
    }
    /// The Run records the last [`Cache::catalog`] read could not decode.
    pub fn unreadable_runs(&self) -> Vec<super::Unreadable> {
        self.unreadable_runs.clone()
    }
    pub fn page(
        &mut self,
        db: &Connection,
        state: &Path,
        paths: Option<&[PathBuf]>,
        limit: u32,
        offset: u32,
        pinned: Option<&RunId>,
    ) -> Result<Vec<RunSummary>, Error> {
        let paths = paths.map(|paths| {
            paths
                .iter()
                .map(|path| platform::path_text(path))
                .collect::<Vec<_>>()
        });
        let repos = paths
            .map(|paths| serde_json::to_string(&paths))
            .transpose()?;
        let mut runs = super::runs::page(db, state, repos.as_deref(), limit, offset, pinned)?;
        // A Run whose record the catalog could not decode is listed with why.
        for run in &mut runs {
            run.unreadable = self
                .unreadable_runs
                .iter()
                .filter(|record| record.id == run.id.as_str())
                .cloned()
                .collect();
        }
        Ok(runs)
    }
    /// Read the opened Run `id`. Its definitions are decoded once, when it is first opened, and
    /// a request is decoded again only when its row, its execution or its claim changed; the
    /// others are shared with the snapshots read before, which may still be shown. None
    /// means nothing of the Run changed since the last read and `force` is unset. A Run whose
    /// own record or definitions do not decode is [`Error::Unreadable`].
    pub fn selected(
        &mut self,
        db: &Connection,
        state: &Path,
        id: &RunId,
        force: bool,
    ) -> Result<Option<(RunView, Vec<RequestView>)>, Error> {
        let saved = self.selected.take().filter(|saved| &saved.run.id == id);
        // The state's text only when its revision moved, the definitions only on opening.
        let (repo, revision, data, definitions): (String, i64, Option<String>, Option<String>) = db
            .query_row(
                "SELECT r.repo,r.revision,CASE WHEN r.revision IS ?2 THEN NULL ELSE r.data END,
                CASE WHEN ?2 IS NULL THEN (SELECT d.data FROM run_definitions d WHERE d.run_id=r.id) END
                FROM runs r WHERE r.id=?1",
                rusqlite::params![id, saved.as_ref().map(|saved| saved.revision)],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .optional()?
            .ok_or_else(|| Error::Invalid("Run not found.".into()))?;
        outside_workspace(Path::new(&repo), state)
            .map_err(|error| Error::Invalid(error.to_string()))?;
        let (mut selected, state_changed) = match (saved, data) {
            (Some(saved), None) => (saved, false),
            // The Run's state changed; its definitions did not.
            (Some(mut saved), Some(data)) => {
                let definitions = saved.run.definitions.clone();
                saved.run = super::run_record::whole(id, &data, None).map_err(Error::Unreadable)?;
                saved.run.definitions = definitions;
                saved.revision = revision;
                (saved, true)
            }
            (None, data) => {
                let data = data.expect("an unopened Run reads its state");
                let run = super::run_record::whole(id, &data, definitions.as_deref())
                    .map_err(Error::Unreadable)?;
                let opened = Selected {
                    revision,
                    index: DefinitionIndex::new(&run),
                    run,
                    rows: BTreeMap::new(),
                    footprint: crate::platform::Footprint::new(),
                };
                (opened, true)
            }
        };
        let marks = marks(db, id)?;
        let stale: Vec<_> = marks
            .iter()
            .filter(|(request, marks)| {
                selected
                    .rows
                    .get(request)
                    .is_none_or(|kept| &kept.marks != marks)
            })
            .map(|(request, _)| request.clone())
            .collect();
        let changed =
            force || state_changed || !stale.is_empty() || selected.rows.len() != marks.len();
        let mut fresh: BTreeMap<_, _> = if stale.is_empty() {
            BTreeMap::new()
        } else {
            requests::rows(
                db,
                &Query {
                    run: Some(id.clone()),
                    ids: (stale.len() < marks.len()).then_some(stale),
                    ..Query::default()
                },
            )?
            .into_iter()
            .map(|row| (row.id.clone(), row))
            .collect()
        };
        let mut kept = std::mem::take(&mut selected.rows);
        selected.rows = marks
            .iter()
            .map(|(request, marks)| {
                let row = match fresh.remove(request) {
                    Some(row) => {
                        let (view, unreadable) = row.read(&mut selected.index);
                        Kept {
                            marks: marks.clone(),
                            view,
                            unreadable,
                        }
                    }
                    None => kept
                        .remove(request)
                        .expect("an unchanged request was read before"),
                };
                (request.clone(), row)
            })
            .collect();
        let result = changed.then(|| {
            let views: Vec<_> = marks
                .iter()
                .filter_map(|(request, _)| {
                    selected
                        .rows
                        .get(request)
                        .and_then(|kept| kept.view.clone())
                })
                .collect();
            let mut unreadable: Vec<_> = marks
                .iter()
                .filter_map(|(request, _)| selected.rows.get(request))
                .flat_map(|kept| kept.unreadable.iter().cloned())
                .collect();
            unreadable.sort_by(|a, b| (&a.kind, &a.id).cmp(&(&b.kind, &b.id)));
            unreadable.dedup_by(|a, b| (&a.kind, &a.id) == (&b.kind, &b.id));
            (
                RunView {
                    run: selected.run.clone(),
                    requests: views.iter().map(|view| view.request.clone()).collect(),
                    unreadable,
                },
                views,
            )
        });
        self.selected = Some(selected);
        Ok(result)
    }
}

/// The revisions of every request of the Run and of the rows each one shows, in its order.
fn marks(db: &Connection, id: &RunId) -> Result<Vec<(RequestId, Marks)>, Error> {
    // It reads indexes alone: no request's or execution's data.
    let mut statement = db.prepare_cached(
        "SELECT q.id,q.revision,e.id,e.revision,h.id,h.revision FROM requests q
        LEFT JOIN executions e ON e.id=q.execution_id
        LEFT JOIN requests h ON h.execution_id=q.execution_id AND h.claimed_by IS NOT NULL
        WHERE q.run_id=? ORDER BY q.ordinal,h.id",
    )?;
    let mut marks: Vec<(RequestId, Marks)> = Vec::new();
    let rows = statement.query_map([id], |row| {
        let execution: Option<crate::types::ExecutionId> = row.get(2)?;
        let revision: Option<i64> = row.get(3)?;
        let claim: Option<RequestId> = row.get(4)?;
        let claimed: Option<i64> = row.get(5)?;
        Ok((
            row.get::<_, RequestId>(0)?,
            row.get(1)?,
            execution.zip(revision),
            claim.zip(claimed),
        ))
    })?;
    for row in rows {
        let (request, revision, execution, claim) = row?;
        // A request whose execution holds several claims comes once per claim, in a row.
        match marks.last_mut() {
            Some((last, mark)) if *last == request => mark.claims.extend(claim),
            _ => marks.push((
                request,
                Marks {
                    request: revision,
                    execution,
                    claims: claim.into_iter().collect(),
                },
            )),
        }
    }
    Ok(marks)
}
