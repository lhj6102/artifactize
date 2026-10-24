//! An Artifact's own Human tools in a branch's worktree as it is now, run from its Detail
//! without a review: the library's [`ArtifactDeclarations`] lists them and runs one. Nobody
//! claims anything and nothing is recorded; what a run prints reaches the Artifact's Tool log.
//! Only the Artifact's own declarations and those it composes are read, found by where each
//! worktree's Artifacts are declared ([`Sites`]).
use crate::{
    App, Failure,
    browse::BranchKey,
    human::{Notice, ToolView},
    ids::JobId,
};
use artifactize_core::{
    config::{ScopeReadError, Site, ToolName},
    review::{self, ArtifactDeclarations, Outcome, log::Key},
    types::ArtifactName,
};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
use tokio::sync::OnceCell;

/// Where one worktree's Artifacts are declared.
type Index = Arc<BTreeMap<ArtifactName, Site>>;

/// Where each worktree's Artifacts are declared, from a read of their declarations' names. It
/// is read when an Artifact's tools are first needed and again only when a site no longer
/// declares the Artifact asked for: a move, a rename or a new Artifact. Calls waiting for a
/// worktree's read share it.
#[derive(Default)]
pub struct Sites {
    worktrees: Mutex<BTreeMap<PathBuf, Arc<OnceCell<Index>>>>,
}

impl Sites {
    fn cell(&self, workspace: &Path) -> Arc<OnceCell<Index>> {
        let mut worktrees = self.worktrees.lock().expect("sites lock");
        worktrees.entry(workspace.to_owned()).or_default().clone()
    }

    /// `workspace`'s `cell` is stale: the next call reads the sites again, unless another call
    /// already started that read.
    fn forget(&self, workspace: &Path, cell: &Arc<OnceCell<Index>>) {
        let mut worktrees = self.worktrees.lock().expect("sites lock");
        if worktrees
            .get(workspace)
            .is_some_and(|current| Arc::ptr_eq(current, cell))
        {
            worktrees.remove(workspace);
        }
    }

    /// Artifact `artifact`'s declarations in `workspace` as it is now.
    async fn declarations(
        &self,
        workspace: &Path,
        artifact: &ArtifactName,
    ) -> Result<ArtifactDeclarations, Failure> {
        let mut again = false;
        loop {
            let cell = self.cell(workspace);
            let index = cell
                .get_or_try_init(|| {
                    let workspace = workspace.to_owned();
                    async move {
                        crate::failure::joined(
                            tokio::task::spawn_blocking(move || {
                                artifactize_core::config::read_sites(&workspace)
                            })
                            .await,
                        )?
                        .map(Arc::new)
                        .map_err(|error| Failure::Read(error.to_string()))
                    }
                })
                .await?
                .clone();
            let read = match index.get(artifact) {
                Some(site) => {
                    let (workspace, artifact, site) =
                        (workspace.to_owned(), artifact.clone(), site.clone());
                    crate::failure::joined(
                        tokio::task::spawn_blocking(move || {
                            ArtifactDeclarations::read(&workspace, &artifact, &site)
                        })
                        .await,
                    )?
                    .map_err(Some)
                }
                None => Err(None),
            };
            match read {
                Ok(declarations) => return Ok(declarations),
                Err(None | Some(ScopeReadError::Moved { .. })) if !again => {
                    self.forget(workspace, &cell);
                    again = true;
                }
                Err(None | Some(ScopeReadError::Moved { .. })) => {
                    return Err(Failure::Missing(format!(
                        "{artifact} is not declared in this worktree now."
                    )));
                }
                Err(Some(error)) => return Err(Failure::Read(error.to_string())),
            }
        }
    }
}

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
    let listed = app
        .sites
        .declarations(&workspace, &artifact)
        .await?
        .tools()
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
    let result = match app.sites.declarations(&workspace, &artifact).await {
        Ok(declarations) => declarations.run_tool(&tool, cancel, run).await,
        Err(error) => {
            run.end(review::log::End::Failed(error.to_string()));
            Err(error.to_string())
        }
    };
    app.human.end(job);
    Ok(Outcome::Ran {
        tool,
        claimed: None,
        result,
    }
    .notice()
    .into())
}
