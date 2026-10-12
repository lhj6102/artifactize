//! The Monitor's snapshot loading: what it reads, and how a finished read lands.
use super::{Input, Monitor, Page};
use artifactize_core::monitor::loader::{Completed, Selection};
use time::OffsetDateTime;

impl Monitor {
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
        full: bool,
        resync: bool,
    ) -> Option<tokio::task::JoinHandle<Completed>> {
        self.start_refresh_after(full, resync, || {})
    }
    pub(super) fn start_refresh_after(
        &mut self,
        full: bool,
        resync: bool,
        before: impl FnOnce() + Send + 'static,
    ) -> Option<tokio::task::JoinHandle<Completed>> {
        let mut loader = self.loader.take()?;
        if resync {
            loader.reset();
        }
        let selection = self.selection();
        let input = Input {
            state: self.state.clone(),
            initial: self.initial.clone(),
            catalog: self.catalog.clone(),
            initialized: self.initialized,
            page: selection.page.clone(),
            full,
            displayed: self.run.as_ref().map(|(run, _)| run.run.id.clone()),
            runs: (!full && !self.runs.is_empty()).then(|| self.runs.clone()),
        };
        Some(tokio::task::spawn_blocking(move || {
            before();
            Completed::run(selection, loader, input)
        }))
    }
    /// Returns false for obsolete navigation; the driver queues a fresh job in that case.
    pub(super) async fn finish_refresh(&mut self, mut completed: Completed) -> bool {
        if completed.selection != self.selection() {
            completed.loader.reset();
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
                    if let Some(review) =
                        self.detail.as_ref().and_then(|pane| pane.content.review())
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
