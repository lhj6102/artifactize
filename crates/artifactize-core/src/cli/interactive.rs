//! Terminal UI setup and cancellation workflows.

use super::{Context, Screens, cancellation_listener};
use std::path::PathBuf;

pub(super) async fn monitor(
    context: Context,
    all: bool,
    path: Option<PathBuf>,
    screens: Screens,
) -> Result<u8, String> {
    if context.json {
        return Err("monitor is an interactive terminal UI; --json is not supported.".into());
    }
    if path.is_some() && context.repo.is_some() {
        return Err("monitor accepts PATH or --repo, not both.".into());
    }
    if all && (context.repo.is_some() || path.is_some()) {
        return Err("monitor accepts --repo or --all, not both.".into());
    }
    let state = crate::store::state_dir(context.state_dir.as_deref())?;
    let repo = path.or(context.repo);
    let (cancellation, listener) = cancellation_listener()?;
    let result = (screens.monitor)(state, repo, cancellation).await;
    listener.abort();
    result?;
    Ok(0)
}

pub(super) async fn review(
    context: Context,
    request: Option<crate::types::RequestId>,
    all: bool,
    reviewer: Option<crate::types::ReviewerId>,
    screens: Screens,
) -> Result<u8, String> {
    if context.json {
        return Err("review is an interactive terminal UI; --json is not supported.".into());
    }
    if all && context.repo.is_some() {
        return Err("review accepts --repo or --all, not both.".into());
    }
    let reviewer = match reviewer {
        Some(reviewer) => Ok(reviewer),
        None => crate::human::default_reviewer(),
    }?;
    let state = crate::store::state_dir(context.state_dir.as_deref())?;
    if let Some(id) = &request {
        crate::store::read_request(&state, id).await?;
    }
    // The list searches every repository; --repo starts it on that repository's term, and
    // --all (the default) stays accepted.
    let repo = context.repo;
    let (cancellation, listener) = cancellation_listener()?;
    let result = (screens.review)(state, repo, reviewer, request, cancellation).await;
    listener.abort();
    result?;
    Ok(0)
}
