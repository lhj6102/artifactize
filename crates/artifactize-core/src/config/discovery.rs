//! Definitions live exclusively below `.artifactize/`; target placement has no meaning.

use super::{authored_target::AuthoredTarget, *};
use std::io::Read;

pub const DEFINITION_DIR: &str = ".artifactize";

pub fn read_workspace_config(repo: &Path) -> Result<RepoConfig, ConfigError> {
    read_workspace_declarations(repo)
}

pub(crate) fn read_workspace_declarations(repo: &Path) -> Result<RepoConfig, ConfigError> {
    let root =
        crate::platform::canonicalize(repo).map_err(|error| ConfigError::new(repo, error))?;
    let sources = sources(&root)?;
    let mut config = read_sources(root, &sources)?;
    inspect_targets(&mut config);
    Ok(config)
}

/// Read and validate an in-memory prospective project, without inspecting target files.
pub(super) fn read_sources(
    root: PathBuf,
    sources: &BTreeMap<PathBuf, String>,
) -> Result<RepoConfig, ConfigError> {
    let config = read_sources_for_migration(root, sources)?;
    crate::scope::validate_eval_tools(&config).map_err(|error| located(error, sources))?;
    Ok(config)
}

/// The old project must parse and resolve before migration can identify missing tools.
pub(super) fn read_sources_for_migration(
    root: PathBuf,
    sources: &BTreeMap<PathBuf, String>,
) -> Result<RepoConfig, ConfigError> {
    let mut config =
        read_sources_unresolved(root, sources).map_err(|error| located(error, sources))?;
    crate::scope::resolve_config(&mut config).map_err(|error| located(error, sources))?;
    Ok(config)
}

fn located(mut error: ConfigError, sources: &BTreeMap<PathBuf, String>) -> ConfigError {
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
            .and_then(crate::platform::os_text)
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
        let parsed = parse_definition(&name, source, &mut schemas)
            .map_err(|error| ConfigError::new(file, error))?;
        insert_artifact(
            &mut config,
            parsed.path,
            parsed.kind,
            parsed.declaration,
            file,
        )?;
        let artifact = config.artifacts.get_mut(&name).expect("just inserted");
        artifact.definition = file
            .strip_prefix(&config.root)
            .expect("definition below workspace")
            .to_owned();
        artifact.authored_path = Some(parsed.authored_path);
    }
    let plugins = super::plugins::catalog(&config.root)?;
    for artifact in config.artifacts.values_mut() {
        if let Some(name) = artifact.type_name.as_ref() {
            let path = config.root.join(artifact.declaration_path());
            let plugin = plugins.get(name).ok_or_else(|| ConfigError::declaration(&path, &["type"], format!("Unknown Artifact type {name}. Install the plugin with artifactize plugin add PATH or choose an installed type.")))?;
            plugin
                .apply(artifact)
                .map_err(|error| ConfigError::declaration(&path, &["type"], error))?;
            artifact.views.validate_sessions().map_err(|error| {
                ConfigError::declaration(&path, &["views", "agent_tools"], error)
            })?;
            if plugin.viewer.is_none()
                && artifact
                    .views
                    .human_tools
                    .values()
                    .any(|tool| matches!(tool, HumanTool::Viewer(_)))
            {
                return Err(ConfigError::declaration(
                    &path,
                    &["views", "human_tools"],
                    "This Artifact type has no viewer entry.",
                ));
            }
        }
    }
    populate_nested(&mut config);
    Ok(config)
}

pub(super) fn inspect_targets(config: &mut RepoConfig) {
    for artifact in config.artifacts.values_mut() {
        artifact.target_error = crate::scope::validate_file_target(&config.root, artifact)
            .err()
            .map(|error| error.to_string());
    }
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
                    if part.kind() != ArtifactKind::Folder {
                        return None;
                    }
                    let relative = part.path.local()?.strip_prefix(artifact.folder()).ok()?;
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
                    part.kind() == ArtifactKind::Folder
                        && artifact.folder().starts_with(part.folder())
                })
                .max_by_key(|part| part.folder().components().count())
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

pub(super) struct ParsedDefinition {
    pub authored_path: String,
    pub path: Target,
    pub kind: ArtifactKind,
    pub declaration: ArtifactDeclaration,
}

pub(super) fn parse_definition(
    name: &ArtifactName,
    source: &str,
    schemas: &mut schemas::Schemas,
) -> Result<ParsedDefinition, String> {
    let table = toml::de::DeTable::parse(source).map_err(|error| error.to_string())?;
    if table.get_ref().contains_key("name") {
        return Err(location::error_at(
            source,
            &["name"],
            "the file name is the Artifact's name",
        ));
    }
    let (authored_path, path, kind) = target(source, &table)?;
    // Retain the original spans through typed validation: changing a key in memory
    // must not shift an error away from the source the editor is displaying.
    let declaration = super::definition::parse_table(
        &super::definition::Input {
            kind,
            name: Some(name),
            path: path.local().unwrap_or(Path::new("")),
            file: Path::new(""),
            source,
        },
        schemas,
        table,
    )
    .map_err(|error| error.message)?;
    Ok(ParsedDefinition {
        authored_path,
        path,
        kind,
        declaration,
    })
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
            if file == root.join(DEFINITION_DIR).join(super::plugins::DIRECTORY) {
                continue;
            }
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
    files(root)?
        .into_iter()
        .map(|(file, kind)| match kind {
            crate::platform::FileKind::File => read_regular_text(&file)
                .map_err(|error| ConfigError::new(&file, error))
                .map(|source| (file, source)),
            _ => Err(ConfigError::new(
                file,
                "line 1, column 1: A definition must be a regular file, not a link, directory or special file.",
            )),
        })
        .collect()
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
    path: Target,
    kind: ArtifactKind,
    declaration: ArtifactDeclaration,
    file: &Path,
) -> Result<(), ConfigError> {
    let ArtifactDeclaration {
        name,
        tags,
        type_name,
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
            instruction_references: Vec::new(),
            variant: None,
        });
    }
    config.artifacts.insert(
        name.clone(),
        Artifact {
            definition: PathBuf::new(),
            authored_path: None,
            target_error: None,
            path: path
                .with_kind(kind, select.unwrap_or_default())
                .map_err(|error| ConfigError::new(file, error))?,
            covering_part: None,
            nested: BTreeMap::new(),
            parts: parts.into_iter().flatten().collect(),
            name,
            tags,
            views,
            type_name,
            plugin: None,
            basis,
            fingerprint,
        },
    );
    Ok(())
}

fn target(
    source: &str,
    table: &toml::Spanned<toml::de::DeTable<'_>>,
) -> Result<(String, Target, ArtifactKind), String> {
    let doc = table.get_ref();
    let text = doc
        .get("path")
        .and_then(|value| match value.get_ref() {
            toml::de::DeValue::String(text) => Some(text.as_ref()),
            _ => None,
        })
        .ok_or_else(|| {
            location::error_at(source, &["path"], "path is required and must be a string")
        })?;
    let (authored, target) = text
        .parse::<AuthoredTarget>()
        .map_err(|error| location::error_at(source, &["path"], &error))?
        .into_parts();
    let kind = match (target.kind(), doc.contains_key("select")) {
        (ArtifactKind::Folder, true) => ArtifactKind::Aspect,
        (ArtifactKind::Live, true) => {
            return Err(location::error_at(
                source,
                &["select"],
                "select is not supported on a Live target",
            ));
        }
        (ArtifactKind::File, true) => {
            return Err(location::error_at(
                source,
                &["select"],
                "select requires a folder path ending in /",
            ));
        }
        (kind, _) => kind,
    };
    Ok((authored, target, kind))
}
