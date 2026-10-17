//! Shared non-interactive operations and JSON projections. Interfaces add rendering and paging only.
pub mod session;

use crate::{config::read_workspace_config, types::RequestId};
use serde::Serialize;
use serde_json::{Value, json};
use std::path::Path;

#[derive(Debug, Serialize)]
pub struct ConfigCheck {
    pub ok: bool,
    pub artifacts: usize,
    pub evals: usize,
}
pub fn config_check(repo: &Path) -> Result<ConfigCheck, String> {
    let config = read_workspace_config(repo).map_err(|error| error.to_string())?;
    Ok(ConfigCheck {
        ok: true,
        artifacts: config.artifacts.len(),
        evals: config.evals.len(),
    })
}

pub async fn request_show(
    state: &Path,
    id: &RequestId,
    now: time::OffsetDateTime,
) -> Result<Value, String> {
    Ok(crate::query::request_output(
        &crate::store::read_request(state, id).await?,
        now,
    ))
}

pub async fn request_list(
    state: &Path,
    run: Option<&crate::types::RunId>,
    repo: Option<&Path>,
    now: time::OffsetDateTime,
) -> Result<Vec<Value>, String> {
    let views =
        crate::store::read_requests_in(state, run.map(crate::types::RunId::as_str), repo).await?;
    Ok(views
        .iter()
        .map(|view| crate::query::request_output(view, now))
        .collect())
}

pub async fn remote_status(state: Option<&Path>, repo: Option<&Path>) -> Result<Value, String> {
    Ok(remote_output(
        crate::auth::remote::status(state, repo).await?.as_ref(),
    ))
}

pub fn remote_output(status: Option<&crate::auth::remote::Status>) -> Value {
    match status {
        Some(status) => {
            let mut value = json!(status);
            value["configured"] = json!(true);
            value
        }
        None => json!({"configured": false}),
    }
}

pub async fn session_show(state: &Path, reference: &str, summary: bool) -> Result<Value, String> {
    session::show(state, reference, summary).await
}
