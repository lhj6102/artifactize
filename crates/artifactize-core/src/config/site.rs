//! Target and definition locations are independent; moving a definition changes no meaning.
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
pub fn read_sites(repo: &Path) -> Result<BTreeMap<ArtifactName, Site>, ConfigError> {
    let root =
        crate::platform::canonicalize(repo).map_err(|error| ConfigError::new(repo, error))?;
    discovery::sources(&root)?
        .into_iter()
        .map(|(file, source)| {
            let name = discovery::definition_name(
                file.file_stem()
                    .and_then(|name| name.to_str())
                    .unwrap_or_default(),
            )
            .map_err(|error| ConfigError::new(&file, error))?;
            let (path, kind, declaration) =
                discovery::parse_definition(&name, &source, &mut schemas::Schemas::default())
                    .map_err(|error| ConfigError::new(&file, error))?;
            let path = path
                .with_kind(kind, declaration.select.unwrap_or_default())
                .map_err(|error| ConfigError::new(&file, error))?;
            Ok((
                name,
                Site {
                    path,
                    definition: file.strip_prefix(&root).unwrap().to_owned(),
                },
            ))
        })
        .collect()
}
#[derive(Debug, Error)]
pub enum ScopeReadError {
    #[error("{name} is no longer declared at {}.", crate::platform::path_text(.path))]
    Moved { name: ArtifactName, path: PathBuf },
    #[error(transparent)]
    Config(#[from] ConfigError),
}
fn read_declared(
    repo: &Path,
    name: &ArtifactName,
    site: &Site,
) -> Result<RepoConfig, ScopeReadError> {
    let root =
        crate::platform::canonicalize(repo).map_err(|error| ConfigError::new(repo, error))?;
    let all = discovery::files(&root)?;
    let by_name = all
        .iter()
        .map(|(file, _)| file)
        .filter_map(|file| {
            file.file_stem()
                .and_then(|name| name.to_str())
                .map(|name| (name.to_owned(), file))
        })
        .collect::<BTreeMap<_, _>>();
    let mut selected = BTreeMap::new();
    let mut pending = vec![name.clone()];
    while let Some(next) = pending.pop() {
        let file = if next == *name {
            root.join(site.declaration_path())
        } else {
            by_name
                .get(next.as_str())
                .map(|file| (*file).clone())
                .ok_or_else(|| ConfigError::new(&root, format!("Unknown Artifact {next}.")))?
        };
        if selected.contains_key(&file) {
            continue;
        }
        let source = read_regular_text(&file).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                ScopeReadError::Moved {
                    name: next.clone(),
                    path: site.declaration_path(),
                }
            } else {
                ScopeReadError::Config(ConfigError::new(&file, error))
            }
        })?;
        let (_, _, declaration) =
            discovery::parse_definition(&next, &source, &mut schemas::Schemas::default())
                .map_err(|error| ConfigError::new(&file, error))?;
        pending.extend(declaration.parts.into_iter().flatten());
        selected.insert(file, source);
    }
    let mut config = discovery::read_sources_unresolved(root, &selected)?;
    discovery::inspect_targets(&mut config);
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
pub(crate) fn read_eval_scope(
    repo: &Path,
    id: &EvalId,
    sites: &BTreeMap<ArtifactName, Site>,
) -> Result<Option<RepoConfig>, ScopeReadError> {
    let mut combined: Option<RepoConfig> = None;
    for (name, site) in sites {
        let mut read = read_declared(repo, name, site)?;
        read.evals.retain(|eval| &eval.id == id);
        if let Some(config) = &mut combined {
            config.artifacts.extend(read.artifacts);
            if config.evals.is_empty() {
                config.evals = read.evals;
            }
        } else {
            combined = Some(read);
        }
    }
    let Some(mut config) = combined else {
        return Ok(None);
    };
    if crate::scope::resolve_config(&mut config).is_err() {
        return Ok(None);
    }
    Ok(Some(config))
}
