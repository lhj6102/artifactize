//! Continue saved conversations without changing their recorded verdicts.
use super::Located;
use crate::{
    agent::{
        self,
        session::{self, Recorder},
    },
    cache,
    config::read_workspace_config,
    store, workspace,
};
use std::path::Path;

/// Metadata for a recorded follow-up.
pub struct Sent {
    pub number: usize,
    pub model: String,
    pub files_changed: Option<bool>,
}

/// Continue the conversation under its lock, so concurrent sends make one thread.
pub async fn send(
    state: &Path,
    repo: Option<&Path>,
    located: &Located,
    message: &str,
    cancellation: tokio_util::sync::CancellationToken,
) -> Result<(Sent, agent::FollowUp), String> {
    if matches!(
        located.request.request.status(),
        crate::types::RequestStatus::Queued | crate::types::RequestStatus::Running
    ) {
        return Err(format!(
            "Request {} is still running; send a follow-up once its review has finished.",
            located.request.request.id
        ));
    }
    // A session that was never saved, or is gone, gets no lock file.
    located.load()?;
    let _lock = tokio::select! {
        lock = session::lock(state, &located.id) => lock?,
        () = cancellation.cancelled() => {
            return Err("Cancelled while waiting for another send to this session.".into());
        }
    };
    let conversation = located.load()?;
    let request = &located.request.request;
    let repo = match repo {
        Some(repo) => repo.to_path_buf(),
        None => store::read_run(state, &request.run_id).await?.run.repo_path,
    };
    let config = read_workspace_config(&repo).map_err(|error| {
        format!(
            "Cannot resolve the review's tools in {}: {error}",
            crate::platform::path_text(&repo)
        )
    })?;
    let eval = config
        .evals
        .iter()
        .find(|eval| eval.id == request.eval_id)
        .ok_or_else(|| {
            format!(
                "Eval {} is no longer declared in {}; its tools cannot be resolved.",
                request.eval_id,
                crate::platform::path_text(&repo)
            )
        })?;
    let output = workspace::prepare_directory(
        &state.join("runs").join(request.run_id.as_str()),
        &config.root,
    )
    .map_err(|e| e.to_string())?;
    let files_changed = files_changed(&config, request, &output, cancellation.clone()).await;
    let header = conversation.header();
    let sent = Sent {
        number: conversation.sends() + 1,
        model: format!(
            "{} {}{}",
            header
                .backend
                .as_ref()
                .map_or("?", crate::config::Backend::as_str),
            header.model.as_deref().unwrap_or("?"),
            header.reasoning.map_or(String::new(), |reasoning| format!(
                " (reasoning {reasoning})"
            ))
        ),
        files_changed,
    };
    let mut recorder = Recorder::append(state, &conversation.path, &located.id, sent.number)?;
    let answer = agent::follow_up(
        &config,
        eval,
        agent::Continuation {
            conversation: &conversation,
            files_changed,
            output: &output,
            state,
        },
        message,
        &mut recorder,
        cancellation,
    )
    .await;
    Ok((sent, answer))
}

/// Whether an Artifact the review's key covered has another fingerprint now; `None` when
/// the review had no key or a fingerprint cannot be computed.
async fn files_changed(
    config: &crate::config::RepoConfig,
    request: &store::Request,
    output: &Path,
    cancellation: tokio_util::sync::CancellationToken,
) -> Option<bool> {
    if request.fingerprints.is_empty() {
        return None;
    }
    if request
        .fingerprints
        .keys()
        .any(|id| !config.artifacts.contains_key(id))
    {
        return Some(true);
    }
    let current = cache::prepare(
        config,
        request.fingerprints.keys(),
        output,
        &cache::Parallelism::new(cache::Parallelism::available()),
        cancellation,
    )
    .await
    .ok()?;
    Some(
        request
            .fingerprints
            .iter()
            .any(|(id, fingerprint)| current.get(id).is_none_or(|now| &now.value != fingerprint)),
    )
}
