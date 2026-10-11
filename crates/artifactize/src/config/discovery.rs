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
    let root =
        crate::platform::canonicalize(repo).map_err(|error| ConfigError::new(repo, error))?;
    let mut config = RepoConfig {
        root,
        artifacts: BTreeMap::new(),
        evals: Vec::new(),
        relations: Vec::new(),
    };
    let ignored = discovery_ignore(&config.root)?;
    let mut pending = vec![(PathBuf::new(), None::<ArtifactName>)];
    while let Some((relative, mut owner)) = pending.pop() {
        let directory = config.root.join(&relative);
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
                ));
            }
        }
        if let Some(marker) = entries
            .iter()
            .find(|entry| entry.file_name() == CONFIG_FILE)
        {
            let file = marker.path();
            let kind = marker.kind;
            if kind != crate::platform::FileKind::File {
                return Err(ConfigError::new(file, "index.artf must be a regular file."));
            }
            let source =
                read_regular_text(&file).map_err(|error| ConfigError::new(&file, error))?;
            let declaration =
                parse_declaration(&source).map_err(|error| ConfigError::new(&file, error))?;
            if !relative.as_os_str().is_empty() && declaration.review_policy.is_some() {
                return Err(ConfigError::declaration(
                    &file,
                    &["review_policy"],
                    "review_policy belongs only to the repository root index.artf.",
                ));
            }
            let name = declaration.name.clone();
            if let Some(parent) = &owner {
                let parent = config.artifacts.get_mut(parent).unwrap();
                let child = relative.strip_prefix(&parent.path).unwrap();
                let child = child
                    .to_str()
                    .ok_or_else(|| ConfigError::new(&file, "Artifact paths must be UTF-8."))?;
                let child = child
                    .parse()
                    .map_err(|error: String| ConfigError::new(&file, error))?;
                parent.children.insert(child, name.clone());
            }
            insert_artifact(
                &mut config,
                relative.clone(),
                ArtifactKind::Folder,
                declaration,
                &file,
            )?;
            owner = Some(name);
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
                ));
            }
        }
        for marker in &entries {
            if ignored.matched(marker.path(), false).is_ignore() {
                continue;
            }
            let filename = marker.file_name();
            if !filename.as_encoded_bytes().ends_with(b".artf") {
                continue;
            }
            let Some(filename) = filename.to_str() else {
                return Err(ConfigError::new(
                    marker.path(),
                    "Artifact paths must be UTF-8.",
                ));
            };
            let Some(target) = filename.strip_suffix(".artf") else {
                continue;
            };
            if filename == CONFIG_FILE {
                continue;
            }
            let file = marker.path();
            if marker.kind != crate::platform::FileKind::File {
                return Err(ConfigError::new(
                    &file,
                    "File Artifact declaration must be a regular file.",
                ));
            }
            if target.is_empty() || target.ends_with(".artf") {
                return Err(ConfigError::new(
                    &file,
                    "File Artifact target must not be a declaration (.artf).",
                ));
            }
            path(target).map_err(|error| ConfigError::new(&file, error))?;
            if target.contains(':') {
                return Err(ConfigError::new(
                    &file,
                    "File Artifact target name must not contain ':'; scoped tool paths do not support colons.",
                ));
            }
            let kind = crate::platform::entry_kind(&opened, std::ffi::OsStr::new(target)).map_err(
                |error| {
                    ConfigError::new(
                        &file,
                        if error.kind() == std::io::ErrorKind::NotFound {
                            format!("File Artifact target {target} is missing.")
                        } else {
                            format!("Cannot inspect File Artifact target {target}: {error}.")
                        },
                    )
                },
            )?;
            if kind != crate::platform::FileKind::File {
                return Err(ConfigError::new(
                    &file,
                    format!(
                        "File Artifact target {target} must be a regular file, not a symlink, directory or special file."
                    ),
                ));
            }
            let source =
                read_regular_text(&file).map_err(|error| ConfigError::new(&file, error))?;
            let mut declaration =
                parse_declaration(&source).map_err(|error| ConfigError::new(&file, error))?;
            if declaration.review_policy.is_some() {
                return Err(ConfigError::declaration(
                    &file,
                    &["review_policy"],
                    "review_policy belongs only to the repository root index.artf.",
                ));
            }
            let value = toml::de::DeTable::parse(&source)
                .map_err(|error| ConfigError::new(&file, error))?;
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
                        &file,
                        &["fingerprint", "ignore"],
                        "fingerprint.ignore is not supported for a file Artifact.",
                    ));
                }
                if files.iter().any(|input| input != target) {
                    return Err(ConfigError::declaration(
                        &file,
                        &["fingerprint", "files"],
                        format!(
                            "fingerprint.files for a file Artifact may name only its target {target}."
                        ),
                    ));
                }
            }
            insert_artifact(
                &mut config,
                logical_join(&relative, std::ffi::OsStr::new(target)),
                ArtifactKind::File,
                declaration,
                &file,
            )?;
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
    if config.artifacts.is_empty() {
        return Err(ConfigError::new(
            &config.root,
            "Workspace must contain at least one .artf Artifact.",
        ));
    }
    crate::scope::resolve_config(&mut config)?;
    Ok(config)
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
        mounts,
        basis,
        fingerprint,
        review_policy,
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
            mounts,
            basis,
            fingerprint,
            review_policy,
        },
    );
    Ok(())
}

/// The folders the workspace root's `.artfignore` (gitignore syntax) keeps out of
/// discovery, such as test fixtures or example projects with their own `index.artf`.
fn discovery_ignore(root: &Path) -> Result<ignore::gitignore::Gitignore, ConfigError> {
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
