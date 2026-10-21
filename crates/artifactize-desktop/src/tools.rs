//! An Artifact's own Human tools in a branch's worktree as it is now, run from its Detail
//! without a review: the library's [`review::artifact_tools`] lists them and
//! [`review::run_artifact_tool`] runs one. Nobody claims anything and nothing is recorded; what a
//! run prints reaches the Artifact's Tool log.
use crate::{
    App, Failure,
    browse::BranchKey,
    human::{Notice, ToolView},
    ids::JobId,
};
use artifactize_core::{
    config::ToolName,
    review::{self, Outcome, log::Key},
    types::ArtifactName,
};

fn artifact_name(artifact: &str) -> Result<ArtifactName, Failure> {
    artifact.parse().map_err(Failure::Missing)
}

/// The Human tools Artifact `artifact` declares in `branch`'s worktree as it is now, with
/// what running each does.
#[tauri::command]
#[specta::specta]
pub async fn artifact_tools(
    app: tauri::State<'_, App>,
    branch: BranchKey,
    artifact: String,
) -> Result<Vec<ToolView>, Failure> {
    let artifact = artifact_name(&artifact)?;
    let workspace = app.browse.branch(&branch.to_place())?.workspace;
    let listed = crate::failure::joined(
        tokio::task::spawn_blocking(move || review::artifact_tools(&workspace, &artifact)).await,
    )?
    .map_err(Failure::Read)?;
    Ok(listed
        .into_iter()
        .map(|(tool, resolved)| ToolView {
            action: tool.action(resolved.as_ref()),
            error: resolved.and_then(Result::err),
            operation: tool.operation().to_owned(),
            artifact: tool.artifact.to_string(),
            name: tool.name.to_string(),
            kind: crate::human::kind(tool.kind),
            description: tool.description,
        })
        .collect())
}

/// Run tool `tool` of Artifact `artifact` at once in `branch`'s worktree; job number `job`
/// cancels it. What it prints arrives as [`crate::human::log::HumanToolLog`] events keyed by
/// the Artifact; a launch tool answers at once and its program's output keeps arriving until
/// it exits.
#[tauri::command]
#[specta::specta]
pub async fn artifact_tool_run(
    app: tauri::State<'_, App>,
    branch: BranchKey,
    artifact: String,
    tool: String,
    job: JobId,
) -> Result<Notice, Failure> {
    let artifact = artifact_name(&artifact)?;
    let tool: ToolName = tool.parse().map_err(Failure::Missing)?;
    let workspace = app.browse.branch(&branch.to_place())?.workspace;
    let operation = review::Tool::operation_of(&tool, &artifact).to_owned();
    let run = app.human.log().start(
        Key::Artifact {
            repo: workspace.clone(),
            artifact: artifact.clone(),
        },
        &operation,
        artifact.as_str(),
    );
    let cancel = app.human.start(job);
    let result = review::run_artifact_tool(&workspace, &artifact, &tool, cancel, run).await;
    app.human.end(job);
    Ok(Outcome::Ran {
        tool,
        claimed: None,
        result,
    }
    .notice()
    .into())
}
