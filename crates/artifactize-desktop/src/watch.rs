//! Live updates: the state's invalidation hub, as the terminal monitor subscribes to it, and
//! a rescan of which worktrees exist, which no state change reports. Hints only say what to
//! read again: the next load reads what they named, and the frontend reads through the
//! commands.
use crate::App;
use artifactize_core::changes::{Change, Subscription};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};
use tauri::Manager;
use tauri_specta::Event;

/// How often Git is asked which worktrees exist: a worktree added or removed in another
/// process reaches the Pane within this.
const RESCAN: Duration = Duration::from_secs(2);

/// The state changed, or (`resync`) hints may have been missed: read it afresh.
#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct StateChanged {
    pub resync: bool,
}

/// A saved Run finished, even if Verify is not showing its branch.
#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct RunFinished {
    pub run: crate::ids::RunKey,
    pub branch: crate::browse::BranchKey,
}

#[derive(Default)]
struct Finished(BTreeSet<artifactize_core::types::RunId>);

impl Finished {
    fn seed(
        runs: &[artifactize_core::store::CatalogRun],
        before: artifactize_core::types::Timestamp,
    ) -> Self {
        Self(
            runs.iter()
                .filter(|run| run.completed_at.is_some_and(|at| at < before))
                .map(|run| run.id.clone())
                .collect(),
        )
    }

    async fn observed(&mut self, run: artifactize_core::store::CatalogRun) -> Option<RunFinished> {
        if run.completed_at.is_none() || self.0.contains(&run.id) {
            return None;
        }
        let branch = crate::browse::BranchKey::new(&run.branch().await.ok()?);
        self.0.insert(run.id.clone());
        Some(RunFinished {
            branch,
            run: run.id.into(),
        })
    }
}

/// An Agent session file grew; only its open transcript reads again.
#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct SessionChanged {
    pub session: crate::ids::SessionKey,
}

/// Only the last opened conversation needs a live transcript fallback in this window.
#[derive(Default)]
pub(crate) struct ChatStream(Mutex<Option<(artifactize_core::types::SessionId, Option<u64>)>>);

impl ChatStream {
    pub(crate) fn opened(
        &self,
        chat: &artifactize_core::chat::Chat,
        id: &artifactize_core::types::SessionId,
    ) {
        let mut shown = self.0.lock().expect("Chat stream lock");
        *shown = Some((id.clone(), chat.transcript_size(id)));
    }

    fn changed(&self, chat: &artifactize_core::chat::Chat) -> Option<crate::ids::SessionKey> {
        let mut shown = self.0.lock().expect("Chat stream lock");
        let (id, size) = shown.as_mut()?;
        let current = chat.transcript_size(id);
        if current == *size {
            return None;
        }
        *size = current;
        Some(id.clone().into())
    }
}

/// Chat sessions read by a window may be the only record of a live worktree.
#[derive(Default, Clone)]
pub(crate) struct ChatWorktrees(Arc<Mutex<BTreeMap<PathBuf, Checkout>>>);

type Checkout = Option<artifactize_core::repository::Identity>;

impl ChatWorktrees {
    pub(crate) fn watch(&self, sessions: &[artifactize_core::chat::Session]) {
        let mut known = self.0.lock().expect("Chat worktrees lock");
        for session in sessions {
            let checkout = session
                .worktree
                .is_dir()
                .then(|| session.repository.clone().unwrap_or_default());
            // Once registered, only the rescan consumes filesystem transitions.
            known.entry(session.worktree.clone()).or_insert(checkout);
        }
    }

    fn rescan(&self) -> bool {
        let snapshot = self.0.lock().expect("Chat worktrees lock").clone();
        let changed: Vec<_> = snapshot
            .into_iter()
            .filter_map(|(path, before)| {
                let current = path
                    .is_dir()
                    .then(|| artifactize_core::repository::identify(&path));
                (before != current).then_some((path, current))
            })
            .collect();
        if changed.is_empty() {
            return false;
        }
        self.0.lock().expect("Chat worktrees lock").extend(changed);
        true
    }
}

/// Run owners can die silently; the state hub only announces actual commits.
pub(crate) async fn reconcile_runs(app: &App) {
    let state = &app.launch.state;
    if let Ok(open) = artifactize_core::store::read_open_run_ids(state).await {
        for run in open {
            let _ = artifactize_core::runs::reconcile_interrupted(state, &run).await;
        }
    }
}

/// Forward the hub's hints to every window until the app exits. The subscription itself
/// reconciles missed commits and database replacement every few seconds. Every [`RESCAN`]
/// the live branches are listed again, and a change is a hint as well.
pub fn spawn<R: tauri::Runtime>(app: tauri::AppHandle<R>, state: PathBuf) {
    let boot = artifactize_core::types::Timestamp::now();
    let hub = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut changes = Subscription::new(&state).await;
        let initial = artifactize_core::store::read_catalog(&state).await;
        let mut retry = initial.is_err();
        let mut first = Some(initial.unwrap_or_default());
        let mut finished = Finished::seed(first.as_deref().unwrap_or_default(), boot);
        loop {
            let starting = first.is_some();
            let change = if starting {
                None
            } else if retry {
                tokio::time::timeout(RESCAN, changes.next()).await.ok()
            } else {
                Some(changes.next().await)
            };
            // A failed point read may have missed the only completion hint. Reconcile the
            // catalog after a bounded wait even when no further state changes arrive.
            let runs = if let Some(initial) = first.take() {
                Ok(initial)
            } else if retry || matches!(change, Some(Change::StateInvalidated | Change::Resync)) {
                artifactize_core::store::read_catalog(&state).await
            } else if let Some(Change::RunInvalidated(id)) = &change {
                artifactize_core::store::read_catalog_run(&state, id)
                    .await
                    .map(|run| run.into_iter().collect())
            } else {
                Ok(Vec::new())
            };
            retry = runs.is_err() || (starting && retry);
            let runs = runs.unwrap_or_default();
            let state = hub.state::<App>();
            for run in runs {
                if let Some(event) = finished.observed(run).await {
                    state.currents.recheck_after_run(&event.branch.to_place());
                    if event.emit(&hub).is_err() {
                        return;
                    }
                }
            }
            let Some(change) = change else { continue };
            state.browse.invalidate(&change);
            if !matches!(change, Change::SessionInvalidated(_)) {
                crate::viewer::refresh_active(&state).await;
            }
            if matches!(
                change,
                Change::RunInvalidated(_) | Change::StateInvalidated | Change::Resync
            ) {
                state.human.review_status_changed();
            }
            if matches!(
                change,
                Change::RunInvalidated(_) | Change::StateInvalidated | Change::Resync
            ) {
                state.human.invalidate_searches();
            }
            let sent = match change {
                Change::StateInvalidated | Change::RunInvalidated(_) => {
                    StateChanged { resync: false }.emit(&hub)
                }
                Change::Resync => StateChanged { resync: true }.emit(&hub),
                Change::DraftInvalidated(id) => {
                    crate::human::draft::HumanDraftChanged { id: id.into() }.emit(&hub)
                }
                Change::SessionInvalidated(id) => SessionChanged { session: id.into() }.emit(&hub),
            };
            // The app is closing; nobody listens any more.
            if sent.is_err() {
                return;
            }
        }
    });
    tauri::async_runtime::spawn(async move {
        let mut ticks = tokio::time::interval(RESCAN);
        ticks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            ticks.tick().await;
            // A failed read is the next load's notice; the rescan tries again next tick.
            let app_state = app.state::<App>();
            if let Some(session) = app_state.chat_stream.changed(&app_state.chat)
                && (SessionChanged { session }).emit(&app).is_err()
            {
                return;
            }
            let _ = app_state.chat.reconcile_interrupted().await;
            reconcile_runs(&app_state).await;
            let reviews_changed =
                artifactize_core::store::retire_removed_signoffs(&app_state.launch.state).await;
            let branches_changed = crate::browse::rescan(&app_state).await;
            let review_worktrees_changed = app_state.human.review_worktrees_changed();
            let observed = app_state.chat_worktrees.clone();
            let chat_worktrees_changed = crate::failure::joined(
                tokio::task::spawn_blocking(move || observed.rescan()).await,
            )
            .unwrap_or(false);
            if review_worktrees_changed {
                app_state.human.invalidate_searches();
            }
            if (reviews_changed.unwrap_or(false)
                || branches_changed.unwrap_or(false)
                || review_worktrees_changed
                || chat_worktrees_changed)
                && (StateChanged { resync: false }).emit(&app).is_err()
            {
                return;
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_run_completion_names_its_recorded_branch_once_without_browse() {
        use artifactize_core::{store::CatalogRun, types::RunStatus};
        let mut run = CatalogRun {
            id: "run-finished".parse().unwrap(),
            created_at: "2026-11-21T00:00:00Z".parse().unwrap(),
            completed_at: None,
            repo_path: PathBuf::from("worktree"),
            repository: artifactize_core::repository::Identity {
                common_dir: Some(PathBuf::from("repository")),
                worktree_path: Some(PathBuf::from("worktree")),
                branch: Some("filtered-out".into()),
            },
            status: RunStatus::Running,
            comment: None,
            red: 0,
            error: 0,
            waiting: vec![],
        };
        let cutoff = "2026-11-21T00:01:00Z".parse().unwrap();
        let mut finished = Finished::seed(std::slice::from_ref(&run), cutoff);
        assert!(finished.observed(run.clone()).await.is_none());
        run.status = RunStatus::Green;
        run.completed_at = Some("2026-11-21T00:01:00Z".parse().unwrap());
        // A completion already present in the first catalog is still new if it happened
        // during startup, even when its notification was in flight while seeding.
        let mut finished = Finished::seed(std::slice::from_ref(&run), cutoff);
        let event = serde_json::to_value(finished.observed(run.clone()).await.unwrap()).unwrap();
        assert_eq!(event["run"], "run-finished");
        assert_eq!(
            event["branch"],
            serde_json::json!({"kind":"branch","common":"repository","branch":"filtered-out"})
        );
        assert!(finished.observed(run).await.is_none());
    }

    #[test]
    fn a_chat_only_worktree_disappearing_invalidates_once() {
        let root = tempfile::tempdir().unwrap();
        let worktree = root.path().join("chat-only");
        std::fs::create_dir(&worktree).unwrap();
        let watched = ChatWorktrees::default();
        watched
            .0
            .lock()
            .unwrap()
            .insert(worktree.clone(), Some(Default::default()));
        assert!(!watched.rescan());
        std::fs::remove_dir(worktree).unwrap();
        assert!(watched.rescan());
        assert!(!watched.rescan());
    }
}
