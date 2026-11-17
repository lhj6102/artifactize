//! One-way conversion. Normal discovery never invokes the old reader.
use super::*;

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Migration {
    #[serde(serialize_with = "crate::platform::path_serde::pairs")]
    pub files: Vec<(PathBuf, PathBuf)>,
    pub explicit_parts: BTreeMap<ArtifactName, Vec<ArtifactName>>,
    pub ignored: Vec<ArtifactName>,
    #[serde(serialize_with = "crate::platform::serialize_paths")]
    pub separate_projects: Vec<PathBuf>,
}

/// Validate the complete converted project before changing any of its files.
pub fn migrate(repo: &Path) -> Result<Migration, ConfigError> {
    let root =
        crate::platform::canonicalize(repo).map_err(|error| ConfigError::new(repo, error))?;
    let destination = root.join(discovery::DEFINITION_DIR);
    let existed = destination.exists();
    if existed && !discovery::sources(&root)?.is_empty() {
        return Err(ConfigError::new(
            destination,
            ".artifactize/ already holds definitions; migration writes nothing.",
        ));
    }
    let old = legacy::read_workspace_config(&root)?;
    let mut sources = BTreeMap::new();
    let mut report = Migration {
        files: Vec::new(),
        explicit_parts: BTreeMap::new(),
        ignored: Vec::new(),
        separate_projects: Vec::new(),
    };
    let mut originals = BTreeMap::new();
    let mut problems = Vec::new();
    let mut names = BTreeMap::new();
    for (name, artifact) in &old.artifacts {
        if let Err(error) = discovery::definition_name(name) {
            problems.push(error);
        }
        if let Some(previous) = names.insert(name.to_ascii_lowercase(), name) {
            problems.push(format!("Names collide ignoring case: {previous}, {name}."));
        }
        let file = root.join(artifact.declaration_path());
        let source = read_regular_text(&file).map_err(|error| ConfigError::new(&file, error))?;
        let mut doc = source
            .parse::<toml_edit::DocumentMut>()
            .map_err(|error| ConfigError::new(&file, error))?;
        // Keep the removed name's prefix comments on PATH.
        let prefix = doc
            .get("name")
            .and_then(toml_edit::Item::as_value)
            .map(|value| value.decor().clone());
        let key_decor = doc.key("name").map(|key| key.leaf_decor().clone());
        doc.remove("name");
        let target = match artifact.kind() {
            ArtifactKind::Graph => ".artifactize/".to_owned(),
            ArtifactKind::Folder | ArtifactKind::Aspect => {
                if artifact.folder().as_os_str().is_empty() {
                    "./".to_owned()
                } else {
                    format!("{}/", crate::platform::path_text(artifact.folder()))
                }
            }
            ArtifactKind::File => artifact.path.to_string(),
            ArtifactKind::Live => unreachable!("legacy reader has no Live targets"),
        };
        doc.insert("path", toml_edit::value(target));
        if let Some(decor) = key_decor {
            *doc.key_mut("path").expect("path key").leaf_decor_mut() = decor;
        }
        if let Some(decor) = prefix {
            *doc["path"]
                .as_value_mut()
                .expect("path is a value")
                .decor_mut() = decor;
        }
        if !artifact.parts.is_empty() {
            let mut parts = doc
                .get("parts")
                .and_then(toml_edit::Item::as_array)
                .cloned()
                .unwrap_or_default();
            for part in &artifact.parts {
                if !parts
                    .iter()
                    .any(|value| value.as_str() == Some(part.as_str()))
                {
                    parts.push(part.as_str());
                }
            }
            doc.insert("parts", toml_edit::value(parts));
            report
                .explicit_parts
                .insert(name.clone(), artifact.parts.iter().cloned().collect());
        }
        if artifact.kind() == ArtifactKind::Folder
            && artifact.folder().as_os_str().is_empty()
            && matches!(artifact.fingerprint, Some(Fingerprint::Artifactsum { .. }))
        {
            if doc.get("fingerprint").is_none() {
                doc["fingerprint"] = toml_edit::Item::Table(toml_edit::Table::new());
            }
            if doc["fingerprint"].get("ignore").is_none() {
                doc["fingerprint"]["ignore"] = toml_edit::value(toml_edit::Array::new());
            }
            doc["fingerprint"]["ignore"]
                .as_array_mut()
                .expect("validated ignores")
                .push(".artifactize/");
            report.ignored.push(name.clone());
        }
        let new = destination.join(format!("{name}.artf"));
        report.files.push((
            file.strip_prefix(&root).unwrap().to_owned(),
            new.strip_prefix(&root).unwrap().to_owned(),
        ));
        originals.insert(file, source);
        sources.insert(new, doc.to_string());
    }
    if !problems.is_empty() {
        return Err(ConfigError::new(&root, problems.join("\n")));
    }
    discovery::read_sources(root.clone(), &sources)?;
    // Report nested old projects without interpreting them as part of this project.
    let ignored = legacy::discovery_ignore(&root)?;
    let mut pending = vec![root.clone()];
    while let Some(directory) = pending.pop() {
        for entry in
            std::fs::read_dir(&directory).map_err(|error| ConfigError::new(&directory, error))?
        {
            let entry = entry.map_err(|error| ConfigError::new(&directory, error))?;
            let file = entry.path();
            let kind = crate::platform::path_kind(&file)
                .map_err(|error| ConfigError::new(&file, error))?;
            if kind == crate::platform::FileKind::Directory
                && entry.file_name() != ".git"
                && entry.file_name() != "node_modules"
                && entry.file_name() != "target"
            {
                pending.push(file);
            } else if kind == crate::platform::FileKind::File
                && file
                    .extension()
                    .is_some_and(|extension| extension == "artf")
                && !originals.contains_key(&file)
                && ignored
                    .matched_path_or_any_parents(&file, false)
                    .is_ignore()
            {
                report
                    .separate_projects
                    .push(file.strip_prefix(&root).unwrap().to_owned());
            }
        }
    }
    let ignore = root.join(IGNORE_FILE);
    if crate::platform::path_kind(&ignore).is_ok_and(|kind| kind == crate::platform::FileKind::File)
    {
        originals.insert(
            ignore.clone(),
            read_regular_text(&ignore).map_err(|error| ConfigError::new(&ignore, error))?,
        );
    }
    let staged = tempfile::Builder::new()
        .prefix(".artifactize-migrate-")
        .tempdir_in(&root)
        .map_err(|error| ConfigError::new(&root, error))?;
    for (file, source) in &sources {
        let file = staged
            .path()
            .join(file.file_name().expect("definition filename"));
        std::fs::write(&file, source).map_err(|error| ConfigError::new(&file, error))?;
    }
    if !existed {
        std::fs::create_dir(&destination).map_err(|error| ConfigError::new(&destination, error))?;
    }
    let mut created = Vec::new();
    for file in sources.keys() {
        let final_file = destination.join(file.file_name().expect("definition filename"));
        if let Err(error) = std::fs::rename(
            staged
                .path()
                .join(file.file_name().expect("definition filename")),
            &final_file,
        ) {
            for created in created {
                let _ = std::fs::remove_file(created);
            }
            if !existed {
                let _ = std::fs::remove_dir(&destination);
            }
            return Err(ConfigError::new(&final_file, error));
        }
        created.push(final_file);
    }
    for file in originals.keys() {
        if let Err(error) = std::fs::remove_file(file) {
            for (file, source) in &originals {
                let _ = std::fs::write(file, source);
            }
            for file in &created {
                let _ = std::fs::remove_file(file);
            }
            if !existed {
                let _ = std::fs::remove_dir(&destination);
            }
            return Err(ConfigError::new(file, error));
        }
    }
    Ok(report)
}
