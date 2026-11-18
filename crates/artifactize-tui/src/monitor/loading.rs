//! The Monitor's snapshot loading: what it reads, and how a finished read lands.
use super::{Focus, Input, Monitor, Page, RunSummary};
use artifactize_core::monitor::loader::{Completed, Selection};
use time::OffsetDateTime;

/// What a snapshot read must cover, from cheapest to most comprehensive.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Refresh {
    /// Reuse unchanged pages when navigation only moved within them.
    Changes,
    /// Read the whole snapshot.
    Full,
    /// Read the whole snapshot and rescan Git after hints may have been missed.
    Resync,
}

impl Monitor {
    pub async fn refresh(&mut self) {
        if let Some(job) = self.start_refresh(Refresh::Full) {
            match crate::task::joined(job.await) {
                Ok(completed) => {
                    self.finish_refresh(completed).await;
                }
                Err(error) => self.error = Some(error.to_string()),
            }
        }
    }
    pub(super) fn set_runs(&mut self, runs: Vec<RunSummary>) {
        let selected = self
            .open
            .clone()
            .or_else(|| self.selected_run().map(|run| run.id.clone()));
        let index = selected
            .and_then(|id| runs.iter().position(|run| run.id == id))
            .or(self.list.selected());
        self.runs = runs;
        self.list.select(
            index
                .map(|index| index.min(self.runs.len().saturating_sub(1)))
                .or(Some(0))
                .filter(|_| !self.runs.is_empty()),
        );
        self.open = self
            .selected_run()
            .filter(|run| run.readable())
            .map(|run| run.id.clone());
        if self.open.is_none() {
            self.run = None;
            self.footprint = None;
        }
    }

    fn selection(&self) -> Selection {
        Selection {
            state: self.state.clone(),
            page: Page {
                scope: self.scope.clone(),
                open: self.selected_run().map(|run| run.id.clone()),
                offset: self.offset,
                tail: self.page_tail,
                limit: self.limit,
            },
            revision: self.load_revision,
        }
    }
    pub(super) fn start_refresh(
        &mut self,
        mode: Refresh,
    ) -> Option<tokio::task::JoinHandle<Completed>> {
        self.start_refresh_after(mode, || {})
    }
    pub(super) fn start_refresh_after(
        &mut self,
        mode: Refresh,
        before: impl FnOnce() + Send + 'static,
    ) -> Option<tokio::task::JoinHandle<Completed>> {
        let mut loader = self.loader.take()?;
        if mode == Refresh::Resync {
            loader.rescan();
        }
        let selection = self.selection();
        let full = mode != Refresh::Changes;
        let input = Input {
            state: self.state.clone(),
            initial: self.initial.clone(),
            catalog: self.catalog.clone(),
            initialized: self.initialized,
            page: selection.page.clone(),
            full,
            displayed: self.run.as_ref().map(|(run, _)| run.run.id.clone()),
            changed: std::mem::take(&mut self.changed),
            runs: (!full && !self.runs.is_empty()).then(|| self.runs.clone()),
        };
        Some(tokio::task::spawn_blocking(move || {
            before();
            Completed::run(selection, loader, input)
        }))
    }
    /// Returns false for obsolete navigation; the driver queues a fresh job in that case.
    pub(super) async fn finish_refresh(&mut self, completed: Completed) -> bool {
        if completed.selection != self.selection() {
            // The driver reads everything again for the new selection.
            self.loader = Some(completed.loader);
            return false;
        }
        self.loader = Some(completed.loader);
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
                if snapshot.origin.replaces(&self.origin) {
                    if let Focus::Detail(pane) = &self.focus {
                        if let Some(review) = pane.content.review() {
                            review.cancel_single();
                        }
                        self.focus = Focus::Artifacts;
                    }
                    self.peek = None;
                    self.reviews.clear();
                    self.run = None;
                    self.tree = Default::default();
                    self.folds = Default::default();
                    self.framed = None;
                    self.footprint = None;
                    self.serial = self.serial.next();
                }
                self.origin = snapshot.origin;
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
                if let Some((run, requests, nodes, footprint)) = snapshot.selected {
                    let first = self
                        .run
                        .as_ref()
                        .is_none_or(|(old, _)| old.run.id != run.run.id);
                    if first {
                        self.tree = Default::default();
                    }
                    self.run = Some((run, requests));
                    self.sync_projected_tree(first, nodes);
                    self.footprint = Some(footprint);
                }
                self.refreshed = Some(now);
                let unreadable = self
                    .runs
                    .iter()
                    .flat_map(|run| &run.unreadable)
                    .map(ToString::to_string)
                    .collect::<Vec<_>>();
                self.error = (!unreadable.is_empty()).then(|| unreadable.join("; "));
                self.refresh_detail(changed, now).await;
            }
        }
        true
    }
}
