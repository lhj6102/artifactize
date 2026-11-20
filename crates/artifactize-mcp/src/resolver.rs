//! Resolve and bind the project independently of tool dispatch.
use artifactize_core::{platform, runs};
use rmcp::{
    RoleServer,
    service::{RequestContext, ServiceError},
};
use std::{
    fmt, io,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio::sync::{Mutex, OnceCell};

/// A client lists its roots from memory; this bound only stops one that never answers.
const ROOTS_WAIT: Duration = Duration::from_secs(10);

#[derive(Debug)]
pub(super) enum ProjectError {
    Binding(String),
    RootsTimeout,
    Roots(ServiceError),
    LocalRootMissing,
    CurrentDirectory(io::Error),
    PluginFolder,
}

impl fmt::Display for ProjectError {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Binding(message) => out.write_str(message),
            Self::RootsTimeout => out.write_str("The client did not list its roots in time; pass `project`, the absolute project folder."),
            Self::Roots(error) => write!(out, "The client's roots could not be read: {error}"),
            Self::LocalRootMissing => out.write_str("The client's roots name no local folder; pass `project`, the absolute project folder."),
            Self::CurrentDirectory(error) => error.fmt(out),
            Self::PluginFolder => out.write_str("This host names no project: pass `project`, the absolute project folder, with each call."),
        }
    }
}

/// What the server was started with.
#[derive(Clone)]
pub struct Launch {
    /// The project named with `--repo`.
    pub repo: Option<runs::Context>,
    pub state: Option<PathBuf>,
    pub executable: PathBuf,
}

#[derive(Clone)]
pub(super) struct Resolver {
    launch: Launch,
    /// Each roots generation has its own single-flight project lookup.
    bound: Arc<Mutex<Arc<OnceCell<runs::Context>>>>,
}

impl Resolver {
    pub(super) fn new(launch: Launch) -> Self {
        Self {
            launch,
            bound: Arc::default(),
        }
    }

    /// Serve the call's `project`, the launch's `--repo`, the client's first local root,
    /// or the working directory, in that order.
    pub(super) async fn context(
        &self,
        call: &RequestContext<RoleServer>,
        project: Option<&Path>,
    ) -> Result<runs::Context, ProjectError> {
        let Launch {
            repo,
            state,
            executable,
        } = &self.launch;
        let bind = |repo: &Path| {
            runs::Context::new(repo, state.as_deref(), executable).map_err(ProjectError::Binding)
        };
        if let Some(project) = project {
            return bind(project);
        }
        if let Some(repo) = repo {
            return Ok(repo.clone());
        }
        let generation = Arc::clone(&*self.bound.lock().await);
        generation
            .get_or_try_init(|| async {
                let repo = match root(call).await? {
                    Some(root) => root,
                    None => working_folder()?,
                };
                bind(&repo)
            })
            .await
            .cloned()
    }

    pub(super) async fn roots_changed(&self) {
        // An in-flight discovery can finish only into the old cell. A later call cannot
        // see its result, and this notification doesn't have to wait for the client.
        *self.bound.lock().await = Arc::default();
    }
}

/// The client's first `file:` root, if the client has roots; an error when it has roots but
/// names no local folder, since its working directory is then the plugin's, not a project.
#[expect(
    deprecated,
    reason = "SEP-2577 deprecates roots without a replacement, and every spec version still serves them for a year after 2026-07-28; the `project` argument covers hosts without them"
)]
async fn root(call: &RequestContext<RoleServer>) -> Result<Option<PathBuf>, ProjectError> {
    let declared = call
        .meta
        .client_capabilities()
        .or_else(|| call.peer.peer_info().map(|info| info.capabilities.clone()));
    if declared
        .and_then(|capabilities| capabilities.roots)
        .is_none()
    {
        return Ok(None);
    }
    let listed = tokio::time::timeout(ROOTS_WAIT, call.peer.list_roots())
        .await
        .map_err(|_| ProjectError::RootsTimeout)?
        .map_err(ProjectError::Roots)?;
    listed
        .roots
        .iter()
        .find_map(|root| platform::file_uri_path(&root.uri))
        .map(Some)
        .ok_or(ProjectError::LocalRootMissing)
}

/// A host without roots starts a plugin's server in the plugin's package, not the project.
fn working_folder() -> Result<PathBuf, ProjectError> {
    let folder = platform::current_directory().map_err(ProjectError::CurrentDirectory)?;
    if ["plugin.json", "mcp.json"]
        .iter()
        .all(|marker| platform::is_regular_file(&folder.join(marker)))
    {
        return Err(ProjectError::PluginFolder);
    }
    Ok(folder)
}
