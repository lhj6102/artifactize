//! One consistent read of the live branches and of the Run the Main area shows. Call
//! [`Loader::load`] on a blocking thread; it never writes. The catalog of saved Runs is read
//! again only when the database changed, and Git is asked again which worktrees exist on a
//! rescan.
use super::{Branch, Discovery, Labels, Place, branches};
use crate::{
    monitor::{Node, Origin, model},
    store::{self, CatalogRun, RequestView, RunView, Unreadable, reader::Reader},
    types::RunId,
};
use std::{path::PathBuf, sync::Arc};
use time::OffsetDateTime;

/// What one read needs.
#[derive(Debug, Clone, Default)]
pub struct Input {
    pub state: PathBuf,
    /// The folder the app was started for: its branch is listed even before its first Run.
    pub launch: Option<PathBuf>,
    /// The Run the Main area shows, read in full.
    pub open: Option<RunId>,
    /// Read the open Run again even when its records did not change.
    pub force: bool,
    /// Ask Git again which worktrees exist.
    pub rescan: bool,
}

/// The open Run: its view, its requests and their projected tree.
pub type Opened = (RunView, Vec<RequestView>, Arc<Vec<Node>>);

/// One consistent read.
pub struct Snapshot {
    pub origin: Origin,
    /// The live branches, the most recent Run first.
    pub branches: Vec<Branch>,
    /// The project and branch of every readable saved Run.
    pub labels: Labels,
    /// The branch of the folder the app was started for, when it lives.
    pub launch: Option<Place>,
    /// Saved Run records that cannot be read, and records of the open Run that cannot.
    pub unreadable: Vec<Unreadable>,
    /// The open Run when it was read again, `None` when its records did not change since the
    /// last read (or no Run is open, or it is gone: see [`Snapshot::open`]).
    pub opened: Option<Opened>,
    /// The open Run still exists and reads.
    pub open: bool,
}

/// A persistent read-only worker with its caches.
#[derive(Default)]
pub struct Loader {
    reader: Reader,
    discovery: Discovery,
    /// The catalog last read, kept while the database is unchanged.
    facts: Option<(Arc<Vec<CatalogRun>>, Vec<Unreadable>)>,
}

impl Loader {
    /// Drop every cache: the next load reads the whole state and asks Git afresh.
    pub fn reset(&mut self) {
        self.reader.reset();
        self.discovery = Discovery::default();
        self.facts = None;
    }

    pub fn load(&mut self, input: Input) -> Result<Snapshot, String> {
        if input.rescan {
            self.discovery.rescan();
        }
        let database = input.state.join(store::DATABASE);
        if !database.try_exists().map_err(|error| error.to_string())? {
            // Empty state is a normal baseline; its file safety checks still run here.
            store::check_probe_files(&input.state)?;
            self.reader.reset();
            self.facts = None;
            return Ok(self.empty(&input, Origin::default()));
        }
        let facts = &mut self.facts;
        let saved = self
            .reader
            .snapshot_optional(&input.state, |db, cache, state| {
                if facts.is_none() || cache.changed() || !cache.has_catalog() {
                    let runs = cache.catalog(db, state)?;
                    *facts = Some((Arc::new(runs), cache.unreadable_runs()));
                }
                let (runs, _) = facts.as_ref().expect("read above");
                let open = input
                    .open
                    .as_ref()
                    .filter(|id| runs.iter().any(|run| &run.id == *id));
                let opened = match open {
                    Some(id) => cache.selected(db, state, id, None, input.force)?,
                    None => None,
                };
                Ok((cache.identity(db)?, open.is_some(), opened))
            })?;
        let Some((identity, open, opened)) = saved else {
            self.facts = None;
            return Ok(self.empty(&input, Origin::new(None, self.reader.database())));
        };
        // No read transaction survives heavy projection or Git discovery.
        let opened = opened.map(|(run, requests)| {
            let nodes = Arc::new(model::tree(&run, &requests, OffsetDateTime::now_utc()));
            (run, requests, nodes)
        });
        let (runs, mut unreadable) = self.facts.clone().expect("read above");
        if let Some((run, _, _)) = &opened {
            unreadable.extend(run.unreadable.iter().cloned());
        }
        let listing = branches(&runs, input.launch.as_deref(), &mut self.discovery);
        self.reader.require_current(&input.state)?;
        Ok(Snapshot {
            origin: Origin::new(identity, self.reader.database()),
            branches: listing.branches,
            labels: listing.labels,
            launch: listing.launch,
            unreadable,
            opened,
            open,
        })
    }

    fn empty(&mut self, input: &Input, origin: Origin) -> Snapshot {
        let listing = branches(&[], input.launch.as_deref(), &mut self.discovery);
        Snapshot {
            origin,
            branches: listing.branches,
            labels: listing.labels,
            launch: listing.launch,
            unreadable: Vec::new(),
            opened: None,
            open: false,
        }
    }
}
