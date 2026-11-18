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
    let existed = crate::platform::paths::reusable_directory(&root, discovery::DEFINITION_DIR)
        .map_err(|error| ConfigError::new(&destination, error))?;
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
            let fingerprint = doc["fingerprint"]
                .as_table_like_mut()
                .ok_or_else(|| ConfigError::new(&file, "fingerprint must be a table."))?;
            let ignores = fingerprint
                .entry("ignore")
                .or_insert(toml_edit::value(toml_edit::Array::new()))
                .as_array_mut()
                .ok_or_else(|| ConfigError::new(&file, "fingerprint.ignore must be an array."))?;
            ignores.push(".artifactize/");
            report.ignored.push(name.clone());
        }
        let new = destination.join(format!("{name}.artf"));
        report.files.push((
            file.strip_prefix(&root).unwrap().to_owned(),
            new.strip_prefix(&root).unwrap().to_owned(),
        ));
        originals.insert(file, source);
        normalize_commands(doc.as_table_mut());
        sources.insert(new, doc.to_string());
    }
    if !problems.is_empty() {
        return Err(ConfigError::new(&root, problems.join("\n")));
    }
    discovery::read_sources(root.clone(), &sources)?;
    // Report nested old projects without interpreting them as part of this project.
    let ignored = legacy::discovery_ignore(&root)?;
    let opened =
        crate::platform::open_directory(&root).map_err(|error| ConfigError::new(&root, error))?;
    let mut pending = vec![(root.clone(), opened)];
    while let Some((directory, opened)) = pending.pop() {
        for entry in crate::platform::read_dir(&opened)
            .map_err(|error| ConfigError::new(&directory, error))?
        {
            let entry = entry.map_err(|error| ConfigError::new(&directory, error))?;
            let name = entry.file_name();
            let file = directory.join(&name);
            let kind = entry
                .file_type()
                .map_err(|error| ConfigError::new(&file, error))?;
            if kind == crate::platform::FileKind::Directory
                && name != ".git"
                && name != "node_modules"
                && name != "target"
            {
                let child = crate::platform::paths::open_named(&opened, &name)
                    .map_err(|error| ConfigError::new(&file, error))?;
                pending.push((file, child));
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
    let mut created: Vec<PathBuf> = Vec::new();
    for file in sources.keys() {
        let final_file = destination.join(file.file_name().expect("definition filename"));
        if let Err(error) = std::fs::rename(
            staged
                .path()
                .join(file.file_name().expect("definition filename")),
            &final_file,
        ) {
            for created in created {
                let _ = crate::platform::paths::remove_regular(&created);
            }
            if !existed {
                let _ = std::fs::remove_dir(&destination);
            }
            return Err(ConfigError::new(&final_file, error));
        }
        created.push(final_file);
    }
    for file in originals.keys() {
        if let Err(error) = crate::platform::paths::remove_regular(file) {
            for (file, source) in &originals {
                let _ = crate::platform::paths::restore_file(file, source.as_bytes());
            }
            for file in &created {
                let _ = crate::platform::paths::remove_regular(file);
            }
            if !existed {
                let _ = std::fs::remove_dir(&destination);
            }
            return Err(ConfigError::new(file, error));
        }
    }
    Ok(report)
}

/// Normalize only executable fields, retaining all other text and the lines' comments.
fn normalize_commands(table: &mut dyn toml_edit::TableLike) {
    use toml_edit::{Formatted, Item, Value};
    fn visit(item: &mut Item) {
        if let Some(table) = item.as_table_like_mut() {
            normalize_commands(table);
        } else if let Some(tables) = item.as_array_of_tables_mut() {
            for table in tables.iter_mut() {
                normalize_commands(table);
            }
        } else if let Some(array) = item.as_array_mut() {
            for value in array.iter_mut() {
                if let Some(table) = value.as_inline_table_mut() {
                    normalize_commands(table);
                }
            }
        }
    }
    for (key, item) in table.iter_mut() {
        if key.get() == "command"
            && let Some(Value::String(command)) = item.as_value_mut()
        {
            let portable = crate::platform::program_text(command.value());
            if &portable != command.value() {
                let mut normalized = Formatted::new(portable);
                *normalized.decor_mut() = command.decor().clone();
                *command = normalized;
            }
        } else {
            visit(item);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migration_refuses_a_destination_link_or_case_alias_without_writing() {
        let temp = crate::test_os::tempdir();
        let root = temp.path();
        let original = root.join("index.artf");
        let source = "name = 'root'\nbasis = true\n";
        std::fs::write(&original, source).unwrap();

        let target = root.join("linked");
        std::fs::create_dir(&target).unwrap();
        let destination = root.join(discovery::DEFINITION_DIR);
        crate::test_os::link_dir(&target, &destination);
        assert!(migrate(root).unwrap_err().message.contains("exact name"));
        assert_eq!(std::fs::read_dir(&target).unwrap().count(), 0);
        assert_eq!(std::fs::read_to_string(&original).unwrap(), source);
        crate::test_os::remove_link_dir(&destination);

        let alias = root.join(discovery::DEFINITION_DIR.to_ascii_uppercase());
        std::fs::create_dir(&alias).unwrap();
        assert!(migrate(root).unwrap_err().message.contains("exact name"));
        assert_eq!(std::fs::read_dir(&alias).unwrap().count(), 0);
        assert_eq!(std::fs::read_to_string(&original).unwrap(), source);
        std::fs::remove_dir(&alias).unwrap();
        std::fs::create_dir(&destination).unwrap();

        migrate(root).unwrap();
        assert!(destination.join("root.artf").is_file());
        assert!(!original.exists());
    }
}
