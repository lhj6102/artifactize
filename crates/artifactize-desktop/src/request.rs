//! A request opened by ID that is not a waiting Human review: its read-only Detail in its Run,
//! as a Run's Detail shows it.
use crate::{
    App, Failure,
    detail::{self, DetailView},
};
use artifactize_core::{monitor::NodeId, store};
use time::OffsetDateTime;

/// The read-only Detail of request `id` in its Run.
#[tauri::command]
#[specta::specta]
pub async fn request_detail(
    app: tauri::State<'_, App>,
    id: crate::ids::RequestKey,
) -> Result<DetailView, Failure> {
    let view = store::read_request(&app.launch.state, &id.into_inner())
        .await
        .map_err(Failure::Read)?;
    let (run, requests) = store::read_run_requests(&app.launch.state, &view.request.run_id)
        .await
        .map_err(Failure::Read)?;
    let node = NodeId::eval(view.request.eval_id.clone());
    Ok(detail::view(
        &app.launch.state,
        &run,
        &requests,
        &node,
        OffsetDateTime::now_utc(),
    ))
}
