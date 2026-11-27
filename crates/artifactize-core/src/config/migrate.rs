//! `artifactize config migrate`: a project from before 0.27, once, into the `.artf` language.
//!
//! It reads the TOML definitions under `.artifactize/` (0.25 and 0.26) or the in-tree
//! `index.artf` and `<file>.artf` markers of earlier versions, the profile aliases of
//! `.artifactize/settings.toml` and old plugin manifests, converts them together, and writes
//! nothing unless the converted project reads.

use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

use serde_json::Value;

use super::{ConfigError, IGNORE_FILE, convert, discovery, lang};

#[derive(Debug, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Migration {
    /// Files written, relative to the project.
    #[serde(serialize_with = "crate::platform::serialize_paths")]
    pub written: Vec<PathBuf>,
    /// Old definition files removed, relative to the project.
    #[serde(serialize_with = "crate::platform::serialize_paths")]
    pub removed: Vec<PathBuf>,
    /// What did not carry over, such as Human result schemas.
    pub warnings: Vec<String>,
}

const SETTINGS: &str = "settings.toml";
/// Folders an in-tree project never declared Artifacts in.
const SKIPPED: [&str; 4] = [".git", "node_modules", "target", discovery::DEFINITION_DIR];

/// Convert the project at `repo`. A project already in the language is left as it is.
pub fn migrate(repo: &Path) -> Result<Migration, ConfigError> {
    let root =
        crate::platform::canonicalize(repo).map_err(|error| ConfigError::new(repo, error))?;
    let definitions = root.join(discovery::DEFINITION_DIR);
    let (catalog, plugin_writes) = super::plugins::migrate(&root)?;
    let mut removed = Vec::new();
    let profiles = settings(&definitions, &mut removed)?;
    let mut current = BTreeMap::new();
    let mut olds = Vec::new();
    let mut warnings = Vec::new();
    let has_definitions =
        crate::platform::paths::reusable_directory(&root, discovery::DEFINITION_DIR)
            .map_err(|error| ConfigError::new(&definitions, error))?;
    if has_definitions {
        for (path, text) in discovery::sources(&root)? {
            if !lang::looks_like_toml(&text) {
                current.insert(path, text);
                continue;
            }
            let relative = path.strip_prefix(&definitions).unwrap_or(&path).to_owned();
            let name = path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .ok_or_else(|| {
                    ConfigError::new(&path, "A definition file is named after its Artifact.")
                })?
                .to_owned();
            let mut table = table(&path, &text, &profiles)?;
            if let Some(object) = table.as_object_mut() {
                object.remove("name");
            }
            olds.push(convert::Old {
                file: relative,
                name,
                table,
                comments: comments(&text),
            });
            removed.push(path);
        }
    }
    // Markers outside `.artifactize/` were the definitions only before it existed.
    if !has_definitions {
        olds.extend(in_tree(&root, &profiles, &mut removed, &mut warnings)?);
    }
    if olds.is_empty() && plugin_writes.is_empty() && removed.is_empty() {
        return Ok(Migration::default());
    }
    let readers = convert::untyped_readers(&olds);
    for old in &mut olds {
        if readers.contains(&old.name)
            && let Some(table) = old.table.as_object_mut()
        {
            table.insert("type".into(), Value::String("files".into()));
        }
    }
    let namespaces: BTreeSet<String> = catalog.keys().map(ToString::to_string).collect();
    let converted = convert::convert(&olds, &profiles, &namespaces)
        .map_err(|error| ConfigError::new(&definitions, error))?;
    warnings.extend(converted.warnings);
    warnings.push(
        "Saved results do not carry over: every eval, Agent, runtime and Human alike, runs once more under the new reuse keys."
            .into(),
    );
    let mut sources = current.clone();
    let mut written = Vec::new();
    for (file, text) in converted.files {
        let path = definitions.join(&file);
        // An existing file may be taken over only by its own exact name, as an old definition
        // being converted in place; one differing only in case is the same file on some volumes.
        let taken = match (path.parent(), path.file_name()) {
            (Some(directory), Some(name))
                if crate::platform::paths::target_exists(directory).unwrap_or(false) =>
            {
                crate::platform::paths::conflicting_entry(directory, std::iter::once(name))
                    .map_err(|error| ConfigError::new(directory, error))?
                    .map(|found| directory.join(found))
                    .filter(|found| found != &path || !removed.contains(found))
            }
            _ => None,
        };
        if current.contains_key(&path) || taken.is_some() {
            return Err(ConfigError::new(
                &path,
                "A file of this name is already in .artifactize/; rename it so the converted definitions can take its name.",
            ));
        }
        sources.insert(path, text);
    }
    discovery::read_sources_with(root.clone(), &sources, catalog)?;
    for (path, text) in plugin_writes.iter().cloned().chain(
        sources
            .into_iter()
            .filter(|(path, _)| !current.contains_key(path)),
    ) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| ConfigError::new(parent, error))?;
        }
        crate::platform::paths::restore_file(&path, text.as_bytes())
            .map_err(|error| ConfigError::new(&path, error))?;
        written.push(path);
    }
    let kept: BTreeSet<&PathBuf> = written.iter().collect();
    let mut gone = Vec::new();
    for path in removed {
        if kept.contains(&path) {
            continue;
        }
        crate::platform::paths::remove_regular(&path)
            .map_err(|error| ConfigError::new(&path, error))?;
        gone.push(path);
    }
    let relative = |paths: Vec<PathBuf>| -> Vec<PathBuf> {
        paths
            .into_iter()
            .map(|path| path.strip_prefix(&root).map(Path::to_owned).unwrap_or(path))
            .collect()
    };
    Ok(Migration {
        written: relative(written),
        removed: relative(gone),
        warnings,
    })
}

/// The named Agent profiles of `.artifactize/settings.toml`, which old evals called aliases.
fn settings(
    definitions: &Path,
    removed: &mut Vec<PathBuf>,
) -> Result<BTreeMap<String, Value>, ConfigError> {
    let path = definitions.join(SETTINGS);
    if !crate::platform::path_kind(&path).is_ok_and(|kind| kind == crate::platform::FileKind::File)
    {
        return Ok(BTreeMap::new());
    }
    let text =
        discovery::read_regular_text(&path).map_err(|error| ConfigError::new(&path, error))?;
    let table: toml::Table =
        toml::from_str(&text).map_err(|error| ConfigError::new(&path, error))?;
    removed.push(path.clone());
    let Some(profiles) = table.get("profiles") else {
        return Ok(BTreeMap::new());
    };
    let profiles = serde_json::to_value(profiles).expect("TOML is JSON");
    let Value::Object(profiles) = profiles else {
        return Err(ConfigError::new(&path, "profiles is a table."));
    };
    Ok(profiles
        .into_iter()
        .map(|(name, mut profile)| {
            if let Some(object) = profile.as_object_mut() {
                object
                    .entry("kind")
                    .or_insert_with(|| Value::String("agent".into()));
            }
            (name, profile)
        })
        .collect())
}

/// An old TOML definition as JSON, its `!alias(…)` profiles resolved from the settings.
fn table(
    path: &Path,
    text: &str,
    profiles: &BTreeMap<String, Value>,
) -> Result<Value, ConfigError> {
    let masked = artifactize_tools::artf::mask(text)
        .map_err(|error| ConfigError::new(path, error.message))?;
    let parsed: toml::Table =
        toml::from_str(&masked.toml).map_err(|error| ConfigError::new(path, error))?;
    let mut value = serde_json::to_value(parsed).expect("TOML is JSON");
    let mut failure = None;
    resolve(&mut value, &mut |placeholder| {
        let index = masked.invocation_index(placeholder)?;
        let invocation = &masked.invocations()[index];
        let Some(base) = profiles.get(&invocation.name) else {
            failure = Some(format!(
                "Unknown profile alias !{}; it is not in .artifactize/settings.toml.",
                invocation.name
            ));
            return None;
        };
        let mut profile = base.clone();
        if let Some(span) = &invocation.overrides_span {
            let wrapped = format!("_ = {{{}}}", &text[span.clone()]);
            match toml::from_str::<toml::Table>(&wrapped) {
                Ok(mut table) => {
                    let overrides = serde_json::to_value(table.remove("_")).expect("TOML is JSON");
                    if let (Some(profile), Some(overrides)) =
                        (profile.as_object_mut(), overrides.as_object())
                    {
                        profile.extend(overrides.clone());
                    }
                }
                Err(error) => {
                    failure = Some(format!("!{}(…): {error}", invocation.name));
                    return None;
                }
            }
        }
        Some(profile)
    });
    match failure {
        Some(message) => Err(ConfigError::new(path, message)),
        None => Ok(value),
    }
}

/// Replace each string `resolved` recognizes.
fn resolve(value: &mut Value, resolved: &mut impl FnMut(&str) -> Option<Value>) {
    match value {
        Value::String(text) => {
            if let Some(replacement) = resolved(text) {
                *value = replacement;
            }
        }
        Value::Array(items) => items.iter_mut().for_each(|item| resolve(item, resolved)),
        Value::Object(fields) => fields
            .values_mut()
            .for_each(|field| resolve(field, resolved)),
        _ => {}
    }
}

/// The markers of a project from before `.artifactize/`: `index.artf` declares its folder (an
/// aspect with `select`), `<file>.artf` its file and a root `.artf` the definitions. A folder
/// Artifact was made of the Artifacts declared below it; that becomes explicit `parts`.
fn in_tree(
    root: &Path,
    profiles: &BTreeMap<String, Value>,
    removed: &mut Vec<PathBuf>,
    warnings: &mut Vec<String>,
) -> Result<Vec<convert::Old>, ConfigError> {
    let ignored = ignore(root)?;
    let mut found: Vec<(String, String, bool, Value, Vec<String>)> = Vec::new();
    let mut pending = vec![(root.to_owned(), PathBuf::new())];
    while let Some((directory, relative)) = pending.pop() {
        let opened = crate::platform::open_directory(&directory)
            .map_err(|error| ConfigError::new(&directory, error))?;
        let mut entries = Vec::new();
        for entry in crate::platform::read_dir(&opened)
            .map_err(|error| ConfigError::new(&directory, error))?
        {
            let entry = entry.map_err(|error| ConfigError::new(&directory, error))?;
            let kind = entry
                .file_type()
                .map_err(|error| ConfigError::new(&directory, error))?;
            entries.push((entry.file_name(), kind));
        }
        entries.sort_by(|a, b| a.0.cmp(&b.0));
        for (name, kind) in entries {
            let path = directory.join(&name);
            let Some(text_name) = name.to_str() else {
                continue;
            };
            let logical = if relative.as_os_str().is_empty() {
                PathBuf::from(text_name)
            } else {
                relative.join(text_name)
            };
            if ignored
                .matched(&path, kind == crate::platform::FileKind::Directory)
                .is_ignore()
            {
                continue;
            }
            if kind == crate::platform::FileKind::Directory {
                // Another project's folder is its own to migrate.
                let separate =
                    crate::platform::paths::reusable_directory(&path, discovery::DEFINITION_DIR)
                        .unwrap_or(false);
                if !SKIPPED.contains(&text_name) && !separate {
                    pending.push((path, logical));
                }
                continue;
            }
            if kind != crate::platform::FileKind::File || !text_name.ends_with(".artf") {
                continue;
            }
            let text = discovery::read_regular_text(&path)
                .map_err(|error| ConfigError::new(&path, error))?;
            let mut table = table(&path, &text, profiles)?;
            let name = table
                .get("name")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    ConfigError::new(&path, "An index.artf names its Artifact with `name`.")
                })?
                .to_owned();
            let folder = crate::platform::path_text(&relative);
            let (target, whole) = if text_name == ".artf" && relative.as_os_str().is_empty() {
                (".artifactize/".to_owned(), false)
            } else if text_name == "index.artf" {
                let target = if folder.is_empty() {
                    "./".to_owned()
                } else {
                    format!("{folder}/")
                };
                (target, table.get("select").is_none())
            } else {
                let file = text_name.strip_suffix(".artf").expect("an .artf file");
                let target = if folder.is_empty() {
                    file.to_owned()
                } else {
                    format!("{folder}/{file}")
                };
                (target, false)
            };
            if let Some(object) = table.as_object_mut() {
                object.remove("name");
                object.insert("path".into(), Value::String(target.clone()));
                // The project folder's content excludes the definitions now kept beside it.
                if target == "./" && !object.contains_key("fingerprint") {
                    object.insert(
                        "fingerprint".into(),
                        serde_json::json!({"ignore": [".artifactize/"]}),
                    );
                    warnings.push(format!(
                        "{name}: its content ignores .artifactize/, where its definitions are now."
                    ));
                }
            }
            removed.push(path);
            found.push((name, target, whole, table, comments(&text)));
        }
    }
    // The nearest enclosing folder Artifact holds each other one as a part.
    let folders: Vec<(String, String)> = found
        .iter()
        .filter(|(_, _, whole, _, _)| *whole)
        .map(|(name, target, _, _, _)| (name.clone(), target.clone()))
        .collect();
    let mut parts: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (name, target, _, _, _) in &found {
        if target == ".artifactize/" {
            continue;
        }
        let owner = folders
            .iter()
            .filter(|(whole, prefix)| {
                whole != name
                    && (prefix == "./" || (target.starts_with(prefix.as_str()) && target != prefix))
            })
            .max_by_key(|(_, prefix)| if prefix == "./" { 0 } else { prefix.len() });
        if let Some((owner, _)) = owner {
            parts.entry(owner.clone()).or_default().push(name.clone());
        }
    }
    Ok(found
        .into_iter()
        .map(|(name, _, _, mut table, comments)| {
            if let Some(added) = parts.get(&name)
                && let Some(object) = table.as_object_mut()
            {
                let list = object
                    .entry("parts")
                    .or_insert_with(|| Value::Array(Vec::new()));
                if let Some(list) = list.as_array_mut() {
                    for part in added {
                        if !list.iter().any(|value| value.as_str() == Some(part)) {
                            list.push(Value::String(part.clone()));
                        }
                    }
                }
            }
            convert::Old {
                file: PathBuf::from(format!("{name}.artf")),
                name,
                table,
                comments,
            }
        })
        .collect())
}

/// The text of a TOML file's comment lines, in order, outside its multi-line strings; a
/// comment after a value stays behind.
fn comments(text: &str) -> Vec<String> {
    let mut open: Option<&str> = None;
    let mut found = Vec::new();
    for line in text.lines() {
        if open.is_none()
            && let Some(comment) = line.trim_start().strip_prefix('#')
        {
            found.push(
                comment
                    .strip_prefix(' ')
                    .unwrap_or(comment)
                    .trim_end()
                    .to_owned(),
            );
            continue;
        }
        for delimiter in ["\"\"\"", "'''"] {
            if open.is_none_or(|open| open == delimiter) && line.matches(delimiter).count() % 2 == 1
            {
                open = if open.is_some() {
                    None
                } else {
                    Some(delimiter)
                };
            }
        }
    }
    found
}

/// The folders the project's `.artfignore` (gitignore syntax) kept out of discovery.
fn ignore(root: &Path) -> Result<ignore::gitignore::Gitignore, ConfigError> {
    let file = root.join(IGNORE_FILE);
    let mut builder = ignore::gitignore::GitignoreBuilder::new(root);
    builder
        .case_insensitive(false)
        .expect("case-sensitive discovery");
    if crate::platform::path_kind(&file).is_ok_and(|kind| kind == crate::platform::FileKind::File) {
        let source =
            discovery::read_regular_text(&file).map_err(|error| ConfigError::new(&file, error))?;
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

#[cfg(test)]
mod tests;
