//! Where an Artifact is declared: its target and the file that binds it. Moving a binding to
//! another file, or its target elsewhere, is a move; anything else about the file is not.
use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Site {
    pub path: Target,
    pub definition: PathBuf,
}
impl Site {
    /// Base of relative commands and inputs, not the definition's containing folder.
    pub fn folder(&self) -> &Path {
        folder_of(self.path.kind(), &self.path)
    }
    pub fn declaration_path(&self) -> PathBuf {
        self.definition.clone()
    }
}
pub(super) fn folder_of(kind: ArtifactKind, path: &Target) -> &Path {
    match kind {
        ArtifactKind::Folder | ArtifactKind::Aspect | ArtifactKind::Graph => {
            path.local().expect("filesystem kind")
        }
        ArtifactKind::File => path
            .local()
            .expect("filesystem kind")
            .parent()
            .unwrap_or(Path::new("")),
        ArtifactKind::Live => Path::new(""),
    }
}
/// Old locations are used only while migrating and when reading pre-migration saved Runs.
pub(super) fn declaration_of(kind: ArtifactKind, target: &Target) -> PathBuf {
    let path = target.local().unwrap_or(Path::new(""));
    match kind {
        ArtifactKind::Live => PathBuf::new(),
        ArtifactKind::Graph => PathBuf::from(".artf"),
        ArtifactKind::Folder => logical_join(path, std::ffi::OsStr::new(CONFIG_FILE)),
        ArtifactKind::File | ArtifactKind::Aspect => {
            let mut path = path.as_os_str().to_owned();
            path.push(".artf");
            PathBuf::from(path)
        }
    }
}

/// Where every Artifact of the project is declared now.
pub fn read_sites(repo: &Path) -> Result<BTreeMap<ArtifactName, Site>, ConfigError> {
    let config = discovery::read_workspace_declarations(repo)?;
    Ok(config
        .artifacts
        .into_iter()
        .map(|(name, artifact)| {
            let site = artifact.site();
            (name, site)
        })
        .collect())
}

#[derive(Debug, Error)]
pub enum ScopeReadError {
    #[error("{name} is no longer declared at {}.", crate::platform::path_text(.path))]
    Moved { name: ArtifactName, path: PathBuf },
    #[error(transparent)]
    Config(#[from] ConfigError),
}

/// The project as declared now, when `name` is still declared at `site`.
fn read_declared(
    repo: &Path,
    name: &ArtifactName,
    site: &Site,
) -> Result<RepoConfig, ScopeReadError> {
    let config = discovery::read_workspace_declarations(repo)?;
    if !config
        .artifacts
        .get(name)
        .is_some_and(|artifact| artifact.site() == *site)
    {
        return Err(ScopeReadError::Moved {
            name: name.clone(),
            path: site.declaration_path(),
        });
    }
    Ok(config)
}

pub(crate) fn read_artifact_scope(
    repo: &Path,
    name: &ArtifactName,
    site: &Site,
) -> Result<RepoConfig, ScopeReadError> {
    let mut config = read_declared(repo, name, site)?;
    config.evals.clear();
    Ok(config)
}

/// The project as declared now with only eval `id`, when every Artifact it reads is still
/// declared where `sites` says; none when the eval is no longer declared.
pub(crate) fn read_eval_scope(
    repo: &Path,
    id: &EvalId,
    sites: &BTreeMap<ArtifactName, Site>,
) -> Result<Option<RepoConfig>, ScopeReadError> {
    let mut config = discovery::read_workspace_declarations(repo)?;
    for (name, site) in sites {
        if !config
            .artifacts
            .get(name)
            .is_some_and(|artifact| artifact.site() == *site)
        {
            return Err(ScopeReadError::Moved {
                name: name.clone(),
                path: site.declaration_path(),
            });
        }
    }
    config.evals.retain(|eval| &eval.id == id);
    Ok((!config.evals.is_empty()).then_some(config))
}
