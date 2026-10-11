//! A persistent, read-only monitor worker. Each job ends its deferred transaction before
//! projection/rendering. Caches compare saved rows, not hints, and retain only one selected Run
//! and the bounded visible page's audit. Unscoped hints still require a linear reconciliation.
use std::{
    collections::BTreeMap,
    fs::File,
    path::{Path, PathBuf},
};

use super::{
    CatalogRun, DATABASE, RequestView, Run, RunSummary, RunView,
    receipts::{Error, schema_initialized},
    requests::{self, DefinitionIndex, Query, Row},
    unreadable::{self, Audit},
};
use crate::{
    platform,
    types::{RequestId, RunId},
    workspace::{canonical_target, outside_workspace},
};
use rusqlite::{Connection, OptionalExtension};

#[cfg(test)]
mod tests;

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
    audit: Audit,
    facts: super::catalog::Facts,
    selected: Option<Selected>,
    #[cfg(test)]
    pub decoded: usize,
}
#[derive(Default)]
enum StateIdentity {
    #[default]
    Unread,
    Missing,
    Known(crate::types::StateId),
}
struct Selected {
    data: String,
    run: Run,
    index: DefinitionIndex,
    rows: BTreeMap<RequestId, (Row, Option<RequestView>)>,
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
            connection
                .busy_timeout(super::SQLITE_BUSY_TIMEOUT)
                .map_err(|error| error.to_string())?;
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
        #[cfg(test)]
        open.connection.trace_v2(
            rusqlite::trace::TraceEventCodes::SQLITE_TRACE_STMT,
            Some(tests::trace),
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
    #[cfg(test)]
    pub(crate) fn traced_statements(&self) -> Vec<String> {
        tests::statements()
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
        let catalog = super::catalog::in_snapshot_cached(db, state, &mut self.facts)?;
        self.catalog_seen = true;
        Ok(catalog)
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
        super::runs::in_snapshot_cached(
            db,
            state,
            repos.as_deref(),
            limit,
            offset,
            pinned,
            &mut self.audit,
        )
    }
    /// None means byte-identical selected rows and audit: retain the last-good tree/details.
    pub fn selected(
        &mut self,
        db: &Connection,
        state: &Path,
        id: &RunId,
        audit: Option<Vec<super::Unreadable>>,
        force: bool,
    ) -> Result<Option<(RunView, Vec<RequestView>)>, Error> {
        let (repo, data): (String, String) = db
            .query_row("SELECT repo,data FROM runs WHERE id=?", [id], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })
            .optional()?
            .ok_or_else(|| Error::Invalid("Run not found.".into()))?;
        outside_workspace(Path::new(&repo), state)
            .map_err(|error| Error::Invalid(error.to_string()))?;
        let same_run = self
            .selected
            .as_ref()
            .is_some_and(|saved| saved.data == data && &saved.run.id == id);
        let mut selected = match self.selected.take() {
            Some(saved) if same_run => saved,
            saved => {
                let run: Run = match self.audit.take_run(id, &data) {
                    Some(run) => run,
                    None => unreadable::decode("run", id, &data)
                        .map_err(|error| Error::Invalid(error.to_string()))?,
                };
                match saved.filter(|saved| &saved.run.id == id) {
                    Some(mut saved)
                        if serde_json::to_string(&saved.run.definitions)?
                            == serde_json::to_string(&run.definitions)? =>
                    {
                        saved.data = data;
                        saved.run = run;
                        saved
                    }
                    _ => {
                        let index = DefinitionIndex::new(&run);
                        Selected {
                            data,
                            run,
                            index,
                            rows: BTreeMap::new(),
                            unreadable: Vec::new(),
                        }
                    }
                }
            }
        };
        let rows = requests::rows(
            db,
            &Query {
                run: Some(id.clone()),
                ..Query::default()
            },
        )?;
        let audit = match audit {
            Some(audit) => audit,
            None => unreadable::in_run(db, id)?,
        };
        let mut changed = force
            || !same_run
            || selected.rows.len() != rows.len()
            || serde_json::to_string(&selected.unreadable)? != serde_json::to_string(&audit)?;
        let mut retained = BTreeMap::new();
        let mut order = Vec::with_capacity(rows.len());
        for row in rows {
            let saved = selected.rows.remove(&row.id);
            let view = match saved {
                Some((old, view)) if old == row => view,
                _ => {
                    changed = true;
                    #[cfg(test)]
                    {
                        self.decoded += 1;
                    }
                    row.view_cached(&selected.index, &mut self.audit)
                }
            };
            order.push(row.id.clone());
            retained.insert(row.id.clone(), (row, view));
        }
        selected.rows = retained;
        selected.unreadable = audit;
        let result = changed.then(|| {
            let views: Vec<_> = order
                .iter()
                .filter_map(|id| selected.rows.get(id).and_then(|(_, view)| view.clone()))
                .collect();
            (
                RunView {
                    run: selected.run.clone(),
                    requests: views.iter().map(|view| view.request.clone()).collect(),
                    unreadable: selected.unreadable.clone(),
                },
                views,
            )
        });
        self.selected = Some(selected);
        Ok(result)
    }
}
