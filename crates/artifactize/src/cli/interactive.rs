//! Terminal UI setup and cancellation workflows.

use super::{Screens, cancellation_listener};
use std::path::PathBuf;

pub(super) enum Scope {
    All,
    Repository(PathBuf),
}

impl Scope {
    pub(super) fn from_args(
        all: bool,
        path: Option<PathBuf>,
        repo: Option<PathBuf>,
    ) -> Result<Self, String> {
        match (all, path, repo) {
            (_, Some(_), Some(_)) => Err("monitor accepts PATH or --repo, not both.".into()),
            (true, Some(_), _) | (true, _, Some(_)) => {
                Err("monitor accepts --repo or --all, not both.".into())
            }
            (false, Some(path), None) | (false, None, Some(path)) => Ok(Self::Repository(path)),
            (_, None, None) => Ok(Self::All),
        }
    }

    pub(super) fn for_review(all: bool, repo: Option<PathBuf>) -> Result<Self, String> {
        match (all, repo) {
            (true, Some(_)) => Err("review accepts --repo or --all, not both.".into()),
            (false, Some(path)) => Ok(Self::Repository(path)),
            (_, None) => Ok(Self::All),
        }
    }

    fn repository(self) -> Option<PathBuf> {
        match self {
            Self::All => None,
            Self::Repository(path) => Some(path),
        }
    }
}

pub(super) async fn monitor(
    state_dir: Option<PathBuf>,
    scope: Scope,
    json: bool,
    screens: Screens,
) -> Result<u8, String> {
    if json {
        return Err("monitor is an interactive terminal UI; --json is not supported.".into());
    }
    let state = artifactize_core::store::state_dir(state_dir.as_deref())?;
    let repo = scope.repository();
    let (cancellation, listener) = cancellation_listener()?;
    let result = (screens.monitor)(state, repo, cancellation).await;
    listener.abort();
    result?;
    Ok(0)
}

pub(super) async fn review(
    state_dir: Option<PathBuf>,
    scope: Scope,
    request: Option<artifactize_core::types::RequestId>,
    reviewer: Option<artifactize_core::types::ReviewerId>,
    json: bool,
    screens: Screens,
) -> Result<u8, String> {
    if json {
        return Err("review is an interactive terminal UI; --json is not supported.".into());
    }
    let reviewer = match reviewer {
        Some(reviewer) => Ok(reviewer),
        None => artifactize_core::human::default_reviewer(),
    }?;
    let state = artifactize_core::store::state_dir(state_dir.as_deref())?;
    if let Some(id) = &request {
        artifactize_core::store::read_request(&state, id).await?;
    }
    // The list searches every repository; --repo starts it on that repository's term, and
    // --all (the default) stays accepted.
    let repo = scope.repository();
    let (cancellation, listener) = cancellation_listener()?;
    let result = (screens.review)(state, repo, reviewer, request, cancellation).await;
    listener.abort();
    result?;
    Ok(0)
}
