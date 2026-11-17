//! Definitions live exclusively below `.artifactize/`; target placement has no meaning.

use super::*;
use std::io::Read;

pub const DEFINITION_DIR: &str = ".artifactize";

pub fn read_workspace_config(repo: &Path) -> Result<RepoConfig, ConfigError> {
    read_workspace_declarations(repo)
}

pub(crate) fn read_workspace_declarations(repo: &Path) -> Result<RepoConfig, ConfigError> {
    let root =
        crate::platform::canonicalize(repo).map_err(|error| ConfigError::new(repo, error))?;
    let sources = sources(&root)?;
    read_sources(root, &sources)
}

/// Read and validate an in-memory prospective project, without inspecting target files.
pub(super) fn read_sources(
    root: PathBuf,
    sources: &BTreeMap<PathBuf, String>,
) -> Result<RepoConfig, ConfigError> {
    let mut config = read_sources_unresolved(root, sources)?;
    crate::scope::resolve_config(&mut config).map_err(|mut error| {
        if let Some((keys, message)) = &error.declaration
            && let Some(source) = sources.get(&error.path)
        {
            error.message = location::error_at(
                source,
                &keys.iter().map(String::as_str).collect::<Vec<_>>(),
                message,
            );
        }
        error
    })?;
    Ok(config)
}

pub(super) fn read_sources_unresolved(
    root: PathBuf,
    sources: &BTreeMap<PathBuf, String>,
) -> Result<RepoConfig, ConfigError> {
    let mut config = RepoConfig {
        root,
        artifacts: BTreeMap::new(),
        evals: Vec::new(),
        relations: Vec::new(),
    };
    let mut schemas = schemas::Schemas::default();
    let mut names = BTreeMap::<String, PathBuf>::new();
    for (file, source) in sources {
        let name = file
            .file_stem()
            .and_then(|name| name.to_str())
            .ok_or_else(|| {
                ConfigError::new(file, "line 1, column 1: Artifact names must be UTF-8.")
            })?;
        let name = definition_name(name)
            .map_err(|error| ConfigError::new(file, format!("line 1, column 1: {error}")))?;
        if let Some(previous) = names.insert(name.to_ascii_lowercase(), file.clone()) {
            return Err(ConfigError::new(
                file,
                format!(
                    "line 1, column 1: Duplicate Artifact name: {name}. Files: {} and {}.",
                    crate::platform::path_text(&previous),
                    crate::platform::path_text(file)
                ),
            ));
        }
        let (path, kind, declaration) = parse_definition(&name, source, &mut schemas)
            .map_err(|error| ConfigError::new(file, error))?;
        insert_artifact(&mut config, path, kind, declaration, file)?;
        config
            .artifacts
            .get_mut(&name)
            .expect("just inserted")
            .definition = file.strip_prefix(&config.root).unwrap_or(file).to_owned();
    }
    populate_nested(&mut config);
    for artifact in config.artifacts.values_mut() {
        artifact.target_error = crate::scope::validate_file_target(&config.root, artifact)
            .err()
            .map(|error| error.to_string());
    }
    Ok(config)
}

/// Folder parts hide their own target from a whole's tools, not unrelated nested nodes.
pub(super) fn populate_nested(config: &mut RepoConfig) {
    let nested = config
        .artifacts
        .iter()
        .map(|(name, artifact)| {
            let entries = config
                .descendants(name)
                .into_iter()
                .filter_map(|part| {
                    let part = config.artifacts.get(part)?;
                    if part.kind != ArtifactKind::Folder {
                        return None;
                    }
                    let relative = part.path.strip_prefix(artifact.folder()).ok()?;
                    let prefix = crate::platform::path_text(relative)
                        .parse::<ChildPrefix>()
                        .ok()?;
                    Some((prefix, part.name.clone()))
                })
                .collect();
            let covering_part = config
                .descendants(name)
                .into_iter()
                .filter_map(|part| config.artifacts.get(part))
                .filter(|part| {
                    part.kind == ArtifactKind::Folder && artifact.folder().starts_with(&part.path)
                })
                .max_by_key(|part| part.path.components().count())
                .map(|part| part.name.clone());
            (name.clone(), entries, covering_part)
        })
        .collect::<Vec<_>>();
    for (name, nested, covering_part) in nested {
        let nested: BTreeMap<ChildPrefix, ArtifactName> = nested;
        let nearest = nested
            .iter()
            .filter(|(prefix, _)| {
                !nested.keys().any(|outer| {
                    *prefix != outer
                        && prefix
                            .strip_prefix(outer.as_str())
                            .is_some_and(|rest| rest.starts_with('/'))
                })
            })
            .map(|(prefix, name)| (prefix.clone(), name.clone()))
            .collect();
        let artifact = config.artifacts.get_mut(&name).expect("existing node");
        artifact.nested = nearest;
        artifact.covering_part = covering_part;
    }
}

pub(super) fn definition_name(name: &str) -> Result<ArtifactName, String> {
    artifact_name(name, "Artifact name")?;
    let parsed = name
        .parse::<ArtifactName>()
        .expect("validated Artifact name");
    crate::platform::paths::portable_filename(name)?;
    Ok(parsed)
}

pub(super) fn parse_definition(
    name: &ArtifactName,
    source: &str,
    schemas: &mut schemas::Schemas,
) -> Result<(PathBuf, ArtifactKind, ArtifactDeclaration), String> {
    if toml::de::DeTable::parse(source).is_ok_and(|table| table.get_ref().contains_key("name")) {
        return Err(location::error_at(
            source,
            &["name"],
            "the file name is the Artifact's name",
        ));
    }
    let (logical, kind) = target(source)?;
    // Retain the original spans through typed validation: changing a key in memory
    // must not shift an error away from the source the editor is displaying.
    let declaration = super::definition::parse(
        &super::definition::Input {
            kind,
            name: Some(name),
            path: &logical,
            file: Path::new(""),
            source,
        },
        schemas,
    )
    .map_err(|error| error.message)?;
    Ok((logical, kind, declaration))
}

pub(super) fn files(root: &Path) -> Result<Vec<(PathBuf, crate::platform::FileKind)>, ConfigError> {
    let directory = root.join(DEFINITION_DIR);
    if !crate::platform::path_kind(&directory)
        .is_ok_and(|kind| kind == crate::platform::FileKind::Directory)
    {
        return Err(ConfigError::new(
            &directory,
            "Project definitions belong in .artifactize/; run artifactize config migrate to convert an old project.",
        ));
    }
    let mut pending = vec![directory];
    let mut files = Vec::new();
    while let Some(directory) = pending.pop() {
        let opened = crate::platform::open_directory(&directory)
            .map_err(|error| ConfigError::new(&directory, error))?;
        let entries = crate::platform::read_dir(&opened)
            .map_err(|error| ConfigError::new(&directory, error))?;
        for entry in entries {
            let entry = entry.map_err(|error| ConfigError::new(&directory, error))?;
            let file = directory.join(entry.file_name());
            let kind = entry
                .file_type()
                .map_err(|error| ConfigError::new(&file, error))?;
            let definition = file
                .file_name()
                .is_some_and(|name| name.as_encoded_bytes().ends_with(b".artf"));
            if definition {
                files.push((file, kind));
            } else if kind == crate::platform::FileKind::Directory {
                pending.push(file);
            }
        }
    }
    files.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(files)
}

pub(super) fn sources(root: &Path) -> Result<BTreeMap<PathBuf, String>, ConfigError> {
    files(root)?.into_iter().map(|(file, kind)| {
        if kind != crate::platform::FileKind::File { return Err(ConfigError::new(file, "line 1, column 1: A definition must be a regular file, not a link, directory or special file.")); }
        let source = read_regular_text(&file).map_err(|error| ConfigError::new(&file, error))?;
        Ok((file, source))
    }).collect()
}

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

pub(super) fn insert_artifact(
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
        select,
        parts,
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
            definition: PathBuf::new(),
            target_error: None,
            path,
            kind,
            covering_part: None,
            nested: BTreeMap::new(),
            select: select.unwrap_or_default(),
            parts: parts.into_iter().flatten().collect(),
            name,
            tags,
            views,
            basis,
            fingerprint,
        },
    );
    Ok(())
}

pub(super) fn target(source: &str) -> Result<(PathBuf, ArtifactKind), String> {
    let doc = source
        .parse::<toml_edit::DocumentMut>()
        .map_err(|error| error.to_string())?;
    let target = doc
        .get("path")
        .and_then(toml_edit::Item::as_str)
        .ok_or_else(|| {
            location::error_at(source, &["path"], "path is required and must be a string")
        })?
        .to_owned();
    let kind = if target.ends_with('/') {
        if doc.contains_key("select") {
            ArtifactKind::Aspect
        } else {
            ArtifactKind::Folder
        }
    } else {
        ArtifactKind::File
    };
    let logical = target.strip_suffix('/').unwrap_or(&target);
    let logical = if target == "./" { "" } else { logical };
    if logical.contains(':') {
        return Err(location::error_at(
            source,
            &["path"],
            "Local paths must not contain ':'.",
        ));
    }
    if !logical.is_empty() {
        path(logical).map_err(|error| location::error_at(source, &["path"], &error))?;
    } else if target != "./" {
        return Err(location::error_at(
            source,
            &["path"],
            "path must name a file or folder",
        ));
    }
    if doc.contains_key("select") && kind != ArtifactKind::Aspect {
        return Err(location::error_at(
            source,
            &["select"],
            "select requires a folder path ending in /",
        ));
    }
    Ok((PathBuf::from(logical), kind))
}
