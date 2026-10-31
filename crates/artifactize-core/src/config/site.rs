//! Where each Artifact is declared, and one Artifact's declarations read alone: what an
//! Artifact's Detail needs without reading the whole workspace.

use super::discovery::{FileTargets, Found, Reach, declaration, declare, discovery_ignore, walk};
use super::*;

/// Where an Artifact is declared: its kind and logical path, as a full read places it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Site {
    pub kind: ArtifactKind,
    pub path: PathBuf,
}

impl Site {
    /// The folder the Artifact's declaration is in.
    pub fn folder(&self) -> &Path {
        folder_of(self.kind, &self.path)
    }

    /// The declaration file's logical path.
    pub fn declaration_path(&self) -> PathBuf {
        declaration_of(self.kind, &self.path)
    }
}

pub(super) fn folder_of(kind: ArtifactKind, path: &Path) -> &Path {
    match kind {
        ArtifactKind::Folder | ArtifactKind::Graph => path,
        ArtifactKind::File | ArtifactKind::Aspect => path.parent().unwrap_or(Path::new("")),
    }
}

pub(super) fn declaration_of(kind: ArtifactKind, path: &Path) -> PathBuf {
    match kind {
        ArtifactKind::Graph => PathBuf::from(".artf"),
        ArtifactKind::Folder => logical_join(path, std::ffi::OsStr::new(CONFIG_FILE)),
        ArtifactKind::File | ArtifactKind::Aspect => {
            let mut path = path.as_os_str().to_owned();
            path.push(".artf");
            PathBuf::from(path)
        }
    }
}

/// Every Artifact's site in the workspace at `repo`, from its declarations' names alone:
/// nothing else in them is parsed or validated, so this is no check of the workspace.
pub fn read_sites(repo: &Path) -> Result<BTreeMap<ArtifactName, Site>, ConfigError> {
    let root =
        crate::platform::canonicalize(repo).map_err(|error| ConfigError::new(repo, error))?;
    let ignored = discovery_ignore(&root)?;
    let mut sites = BTreeMap::new();
    walk(
        &root,
        &ignored,
        Reach::Workspace,
        FileTargets::DeclarationsOnly,
        &mut |found: Found<'_>| {
            let name = declared_name(&found.source)
                .map_err(|error| ConfigError::new(found.file, error))?;
            let site = Site {
                kind: found.kind,
                path: found.path,
            };
            if sites.insert(name.clone(), site).is_some() {
                return Err(ConfigError::declaration(
                    found.file,
                    &["name"],
                    format!("Duplicate Artifact name: {name}."),
                ));
            }
            Ok(name)
        },
    )?;
    Ok(sites)
}

/// The `name` of a declaration, read without deserializing or validating the rest.
fn declared_name(source: &str) -> Result<ArtifactName, String> {
    let table = toml::de::DeTable::parse(source).map_err(|error| error.to_string())?;
    match table.get_ref().get("name").map(|name| name.get_ref()) {
        Some(toml::de::DeValue::String(name)) => name.parse(),
        Some(_) => Err("name must be a string.".into()),
        None => Err("missing field `name`".into()),
    }
}

/// Why [`read_artifact_scope`] read no declarations.
#[derive(Debug, Error)]
pub enum ScopeReadError {
    /// The site no longer declares the Artifact: where it is now needs [`read_sites`] again.
    #[error("{name} is no longer declared at {}.", crate::platform::path_text(.path))]
    Moved { name: ArtifactName, path: PathBuf },
    #[error(transparent)]
    Config(#[from] ConfigError),
}

/// The declarations of Artifact `name`, declared at `site`, and of the Artifacts it composes,
/// each parsed and validated as a full read does; no other declaration is read. Checks across
/// the workspace (unique names, references, the graph) stay with the full read, so the config
/// holds only these Artifacts, their children and no evals.
pub(crate) fn read_artifact_scope(
    repo: &Path,
    name: &ArtifactName,
    site: &Site,
) -> Result<RepoConfig, ScopeReadError> {
    let mut config = read_declared(repo, name, site)?;
    config.evals.clear();
    Ok(config)
}

/// The declarations eval `id` reviews: those of the Artifacts in its scope, each declared at
/// its site in `sites`, with the Artifacts they compose, and the eval resolved against them.
/// No other declaration is read, and the checks across the workspace stay with the full read.
/// The config holds no eval when the target no longer declares it, and is none when these
/// declarations name an Artifact outside them, which only a full read places.
pub(crate) fn read_eval_scope(
    repo: &Path,
    id: &crate::types::EvalId,
    sites: &BTreeMap<ArtifactName, Site>,
) -> Result<Option<RepoConfig>, ScopeReadError> {
    let mut scoped: Option<RepoConfig> = None;
    for (name, site) in sites {
        let mut read = read_declared(repo, name, site)?;
        read.evals.retain(|eval| &eval.id == id);
        match &mut scoped {
            None => scoped = Some(read),
            Some(config) => {
                for (name, artifact) in read.artifacts {
                    config.artifacts.entry(name).or_insert(artifact);
                }
                if config.evals.is_empty() {
                    config.evals = read.evals;
                }
            }
        }
    }
    let Some(mut config) = scoped else {
        return Ok(None);
    };
    let outside = sites
        .keys()
        .flat_map(|owner| named(&config.artifacts[owner]))
        .any(|name| !config.artifacts.contains_key(name.as_str()));
    if outside || crate::scope::resolve_evals(&mut config).is_err() {
        return Ok(None);
    }
    Ok(Some(config))
}

/// The Artifacts an Artifact's fingerprint script and Human tools reference by name.
fn named(artifact: &Artifact) -> impl Iterator<Item = &ReferenceName> {
    let script = match &artifact.fingerprint {
        Some(Fingerprint::Script { args, .. }) => args.as_slice(),
        _ => &[],
    };
    let tools = artifact
        .views
        .human_tools
        .values()
        .flat_map(|tool| match tool {
            HumanTool::Command(tool) => tool.args.as_slice(),
            HumanTool::Builtin(tool) => tool.args.as_slice(),
        });
    script
        .iter()
        .chain(tools)
        .filter_map(|argument| match argument {
            crate::scope::Argument::Reference { name, .. } => Some(name),
            crate::scope::Argument::Literal(_) => None,
        })
        // `{artifactPath}` stands for the tool's own Artifact.
        .filter(|name| name.as_str() != "artifactPath")
}

/// [`read_artifact_scope`] with the evals these declarations declare.
fn read_declared(
    repo: &Path,
    name: &ArtifactName,
    site: &Site,
) -> Result<RepoConfig, ScopeReadError> {
    let root =
        crate::platform::canonicalize(repo).map_err(|error| ConfigError::new(repo, error))?;
    let moved = || ScopeReadError::Moved {
        name: name.clone(),
        path: site.declaration_path(),
    };
    let ignored = discovery_ignore(&root)?;
    let folder = site.folder();
    if !crate::platform::path_kind(&root.join(site.declaration_path()))
        .is_ok_and(|kind| kind == crate::platform::FileKind::File)
        || !folder.as_os_str().is_empty()
            && ignored
                .matched_path_or_any_parents(root.join(folder), true)
                .is_ignore()
    {
        return Err(moved());
    }
    let reach = match site.kind {
        ArtifactKind::Folder => Reach::Folder(&site.path),
        ArtifactKind::File | ArtifactKind::Aspect | ArtifactKind::Graph => Reach::File(
            folder,
            site.path
                .file_name()
                .and_then(|target| target.to_str())
                .unwrap_or_default(),
        ),
    };
    let mut config = RepoConfig {
        root: root.clone(),
        artifacts: BTreeMap::new(),
        evals: Vec::new(),
        relations: Vec::new(),
    };
    let mut schemas = super::schemas::Schemas::default();
    walk(
        &root,
        &ignored,
        reach,
        FileTargets::Required,
        &mut |found: Found<'_>| {
            let own = config.artifacts.is_empty();
            if own && (found.kind != site.kind || found.path != site.path) {
                return Err(moved());
            }
            let declaration = declaration(&found, &mut schemas)?;
            if own && &declaration.name != name {
                return Err(moved());
            }
            Ok(declare(&mut config, found, declaration)?)
        },
    )?;
    if !config.artifacts.contains_key(name) {
        return Err(moved());
    }
    Ok(config)
}

#[cfg(test)]
mod tests;
