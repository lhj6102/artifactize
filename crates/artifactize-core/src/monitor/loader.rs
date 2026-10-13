//! One in-flight snapshot job, isolated from UI navigation. Generation checks discard stale
//! scopes/pages; state hints coalesce in the driver and resyncs drop the worker's caches.
//!
//! [`Loader`] is the UI-agnostic seam: the terminal monitor and the desktop app both read
//! their catalog, Runs page and selected Run through it.
use super::{Node, Scope, catalog, model};
use crate::{
    store::{self, RequestView, RunSummary, RunView, reader::Reader},
    types::RunId,
};
use std::{path::PathBuf, sync::Arc};
use time::OffsetDateTime;

/// Which Runs page of which scope a snapshot reads, and the Run it opens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Page {
    pub scope: Scope,
    /// The selected Run; `None` opens the first (or, with `tail`, the last) Run of the page.
    pub open: Option<RunId>,
    pub offset: u32,
    pub tail: bool,
    pub limit: u32,
}

/// Where the page of older Runs starts, when the page shown (`shown` Runs of `limit`) is full.
pub fn older_offset(offset: u32, limit: u32, shown: u32) -> Option<u32> {
    (shown >= limit).then(|| offset.saturating_add(limit))
}

/// Where the page of newer Runs starts, when this one is not the first.
pub fn newer_offset(offset: u32, limit: u32) -> Option<u32> {
    (offset > 0).then(|| offset.saturating_sub(limit))
}

/// The monitor's page plus what tells an obsolete job from a current one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selection {
    pub state: PathBuf,
    pub page: Page,
    pub revision: u64,
}

/// Everything one snapshot read needs; the caller keeps the catalog between reads.
pub struct Input {
    pub state: PathBuf,
    /// cwd, `--repo` or `--all` (`None`): only the first selection.
    pub initial: Option<PathBuf>,
    pub catalog: catalog::Catalog,
    /// False before the first accepted snapshot, which picks the initial scope.
    pub initialized: bool,
    pub page: Page,
    /// Read the catalog afresh instead of reusing unchanged cached facts.
    pub full: bool,
    /// The Run the caller already shows; the reader decodes it again only when it changed.
    pub displayed: Option<RunId>,
    /// The page the caller already shows, reused while the database is unchanged.
    pub runs: Option<Vec<RunSummary>>,
}

/// Where a snapshot was read from: the state identity and the database file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Origin {
    identity: Option<crate::types::StateId>,
    database: Option<crate::platform::FileIdentity>,
}
impl Origin {
    /// The state was replaced since `previous` was read: Details, drafts and sessions
    /// of the old state no longer apply.
    pub fn replaces(&self, previous: &Self) -> bool {
        previous.database.is_some() && previous.database != self.database
            || previous.identity.is_some() && previous.identity != self.identity
    }
    pub fn identity(&self) -> Option<&crate::types::StateId> {
        self.identity.as_ref()
    }
    /// The state's origin as it stands, read without a snapshot: its database file, then its
    /// identity. A UI that keeps work outside snapshots (open reviews, drafts, claims taken)
    /// compares it with [`Origin::replaces`] as the monitor compares snapshots.
    pub fn read(state: &std::path::Path) -> Result<Self, String> {
        let path = state.join(store::DATABASE);
        let database =
            match crate::platform::open_no_follow(std::fs::File::options().read(true), &path) {
                Ok(file) => Some(crate::platform::file_identity(&file).map_err(|e| e.to_string())?),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                Err(error) => return Err(error.to_string()),
            };
        Ok(Self {
            identity: store::read_state_id(state)?,
            database,
        })
    }
}

/// One consistent read: the catalog, the scope's Runs page and the selected Run with its tree.
pub struct Snapshot {
    pub origin: Origin,
    pub catalog: catalog::Catalog,
    pub scope: Scope,
    pub runs: Vec<RunSummary>,
    pub selected_id: Option<RunId>,
    /// The selected Run when it was read again: its requests and projected tree.
    pub selected: Option<(RunView, Vec<RequestView>, Arc<Vec<Node>>)>,
}

/// A persistent, read-only snapshot worker with its caches. Call [`Loader::load`] on a
/// blocking thread; it never writes.
#[derive(Default)]
pub struct Loader {
    reader: Reader,
}
impl Loader {
    /// The SELECT statements read since the last call, for tests that count what a read costs.
    #[cfg(any(test, feature = "test-support"))]
    pub fn traced_statements(&self) -> Vec<String> {
        self.reader.traced_statements()
    }

    pub fn load(&mut self, input: Input) -> Result<Snapshot, String> {
        load(&mut self.reader, input)
    }
    /// Drop every cache, so the next load reads the whole state afresh.
    pub fn reset(&mut self) {
        self.reader.reset();
    }
}

pub struct Completed {
    pub selection: Selection,
    pub loader: Loader,
    pub result: Result<Snapshot, String>,
    /// The SELECT statements the read ran.
    #[cfg(any(test, feature = "test-support"))]
    pub queries: Vec<String>,
}
impl Completed {
    /// Read the snapshot of `input` with `loader`, keeping what the caller needs to apply it.
    pub fn run(selection: Selection, mut loader: Loader, input: Input) -> Self {
        #[cfg(any(test, feature = "test-support"))]
        loader.traced_statements();
        let result = loader.load(input);
        #[cfg(any(test, feature = "test-support"))]
        let queries = loader.traced_statements();
        Self {
            selection,
            loader,
            result,
            #[cfg(any(test, feature = "test-support"))]
            queries,
        }
    }
}

fn load(reader: &mut Reader, input: Input) -> Result<Snapshot, String> {
    let mut catalog = input.catalog;
    let mut scope = input.page.scope.clone();
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
            origin: Origin::default(),
            catalog,
            scope,
            runs: Vec::new(),
            selected_id: None,
            selected: None,
        });
    }
    let saved = reader.snapshot_optional(&input.state, |db, cache, state| {
        let reuse_page = input.runs.is_some()
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
            input.runs.clone().expect("accepted same-database page")
        } else {
            cache.page(
                db,
                state,
                paths.as_deref(),
                input.page.limit,
                input.page.offset,
                input.page.open.as_ref(),
            )?
        };
        let selected = input
            .page
            .open
            .as_ref()
            .and_then(|id| runs.iter().find(|run| &run.id == id))
            .or_else(|| {
                if input.page.tail {
                    runs.iter().take(input.page.limit as usize).next_back()
                } else {
                    runs.first()
                }
            });
        let selected_id = selected.map(|run| run.id.clone());
        let selected = match selected.filter(|run| run.readable()) {
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
            origin: Origin {
                identity: None,
                database: reader.database(),
            },
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
        origin: Origin {
            identity,
            database: reader.database(),
        },
        catalog,
        scope,
        runs,
        selected_id,
        selected,
    })
}
