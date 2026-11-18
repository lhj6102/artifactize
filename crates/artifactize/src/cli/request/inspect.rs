//! Inspect saved requests and their evidence without executing reviews.

use std::{
    io::{self, Write},
    path::Path,
};

use artifactize_core::{
    inspect, store,
    types::{RequestId, RunId},
};

use super::super::print_json;

pub(super) async fn list(state: &Path, run: Option<RunId>, json: bool) -> Result<(), String> {
    if json {
        print_json(
            &inspect::request_list(state, run.as_ref(), None, time::OffsetDateTime::now_utc())
                .await?,
        )?;
    } else {
        let views = store::read_requests(state, run.as_deref()).await?;
        let mut out = io::stdout().lock();
        writeln!(out, "REQUEST\tRUN\tEVAL\tSTATUS\tREVIEWER").map_err(|e| e.to_string())?;
        for view in views {
            writeln!(
                out,
                "{}\t{}\t{}\t{}\t{}",
                view.request.id,
                view.request.run_id,
                view.request.eval_id,
                view.request.status(),
                view.claim
                    .as_ref()
                    .map_or("-", |claim| claim.reviewer.as_str())
            )
            .map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

pub(super) async fn show(state: &Path, id: &RequestId) -> Result<(), String> {
    print_json(&inspect::request_show(state, id, time::OffsetDateTime::now_utc()).await?)
}

pub(super) async fn logs(
    state: &Path,
    id: RequestId,
    query: inspect::LogOptions,
) -> Result<(), String> {
    print_json(&inspect::logs(state, &id, query.try_into()?).await?)
}
