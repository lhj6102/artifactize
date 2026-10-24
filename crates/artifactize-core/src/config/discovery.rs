//! Deterministic marker discovery through the no-follow platform file interface.

use super::*;
use std::io::Read;

struct DiscoveryEntry {
    path: PathBuf,
    name: std::ffi::OsString,
    kind: crate::platform::FileKind,
}
impl DiscoveryEntry {
    fn path(&self) -> PathBuf {
        self.path.clone()
    }
    fn file_name(&self) -> std::ffi::OsString {
        self.name.clone()
    }
}

/// Discover regular markers in deterministic path order, without following directory links.
pub fn read_workspace_config(repo: &Path) -> Result<RepoConfig, ConfigError> {
    read(repo, FileTargets::Required)
}
/// A declaration guard must not mistake a changed data file for a changed declaration.
/// Preparation and per-Eval rechecks validate actual file targets separately.
pub(crate) fn read_workspace_declarations(repo: &Path) -> Result<RepoConfig, ConfigError> {
    read(repo, FileTargets::DeclarationsOnly)
}
#[derive(Clone, Copy)]
pub(super) enum FileTargets {
    Required,
    DeclarationsOnly,
}

fn read(repo: &Path, targets: FileTargets) -> Result<RepoConfig, ConfigError> {
    let root =
        crate::platform::canonicalize(repo).map_err(|error| ConfigError::new(repo, error))?;
    let ignored = discovery_ignore(&root)?;
    let mut config = RepoConfig {
        root: root.clone(),
        artifacts: BTreeMap::new(),
        evals: Vec::new(),
        relations: Vec::new(),
    };
    walk(
        &root,
        &ignored,
        Reach::Workspace,
        targets,
        &mut |found: Found<'_>| {
            let declaration = declaration(&found)?;
            declare(&mut config, found, declaration)
        },
    )?;
    if config.artifacts.is_empty() {
        return Err(ConfigError::new(
            &config.root,
            "Workspace must contain at least one .artf Artifact.",
        ));
    }
    crate::scope::resolve_config(&mut config)?;
    Ok(config)
}

/// Which declarations a walk visits.
#[derive(Clone, Copy)]
pub(super) enum Reach<'a> {
    /// Every declaration in the workspace.
    Workspace,
    /// A folder Artifact's `index.artf` and those below it: the folder Artifacts it composes.
    Folder(&'a Path),
    /// The declaration of the file Artifact for target `.1` in folder `.0`, or with an empty
    /// target the graph Artifact's `.artf`.
    File(&'a Path, &'a str),
}

/// A declaration a walk found.
pub(super) struct Found<'a> {
    pub(super) kind: ArtifactKind,
    /// The Artifact's logical path.
    pub(super) path: PathBuf,
    pub(super) file: &'a Path,
    pub(super) source: String,
    /// The folder Artifact that composes it, and its child prefix there.
    parent: Option<(&'a ArtifactName, ChildPrefix)>,
}

/// Walk `reach` below `root` in deterministic path order, handing each declaration to
/// `visit`, which names the Artifact it declares.
pub(super) fn walk<E: From<ConfigError>>(
    root: &Path,
    ignored: &ignore::gitignore::Gitignore,
    reach: Reach<'_>,
    targets: FileTargets,
    visit: &mut dyn FnMut(Found<'_>) -> Result<ArtifactName, E>,
) -> Result<(), E> {
    let (start, recursive) = match reach {
        Reach::Workspace => (PathBuf::new(), true),
        Reach::Folder(folder) => (folder.to_owned(), true),
        Reach::File(folder, _) => (folder.to_owned(), false),
    };
    // Each folder with the folder Artifact that composes what it declares, and its path.
    let mut pending = vec![(start, None::<(ArtifactName, PathBuf)>)];
    while let Some((relative, mut owner)) = pending.pop() {
        let directory = root.join(&relative);
        let opened = crate::platform::open_directory(&directory)
            .map_err(|error| ConfigError::new(&directory, error))?;
        let entries = crate::platform::read_dir(&opened)
            .and_then(|entries| {
                entries
                    .map(|entry| {
                        let entry = entry?;
                        let name = entry.file_name();
                        let kind = entry.file_type()?;
                        Ok(DiscoveryEntry {
                            path: directory.join(&name),
                            name,
                            kind,
                        })
                    })
                    .collect::<Result<Vec<_>, std::io::Error>>()
            })
            .map_err(|error| ConfigError::new(&directory, error))?;
        let mut entries = entries;
        entries.sort_by_key(|entry| entry.file_name());
        if let Some(legacy) = entries
            .iter()
            .find(|entry| entry.file_name() == "artifactize.json")
        {
            let file = legacy.path();
            let kind = legacy.kind;
            if !ignored
                .matched(&file, kind == crate::platform::FileKind::Directory)
                .is_ignore()
                && kind != crate::platform::FileKind::Directory
            {
                return Err(ConfigError::new(
                    file,
                    "artifactize.json is no longer read; declare this Artifact in index.artf (TOML).",
                )
                .into());
            }
        }
        if let Some(marker) = entries
            .iter()
            .find(|entry| entry.file_name() == CONFIG_FILE)
            .filter(|_| !matches!(reach, Reach::File(..)))
        {
            let file = marker.path();
            let kind = marker.kind;
            if kind != crate::platform::FileKind::File {
                return Err(ConfigError::new(file, "index.artf must be a regular file.").into());
            }
            let source =
                read_regular_text(&file).map_err(|error| ConfigError::new(&file, error))?;
            let parent = match &owner {
                Some((parent, path)) => {
                    let child = relative.strip_prefix(path).unwrap();
                    let child = child
                        .to_str()
                        .ok_or_else(|| ConfigError::new(&file, "Artifact paths must be UTF-8."))?;
                    let child = child
                        .parse()
                        .map_err(|error: String| ConfigError::new(&file, error))?;
                    Some((parent, child))
                }
                None => None,
            };
            let name = visit(Found {
                kind: ArtifactKind::Folder,
                path: relative.clone(),
                file: &file,
                source,
                parent,
            })?;
            owner = Some((name, relative.clone()));
        }
        for marker in &entries {
            if !ignored.matched(marker.path(), false).is_ignore()
                && marker
                    .file_name()
                    .as_encoded_bytes()
                    .ends_with(b".artf.artf")
            {
                return Err(ConfigError::new(
                    marker.path(),
                    "File Artifact target must not be a declaration (.artf).",
                )
                .into());
            }
        }
        for marker in entries
            .iter()
            .filter(|_| !matches!(reach, Reach::Folder(_)))
        {
            if marker.file_name() != ".artf" && ignored.matched(marker.path(), false).is_ignore() {
                continue;
            }
            let filename = marker.file_name();
            if !filename.as_encoded_bytes().ends_with(b".artf") {
                continue;
            }
            let Some(filename) = filename.to_str() else {
                return Err(
                    ConfigError::new(marker.path(), "Artifact paths must be UTF-8.").into(),
                );
            };
            let Some(target) = filename.strip_suffix(".artf") else {
                continue;
            };
            if filename == CONFIG_FILE
                || matches!(reach, Reach::File(_, wanted) if wanted != target)
            {
                continue;
            }
            let file = marker.path();
            if marker.kind != crate::platform::FileKind::File {
                return Err(ConfigError::new(
                    &file,
                    "File Artifact declaration must be a regular file.",
                )
                .into());
            }
            if target.is_empty() {
                if !relative.as_os_str().is_empty() {
                    return Err(ConfigError::new(
                        &file,
                        "The graph Artifact .artf belongs only at the repository root.",
                    )
                    .into());
                }
                let source =
                    read_regular_text(&file).map_err(|error| ConfigError::new(&file, error))?;
                visit(Found {
                    kind: ArtifactKind::Graph,
                    path: PathBuf::new(),
                    file: &file,
                    source,
                    parent: None,
                })?;
                continue;
            }
            if target.ends_with(".artf") {
                return Err(ConfigError::new(
                    &file,
                    "File Artifact target must not be a declaration (.artf).",
                )
                .into());
            }
            path(target).map_err(|error| ConfigError::new(&file, error))?;
            if target.contains(':') {
                return Err(ConfigError::new(
                    &file,
                    "File Artifact target name must not contain ':'; scoped tool paths do not support colons.",
                )
                .into());
            }
            if matches!(targets, FileTargets::Required) {
                let kind = crate::platform::entry_kind(&opened, std::ffi::OsStr::new(target))
                    .map_err(|error| {
                        ConfigError::new(
                            &file,
                            if error.kind() == std::io::ErrorKind::NotFound {
                                format!("File Artifact target {target} is missing.")
                            } else {
                                format!("Cannot inspect File Artifact target {target}: {error}.")
                            },
                        )
                    })?;
                if kind != crate::platform::FileKind::File {
                    return Err(ConfigError::new(
                        &file,
                        format!(
                            "File Artifact target {target} must be a regular file, not a symlink, directory or special file."
                        ),
                    )
                    .into());
                }
            }
            let source =
                read_regular_text(&file).map_err(|error| ConfigError::new(&file, error))?;
            visit(Found {
                kind: ArtifactKind::File,
                path: logical_join(&relative, std::ffi::OsStr::new(target)),
                file: &file,
                source,
                parent: None,
            })?;
        }
        if !recursive {
            continue;
        }
        for entry in entries.into_iter().rev() {
            let kind = entry.kind;
            if kind == crate::platform::FileKind::Directory
                && entry.file_name() != ".git"
                && entry.file_name() != "node_modules"
                && !ignored.matched(entry.path(), true).is_ignore()
            {
                pending.push((logical_join(&relative, &entry.file_name()), owner.clone()));
            }
        }
    }
    Ok(())
}

/// Parse and validate the declaration a walk found, as its kind declares it.
pub(super) fn declaration(found: &Found<'_>) -> Result<ArtifactDeclaration, ConfigError> {
    let file = found.file;
    let source = &found.source;
    match found.kind {
        ArtifactKind::Folder => {
            parse_declaration(source).map_err(|error| ConfigError::new(file, error))
        }
        ArtifactKind::Graph => {
            let table: toml::Table =
                toml::from_str(source).map_err(|error| ConfigError::new(file, error))?;
            if table.contains_key("fingerprint") {
                return Err(ConfigError::declaration(
                    file,
                    &["fingerprint"],
                    "The graph Artifact cannot declare fingerprint.",
                ));
            }
            let mut declaration =
                parse_declaration(source).map_err(|error| ConfigError::new(file, error))?;
            declaration.fingerprint = Some(Fingerprint::Graph);
            Ok(declaration)
        }
        ArtifactKind::File => {
            let target = found
                .path
                .file_name()
                .and_then(|target| target.to_str())
                .expect("a walk names file Artifacts by their UTF-8 target");
            let mut declaration =
                parse_declaration(source).map_err(|error| ConfigError::new(file, error))?;
            let value =
                toml::de::DeTable::parse(source).map_err(|error| ConfigError::new(file, error))?;
            if value.get_ref().get("fingerprint").is_none() {
                declaration.fingerprint = Some(Fingerprint::Artifactsum {
                    files: vec![target.parse().expect("validated target logical path")],
                    ignore: Vec::new(),
                });
            }
            if let Some(Fingerprint::Artifactsum { files, .. }) = &declaration.fingerprint {
                if value
                    .get_ref()
                    .get("fingerprint")
                    .and_then(|value| value.get_ref().get("ignore"))
                    .is_some()
                {
                    return Err(ConfigError::declaration(
                        file,
                        &["fingerprint", "ignore"],
                        "fingerprint.ignore is not supported for a file Artifact.",
                    ));
                }
                if files.iter().any(|input| input != target) {
                    return Err(ConfigError::declaration(
                        file,
                        &["fingerprint", "files"],
                        format!(
                            "fingerprint.files for a file Artifact may name only its target {target}."
                        ),
                    ));
                }
            }
            Ok(declaration)
        }
    }
}

/// Add a found declaration to `config`, as a child of the folder Artifact composing it.
pub(super) fn declare(
    config: &mut RepoConfig,
    found: Found<'_>,
    declaration: ArtifactDeclaration,
) -> Result<ArtifactName, ConfigError> {
    let name = declaration.name.clone();
    if let Some((parent, child)) = found.parent {
        let parent = config.artifacts.get_mut(parent).unwrap();
        parent.children.insert(child, name.clone());
    }
    insert_artifact(config, found.path, found.kind, declaration, found.file)?;
    Ok(name)
}

fn insert_artifact(
    config: &mut RepoConfig,
    path: PathBuf,
    kind: ArtifactKind,
    declaration: ArtifactDeclaration,
    file: &Path,
) -> Result<(), ConfigError> {
    let ArtifactDeclaration {
        name,
        tags,
        evals,
        views,
        basis,
        fingerprint,
    } = declaration;
    if config.artifacts.contains_key(&name) {
        return Err(ConfigError::declaration(
            file,
            &["name"],
            format!("Duplicate Artifact name: {name}."),
        ));
    }
    for declaration in evals {
        config.evals.push(Eval {
            id: format!("{name}/{}", declaration.id)
                .parse()
                .expect("an Artifact name and a local Eval id form an Eval id"),
            target: name.clone(),
            references: BTreeMap::new(),
            deps: Vec::new(),
            declaration,
            variant: None,
        });
    }
    config.artifacts.insert(
        name.clone(),
        Artifact {
            path,
            kind,
            children: BTreeMap::new(),
            name,
            tags,
            views,
            basis,
            fingerprint,
        },
    );
    Ok(())
}

/// The folders the workspace root's `.artfignore` (gitignore syntax) keeps out of
/// discovery, such as test fixtures or example projects with their own `index.artf`.
pub(super) fn discovery_ignore(root: &Path) -> Result<ignore::gitignore::Gitignore, ConfigError> {
    let legacy = root.join(".artifactizeignore");
    if crate::platform::marker_metadata(&legacy).is_ok() {
        return Err(ConfigError::new(
            legacy,
            ".artifactizeignore was renamed to .artfignore.",
        ));
    }
    let file = root.join(IGNORE_FILE);
    let mut builder = ignore::gitignore::GitignoreBuilder::new(root);
    builder
        .case_insensitive(false)
        .expect("case-sensitive discovery");
    // A regular file only: discovery never follows links.
    if crate::platform::path_kind(&file).is_ok_and(|kind| kind == crate::platform::FileKind::File) {
        let source = read_regular_text(&file).map_err(|error| ConfigError::new(&file, error))?;
        for line in source.lines() {
            builder
                .add_line(Some(file.clone()), line)
                .map_err(|error| ConfigError::new(&file, error))?;
        }
    }
    builder
        .build()
        .map_err(|error| ConfigError::new(&file, error))
}

/// A logical path below the workspace root: components joined with `/` on every platform,
/// the form Artifact paths take in scopes, references and output.
pub(super) fn logical_join(parent: &Path, name: &std::ffi::OsStr) -> PathBuf {
    if parent.as_os_str().is_empty() {
        return PathBuf::from(name);
    }
    let mut path = parent.as_os_str().to_owned();
    path.push("/");
    path.push(name);
    PathBuf::from(path)
}

pub(super) fn read_regular_text(path: &Path) -> std::io::Result<String> {
    let mut source = String::new();
    crate::platform::open_regular(path)?.read_to_string(&mut source)?;
    Ok(source)
}
