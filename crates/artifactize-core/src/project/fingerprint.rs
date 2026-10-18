//! The current fingerprints of named Artifacts, as review keys use them.
use crate::{cache, config::ArtifactName, store, types::Fingerprint};
use std::path::Path;
use tokio_util::sync::CancellationToken;

/// Prepare the current fingerprint of each of `artifacts`, in the order given, without running
/// evals or saving anything.
pub async fn fingerprints(
    repo: &Path,
    state_dir: Option<&Path>,
    artifacts: &[ArtifactName],
    cancellation: CancellationToken,
) -> Result<Vec<(ArtifactName, Fingerprint)>, String> {
    let config = crate::config::read_workspace_config(repo).map_err(|error| error.to_string())?;
    if let Some(unknown) = artifacts
        .iter()
        .find(|name| !config.artifacts.contains_key(*name))
    {
        return Err(format!("Unknown Artifact: {unknown}"));
    }
    let state = store::state_dir(state_dir)?;
    let parallelism = cache::Parallelism::new(cache::Parallelism::available());
    let prepared = cache::prepare(&config, artifacts, &state, &parallelism, cancellation).await?;
    artifacts
        .iter()
        .map(|name| match prepared.get(name) {
            Some(fingerprint) => Ok((name.clone(), fingerprint.value.clone())),
            None => Err(format!("Artifact {name} declares no fingerprint.")),
        })
        .collect()
}
