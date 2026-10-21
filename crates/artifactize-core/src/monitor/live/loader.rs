//! One consistent read of the live branches and of the Run the Main area shows. Call
//! [`Loader::load`] on a blocking thread; it never writes. What the change hints since the last
//! read name is read again: the whole catalog of saved Runs, or the rows of the Runs named, and
//! the open Run when it is one of them. Git is asked again which worktrees exist on a rescan.
use super::{Branch, Discovery, Labels, Place, branches};
use crate::{
    changes::Changed,
    monitor::{Node, Origin, model},
    store::{self, CatalogRun, RequestView, RunView, Unreadable, reader::Reader},
    types::RunId,
};
use std::{path::PathBuf, sync::Arc};
use time::OffsetDateTime;

#[cfg(test)]
mod tests;

/// What one read needs.
#[derive(Debug, Clone, Default)]
pub struct Input {
    pub state: PathBuf,
    /// The folder the app was started for: its branch is listed even before its first Run.
    pub launch: Option<PathBuf>,
    /// The Run the Main area shows, read in full.
    pub open: Option<RunId>,
    /// Read the open Run again even when no hint named it.
    pub force: bool,
    /// Ask Git again which worktrees exist.
    pub rescan: bool,
    /// What the change hints since the last read named.
    pub changed: Changed,
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
    /// The catalog last read, kept until a hint names a change.
    facts: Option<(Arc<Vec<CatalogRun>>, Vec<Unreadable>)>,
}

impl Loader {
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
                let changed = input.changed.or_written(cache.changed());
                match (facts.as_mut(), &changed) {
                    (Some((runs, unreadable)), Changed::Runs(ids)) if cache.has_catalog() => {
                        let ids: Vec<_> = ids.iter().cloned().collect();
                        let rows = cache.catalog_rows(db, state, &ids)?;
                        *runs = Arc::new(merged(runs, &ids, rows.runs));
                        unreadable.retain(|record| !ids.iter().any(|id| record.id == id.as_str()));
                        unreadable.extend(rows.unreadable);
                    }
                    (Some(_), Changed::Nothing) if cache.has_catalog() => {}
                    _ => {
                        let runs = cache.catalog(db, state)?;
                        *facts = Some((Arc::new(runs), cache.unreadable_runs()));
                    }
                }
                let (runs, _) = facts.as_ref().expect("read above");
                let open = input
                    .open
                    .as_ref()
                    .filter(|id| runs.iter().any(|run| &run.id == *id));
                // The open Run is read when it is new here or a hint named it.
                let read = open.filter(|id| input.force || !cache.holds(id) || changed.run(id));
                let opened = match read {
                    Some(id) => match cache.selected(db, state, id, input.force) {
                        Ok(opened) => Ok(opened),
                        // The Run's definitions do not decode: it is listed with why, not opened.
                        Err(store::receipts::Error::Unreadable(record)) => Err(record),
                        Err(error) => return Err(error),
                    },
                    None => Ok(None),
                };
                Ok((cache.identity(db)?, open.is_some(), opened))
            })?;
        let Some((identity, open, opened)) = saved else {
            self.facts = None;
            return Ok(self.empty(&input, Origin::new(None, self.reader.database())));
        };
        let (opened, unopened) = match opened {
            Ok(opened) => (opened, None),
            Err(record) => (None, Some(record)),
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
        unreadable.extend(unopened);
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

/// `runs` with the rows of the Runs `ids` replaced by `rows`: a Run without a row is gone, and
/// a new one is the newest, listed last.
fn merged(runs: &[CatalogRun], ids: &[RunId], rows: Vec<CatalogRun>) -> Vec<CatalogRun> {
    let mut rows: std::collections::BTreeMap<_, _> =
        rows.into_iter().map(|run| (run.id.clone(), run)).collect();
    let mut merged: Vec<_> = runs
        .iter()
        .filter_map(|run| {
            if ids.contains(&run.id) {
                rows.remove(&run.id)
            } else {
                Some(run.clone())
            }
        })
        .collect();
    merged.extend(rows.into_values());
    merged
}
