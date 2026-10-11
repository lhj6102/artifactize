//! One in-flight snapshot job, isolated from UI navigation. Generation checks discard stale
//! scopes/pages; state hints coalesce in the driver and resyncs drop the worker's caches.
use super::{Monitor, Rows, Scope, catalog, model};
use crate::{
    store::{self, RequestView, RunSummary, RunView, reader::Reader},
    types::RunId,
};
use std::{path::PathBuf, sync::Arc};
use time::OffsetDateTime;

#[derive(Debug, Clone, PartialEq, Eq)]
struct Selection {
    state: PathBuf,
    scope: Scope,
    open: Option<RunId>,
    offset: u32,
    tail: bool,
    limit: u32,
    revision: u64,
}
struct Input {
    state: PathBuf,
    initial: Option<PathBuf>,
    catalog: catalog::Catalog,
    initialized: bool,
    selection: Selection,
    full: bool,
    displayed: Option<RunId>,
    page: Option<Vec<RunSummary>>,
}
pub(super) struct Snapshot {
    identity: Option<crate::types::StateId>,
    database: Option<crate::platform::FileIdentity>,
    catalog: catalog::Catalog,
    scope: Scope,
    runs: Vec<RunSummary>,
    selected_id: Option<RunId>,
    selected: Option<(RunView, Vec<RequestView>, Rows)>,
}
pub(super) struct Completed {
    selection: Selection,
    reader: Reader,
    result: Result<Snapshot, String>,
    #[cfg(test)]
    queries: Vec<String>,
}
fn load(reader: &mut Reader, input: Input) -> Result<Snapshot, String> {
    let mut catalog = input.catalog;
    let mut scope = input.selection.scope.clone();
    // Empty state is a normal baseline. File safety checks still run on the worker.
    if !input
        .state
        .join(store::DATABASE)
        .try_exists()
        .map_err(|error| error.to_string())?
    {
        store::check_probe_files(&input.state)?;
        catalog.update(&[], input.initial.as_deref());
        if !input.initialized {
            scope = catalog.initial(input.initial.as_deref());
        }
        reader.reset();
        return Ok(Snapshot {
            identity: None,
            database: None,
            catalog,
            scope,
            runs: Vec::new(),
            selected_id: None,
            selected: None,
        });
    }
    let saved = reader.snapshot_optional(&input.state, |db, cache, state| {
        let reuse_page = input.page.is_some()
            && !input.full
            && input.initialized
            && cache.has_catalog()
            && !cache.changed();
        if input.initialized {
            catalog.selected = Some(scope.clone());
        }
        if input.full || !input.initialized || !cache.has_catalog() || cache.changed() {
            let facts = cache.catalog(db, state)?;
            if input.initialized {
                catalog.selected = Some(scope.clone());
            }
            catalog.update(&facts, input.initial.as_deref());
            if !input.initialized {
                scope = catalog.initial(input.initial.as_deref());
                catalog.selected = Some(scope.clone());
                catalog.update(&facts, input.initial.as_deref());
            }
        }
        if !input.full && input.initialized {
            catalog.rebuild();
        }
        let paths = catalog.paths(&scope);
        let runs = if reuse_page {
            input.page.clone().expect("accepted same-database page")
        } else {
            cache.page(
                db,
                state,
                paths.as_deref(),
                input.selection.limit,
                input.selection.offset,
                input.selection.open.as_ref(),
            )?
        };
        let selected = input
            .selection
            .open
            .as_ref()
            .and_then(|id| runs.iter().find(|run| &run.id == id))
            .or_else(|| {
                if input.selection.tail {
                    runs.iter().take(input.selection.limit as usize).next_back()
                } else {
                    runs.first()
                }
            });
        let selected_id = selected.map(|run| run.id.clone());
        let selected = match selected
            .filter(|run| !run.unreadable.iter().any(|record| record.kind == "run"))
        {
            Some(run) => cache.selected(
                db,
                state,
                &run.id,
                Some(run.unreadable.clone()),
                input.displayed.as_ref() != Some(&run.id),
            )?,
            None => None,
        };
        Ok((cache.identity(db)?, runs, selected_id, selected))
    })?;
    let Some((identity, runs, selected_id, selected)) = saved else {
        catalog.update(&[], input.initial.as_deref());
        if !input.initialized {
            scope = catalog.initial(input.initial.as_deref());
        }
        return Ok(Snapshot {
            identity: None,
            database: reader.database(),
            catalog,
            scope,
            runs: Vec::new(),
            selected_id: None,
            selected: None,
        });
    };
    // No read transaction survives heavy projection or live-file reads.
    let selected = selected.map(|(run, requests)| {
        let nodes = Arc::new(model::tree(&run, &requests, OffsetDateTime::now_utc()));
        (run, requests, nodes)
    });
    reader.require_current(&input.state)?;
    Ok(Snapshot {
        identity,
        database: reader.database(),
        catalog,
        scope,
        runs,
        selected_id,
        selected,
    })
}
impl Monitor {
    fn selection(&self) -> Selection {
        Selection {
            state: self.state.clone(),
            scope: self.scope.clone(),
            open: self.selected_run().map(|run| run.id.clone()),
            offset: self.offset,
            tail: self.page_tail,
            limit: self.limit,
            revision: self.load_revision,
        }
    }
    pub(super) fn start_refresh(
        &mut self,
        full: bool,
        resync: bool,
    ) -> Option<tokio::task::JoinHandle<Completed>> {
        self.start_refresh_after(full, resync, || {})
    }
    fn start_refresh_after(
        &mut self,
        full: bool,
        resync: bool,
        before: impl FnOnce() + Send + 'static,
    ) -> Option<tokio::task::JoinHandle<Completed>> {
        let mut reader = self.reader.take()?;
        if resync {
            reader.reset();
        }
        let input = Input {
            state: self.state.clone(),
            initial: self.initial.clone(),
            catalog: self.catalog.clone(),
            initialized: self.initialized,
            selection: self.selection(),
            full,
            displayed: self.run.as_ref().map(|(run, _)| run.run.id.clone()),
            page: (!full && !self.runs.is_empty()).then(|| self.runs.clone()),
        };
        Some(tokio::task::spawn_blocking(move || {
            before();
            #[cfg(test)]
            reader.traced_statements();
            let selection = input.selection.clone();
            let result = load(&mut reader, input);
            #[cfg(test)]
            let queries = reader.traced_statements();
            Completed {
                selection,
                reader,
                result,
                #[cfg(test)]
                queries,
            }
        }))
    }
    /// Returns false for obsolete navigation; the driver queues a fresh job in that case.
    pub(super) async fn finish_refresh(&mut self, mut completed: Completed) -> bool {
        if completed.selection != self.selection() {
            completed.reader.reset();
            self.reader = Some(completed.reader);
            return false;
        }
        self.reader = Some(completed.reader);
        let now = OffsetDateTime::now_utc();
        match completed.result {
            Err(error) => {
                self.error = Some(format!("read failed at {}: {error}", super::clock(now)))
            }
            Ok(snapshot) => {
                let fold = self
                    .repositories
                    .selected()
                    .and_then(|index| self.catalog.rows.get(index))
                    .and_then(|row| row.fold.clone());
                if (self.database.is_some() && self.database != snapshot.database)
                    || (self.state_id.is_some() && self.state_id != snapshot.identity)
                {
                    if let Some(review) = self.detail.as_ref().and_then(|pane| pane.review.as_ref())
                    {
                        review.cancel_single();
                    }
                    self.detail = None;
                    self.peek = None;
                    self.reviews.clear();
                    self.run = None;
                    self.tree = Default::default();
                    self.folds = Default::default();
                    self.framed = None;
                    self.serial += 1;
                }
                self.database = snapshot.database;
                self.state_id = snapshot.identity;
                self.catalog = snapshot.catalog;
                self.scope = snapshot.scope;
                self.initialized = true;
                self.repositories.select(
                    fold.and_then(|fold| {
                        self.catalog
                            .rows
                            .iter()
                            .position(|row| row.fold.as_ref() == Some(&fold))
                    })
                    .or_else(|| {
                        self.catalog
                            .rows
                            .iter()
                            .position(|row| row.fold.is_none() && row.scope == self.scope)
                    }),
                );
                self.open = snapshot.selected_id;
                self.set_runs(snapshot.runs);
                self.page_tail = false;
                let changed = snapshot.selected.is_some();
                if let Some((run, requests, nodes)) = snapshot.selected {
                    let first = self
                        .run
                        .as_ref()
                        .is_none_or(|(old, _)| old.run.id != run.run.id);
                    if first {
                        self.tree = Default::default();
                    }
                    self.run = Some((run, requests));
                    self.sync_projected_tree(first, nodes);
                }
                self.refreshed = Some(now);
                let unreadable = self
                    .runs
                    .iter()
                    .flat_map(|run| &run.unreadable)
                    .map(ToString::to_string)
                    .collect::<Vec<_>>();
                self.error = (!unreadable.is_empty()).then(|| unreadable.join("; "));
                self.refresh_detail(changed).await;
            }
        }
        true
    }
}

#[cfg(test)]
mod tests;
