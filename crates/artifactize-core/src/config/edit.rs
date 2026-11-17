//! Workbench file operations, shared with `config rename`.
//!
//! Call [`read`] to obtain project-relative file contents. For each file an operation
//! writes (including a new name, whose content is `None`), pass its last-read content in
//! [`BasedOn`]. [`apply`] returns fresh contents for the next edit. Refusals write nothing:
//! invalid definitions include their file and source position, conflicts include the disk
//! contents, and forbidden operations explain the remaining reference or cycle.
//!
//! [`read_project`] returns each node's definition file, exact authored PATH, target kind,
//! missing-target mark, raw-content SHA-256 identity and annotations, plus the file contents
//! used as [`BasedOn`]. Merge a successful delta into those bases before the next edit.
//! Creation is always `.artifactize/<name>.artf`; rename stays in its original subfolder.
//!
//! [`Operation`] creates a node, changes PATH or source, adds/removes/moves parts, renames
//! and deletes, and saves/resets places or sets/moves a comment. A move changes both wholes
//! in one transaction. Rename updates structural references and whole-local place keys;
//! delete names every remaining referrer. Comments and unknown annotations are not references.
//! A stale file returns [`Refusal::Changed`] with its current text (`None` when deleted),
//! including when the changed text is no longer valid. Source can repair that text after
//! the caller reloads its base. Invalid candidates include file, line and column; cycles,
//! occupied comment destinations and remaining referrers have readable reasons.
//!
//! Every complete prospective project validates before writes. Annotations never invalidate
//! a configuration check; a known malformed section refuses operations that edit it. Other
//! annotation data and untouched text remain as authored. The UI owns unsaved-typing policy.
//! [`Operation::SavePlaces`] uses a plain node name for its top-level `at`, or `whole/part`
//! for a whole-local position. The client supplies every top-level node on its first manual
//! placement, and destination-local coordinates for drops. Omitted drop coordinates carry
//! the previous numeric position. Reset with `None` removes top-level places only; `Some`
//! removes that whole's local places. Empty comment text removes it. No layout file exists.
use super::*;
mod annotations;
pub use annotations::Annotations;
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct BasedOn(
    #[serde(with = "crate::platform::path_serde::map")] BTreeMap<PathBuf, Option<String>>,
);
impl std::ops::Deref for BasedOn {
    type Target = BTreeMap<PathBuf, Option<String>>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl std::ops::DerefMut for BasedOn {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}
impl FromIterator<(PathBuf, Option<String>)> for BasedOn {
    fn from_iter<T: IntoIterator<Item = (PathBuf, Option<String>)>>(iter: T) -> Self {
        Self(iter.into_iter().collect())
    }
}
impl IntoIterator for BasedOn {
    type Item = (PathBuf, Option<String>);
    type IntoIter = std::collections::btree_map::IntoIter<PathBuf, Option<String>>;
    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}
impl Extend<(PathBuf, Option<String>)> for BasedOn {
    fn extend<T: IntoIterator<Item = (PathBuf, Option<String>)>>(&mut self, iter: T) {
        self.0.extend(iter);
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Operation {
    Create {
        name: ArtifactName,
        path: String,
    },
    SetPath {
        name: ArtifactName,
        path: String,
    },
    AddPart {
        whole: ArtifactName,
        part: ArtifactName,
        #[serde(default)]
        at: Option<[f64; 2]>,
    },
    RemovePart {
        whole: ArtifactName,
        part: ArtifactName,
    },
    MovePart {
        part: ArtifactName,
        from: Option<ArtifactName>,
        to: Option<ArtifactName>,
        #[serde(default)]
        at: Option<[f64; 2]>,
    },
    SaveSource {
        name: ArtifactName,
        source: String,
    },
    Rename {
        old: ArtifactName,
        new: ArtifactName,
    },
    Delete {
        name: ArtifactName,
    },
    SavePlaces {
        places: BTreeMap<String, [f64; 2]>,
    },
    SetComment {
        name: ArtifactName,
        text: String,
    },
    MoveComment {
        from: ArtifactName,
        to: ArtifactName,
    },
    ResetPlaces {
        whole: Option<ArtifactName>,
    },
}
#[derive(Debug, Serialize, thiserror::Error)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Refusal {
    #[error("{file}: {message}")]
    Invalid {
        file: String,
        message: String,
        line: Option<usize>,
        column: Option<usize>,
    },
    #[error("{} changed on disk; reload its current content.", crate::platform::path_text(.file))]
    Changed {
        #[serde(with = "crate::platform::path_serde")]
        file: PathBuf,
        current: Option<String>,
    },
    #[error("{reason}")]
    NotAllowed { reason: String },
    #[error("{message}")]
    Io { message: String },
}
impl From<ConfigError> for Refusal {
    fn from(error: ConfigError) -> Self {
        Self::Invalid {
            file: crate::platform::path_text(&error.path),
            line: position(&error.message, "line "),
            column: position(&error.message, "column "),
            message: error.message,
        }
    }
}
fn position(message: &str, marker: &str) -> Option<usize> {
    message
        .split_once(marker)?
        .1
        .split(|character: char| !character.is_ascii_digit())
        .next()?
        .parse()
        .ok()
}
fn io(error: impl std::fmt::Display) -> Refusal {
    Refusal::Io {
        message: error.to_string(),
    }
}
fn forbidden(reason: impl Into<String>) -> Refusal {
    Refusal::NotAllowed {
        reason: reason.into(),
    }
}

/// Exact definition contents, keyed by project-relative file.
pub fn read(project: &Path) -> Result<BasedOn, Refusal> {
    let root = crate::platform::canonicalize(project).map_err(io)?;
    let sources = discovery::sources(&root)?;
    let files = sources
        .into_iter()
        .map(|(file, source)| {
            (
                file.strip_prefix(&root)
                    .expect("definition below project")
                    .to_owned(),
                Some(source),
            )
        })
        .collect::<BasedOn>();
    Ok(files)
}
fn disk(file: &Path) -> Result<Option<String>, Refusal> {
    match read_regular_text(file) {
        Ok(source) => Ok(Some(source)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(io(error)),
    }
}

// Give a stale edit its current disk text even when the edit's other preconditions now fail.
// The final comparison below remains authoritative and covers every actually changed file.
fn compare_named_bases(
    root: &Path,
    based_on: &BasedOn,
    operation: &Operation,
) -> Result<(), Refusal> {
    let names = match operation {
        Operation::Create { name, .. }
        | Operation::SetPath { name, .. }
        | Operation::SaveSource { name, .. }
        | Operation::Delete { name }
        | Operation::SetComment { name, .. } => vec![name.clone()],
        Operation::AddPart { whole, .. } | Operation::RemovePart { whole, .. } => {
            vec![whole.clone()]
        }
        Operation::MovePart { from, to, .. } => from.iter().chain(to.iter()).cloned().collect(),
        Operation::Rename { old, new } => vec![old.clone(), new.clone()],
        Operation::MoveComment { from, to } => vec![from.clone(), to.clone()],
        Operation::SavePlaces { places } => places
            .keys()
            .filter_map(|key| key.split('/').next()?.parse::<ArtifactName>().ok())
            .collect(),
        Operation::ResetPlaces { whole } => whole.iter().cloned().collect(),
    };
    for name in &names {
        discovery::definition_name(name).map_err(forbidden)?;
    }
    for (file, expected) in based_on.iter().filter(|(file, _)| {
        names
            .iter()
            .any(|name| file.file_stem().and_then(|stem| stem.to_str()) == Some(name.as_str()))
    }) {
        if !file.starts_with(discovery::DEFINITION_DIR)
            || file.extension().is_none_or(|extension| extension != "artf")
        {
            return Err(forbidden(
                "Base files must be definition paths below .artifactize/.",
            ));
        }
        crate::scope::logical_path(&crate::platform::path_text(file))
            .map_err(|error| forbidden(error.to_string()))?;
        let absolute = root.join(file);
        crate::platform::program::reject_link_ancestors(&absolute).map_err(forbidden)?;
        let current = disk(&absolute)?;
        if &current != expected {
            return Err(Refusal::Changed {
                file: file.clone(),
                current,
            });
        }
    }
    Ok(())
}

fn compare_changes(
    root: &Path,
    based_on: &BasedOn,
    changes: &BTreeMap<PathBuf, Option<String>>,
) -> Result<(), Refusal> {
    for file in changes.keys() {
        let relative = file.strip_prefix(root).expect("project file");
        let current = disk(file)?;
        let expected = based_on.get(relative).ok_or_else(|| {
            forbidden(format!(
                "No base content was supplied for {}.",
                crate::platform::path_text(relative)
            ))
        })?;
        if &current != expected {
            return Err(Refusal::Changed {
                file: relative.to_owned(),
                current,
            });
        }
    }
    Ok(())
}

/// Apply one validated operation. An absent entry in `based_on` is never an implicit CAS.
pub fn apply(project: &Path, based_on: &BasedOn, operation: Operation) -> Result<BasedOn, Refusal> {
    let root = crate::platform::canonicalize(project).map_err(io)?;
    let state = crate::store::state_dir(None).map_err(io)?;
    let locks =
        crate::workspace::prepare_directory(&state.join("definition-locks"), &root).map_err(io)?;
    use sha2::Digest;
    let identity = sha2::Sha256::digest(crate::platform::path_text(&root).as_bytes());
    let identity = identity
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let lock_path = locks.join(format!("{identity}.lock"));
    let lock = crate::platform::open_no_follow(
        std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false),
        &lock_path,
    )
    .map_err(io)?;
    lock.lock().map_err(io)?;
    let sources = discovery::sources(&root)?;
    compare_named_bases(&root, based_on, &operation)?;
    let mut proposed = sources.clone();
    // Source can repair a broken definition after its current text has been reloaded.
    // Every operation still validates the complete prospective project before writing.
    let config = if let Operation::SaveSource { name, source } = &operation {
        let file = sources
            .keys()
            .find(|file| file.file_stem().and_then(|stem| stem.to_str()) == Some(name.as_str()))
            .ok_or_else(|| forbidden(format!("Unknown Artifact {name}.")))?;
        proposed.insert(file.clone(), source.clone());
        discovery::read_sources(root.clone(), &proposed)?
    } else {
        discovery::read_sources(root.clone(), &sources)?
    };
    match operation {
        Operation::Create { name, path } => {
            discovery::definition_name(&name).map_err(forbidden)?;
            if config
                .artifacts
                .keys()
                .any(|existing| existing.eq_ignore_ascii_case(&name))
            {
                return Err(forbidden(format!("Artifact {name} already exists.")));
            }
            let mut doc = toml_edit::DocumentMut::new();
            doc["path"] = toml_edit::value(path);
            proposed.insert(
                root.join(format!(".artifactize/{name}.artf")),
                doc.to_string(),
            );
        }
        Operation::SetPath { name, path } => update(&config, &mut proposed, &name, |doc, _| {
            if doc["path"].as_str() != Some(path.as_str()) {
                let decor = doc["path"]
                    .as_value()
                    .expect("validated PATH")
                    .decor()
                    .clone();
                let mut value = toml_edit::Value::from(path);
                *value.decor_mut() = decor;
                doc["path"] = toml_edit::Item::Value(value);
            }
            Ok(())
        })?,
        Operation::SaveSource { .. } => {}
        Operation::AddPart { whole, part, at } => {
            let at = at.or_else(|| placement(&config, &proposed, &part, None));
            check_style(&config, &mut proposed, &whole)?;
            check_style(&config, &mut proposed, &part)?;
            update(&config, &mut proposed, &whole, |doc, _| {
                change_part(doc, &part, true)
            })?;
            let next = discovery::read_sources(root.clone(), &proposed)?;
            update(&config, &mut proposed, &whole, |doc, file| {
                annotations::set_parts(
                    doc,
                    file,
                    &next,
                    &whole,
                    &at.map(|at| [(part.clone(), at)].into()).unwrap_or_default(),
                    false,
                )
            })?;
            update(&config, &mut proposed, &part, |doc, file| {
                annotations::set_at(doc, file, &next, &part, None)
            })?;
        }
        Operation::RemovePart { whole, part } => {
            check_style(&config, &mut proposed, &whole)?;
            update(&config, &mut proposed, &whole, |doc, _| {
                change_part(doc, &part, false)
            })?;
            let next = discovery::read_sources(root.clone(), &proposed)?;
            update(&config, &mut proposed, &whole, |doc, file| {
                annotations::set_parts(doc, file, &next, &whole, &BTreeMap::new(), false)
            })?;
        }
        Operation::MovePart { part, from, to, at } => {
            let at = at.or_else(|| placement(&config, &proposed, &part, from.as_ref()));
            check_style(&config, &mut proposed, &part)?;
            for whole in from.iter().chain(to.iter()) {
                check_style(&config, &mut proposed, whole)?;
            }
            if from == to {
                return Err(forbidden("The source and destination whole are the same."));
            }
            if let Some(from) = &from {
                update(&config, &mut proposed, from, |doc, _| {
                    change_part(doc, &part, false)
                })?;
            }
            if let Some(to) = &to {
                update(&config, &mut proposed, to, |doc, _| {
                    change_part(doc, &part, true)
                })?;
            }
            let next = discovery::read_sources(root.clone(), &proposed)?;
            if let Some(from) = from {
                update(&config, &mut proposed, &from, |doc, file| {
                    annotations::set_parts(doc, file, &next, &from, &BTreeMap::new(), false)
                })?;
            }
            if let Some(to) = &to {
                update(&config, &mut proposed, to, |doc, file| {
                    annotations::set_parts(
                        doc,
                        file,
                        &next,
                        to,
                        &at.map(|at| [(part.clone(), at)].into()).unwrap_or_default(),
                        false,
                    )
                })?;
            }
            update(&config, &mut proposed, &part, |doc, file| {
                annotations::set_at(
                    doc,
                    file,
                    &next,
                    &part,
                    if to.is_none() { at } else { None },
                )
            })?;
        }
        Operation::Rename { old, new } => {
            discovery::definition_name(&new).map_err(forbidden)?;
            if config
                .artifacts
                .keys()
                .any(|existing| existing.eq_ignore_ascii_case(&new))
            {
                return Err(forbidden(format!("Artifact {new} already exists.")));
            }
            let old_file = node_file(&config, &old)?;
            for (owner, artifact) in &config.artifacts {
                let file = root.join(artifact.declaration_path());
                let mut doc = proposed[&file]
                    .parse::<toml_edit::DocumentMut>()
                    .expect("validated source");
                rename_item(doc.as_item_mut(), &old, &new, artifact, &mut Vec::new());
                if artifact.parts.contains(&old) {
                    annotations::rename_part(&mut doc, &file, &config, owner, &old, &new)?;
                }
                proposed.insert(file, doc.to_string());
            }
            let source = proposed.remove(&old_file).expect("known node");
            proposed.insert(old_file.with_file_name(format!("{new}.artf")), source);
        }
        Operation::Delete { name } => {
            let file = node_file(&config, &name)?;
            let users = config
                .artifacts
                .iter()
                .filter_map(|(other, artifact)| {
                    if other == &name {
                        return None;
                    }
                    let mut doc = sources[&root.join(artifact.declaration_path())]
                        .parse::<toml_edit::DocumentMut>()
                        .expect("validated source");
                    let original = doc.to_string();
                    rename_item(
                        doc.as_item_mut(),
                        &name,
                        "renamed-node",
                        artifact,
                        &mut Vec::new(),
                    );
                    (doc.to_string() != original).then(|| other.to_string())
                })
                .collect::<Vec<_>>();
            if !users.is_empty() {
                return Err(forbidden(format!(
                    "{name} is still named by: {}.",
                    users.join(", ")
                )));
            }
            proposed.remove(&file);
        }
        Operation::SavePlaces { places } => {
            let mut boxes = BTreeMap::<ArtifactName, BTreeMap<ArtifactName, [f64; 2]>>::new();
            for (key, at) in places {
                if let Some((whole, part)) = key.split_once('/') {
                    let whole = discovery::definition_name(whole).map_err(forbidden)?;
                    let part = discovery::definition_name(part).map_err(forbidden)?;
                    if config.artifacts.contains_key(&whole) {
                        boxes.entry(whole).or_default().insert(part, at);
                    }
                } else {
                    let name = discovery::definition_name(&key).map_err(forbidden)?;
                    if config.artifacts.contains_key(&name) {
                        update(&config, &mut proposed, &name, |doc, file| {
                            annotations::set_at(doc, file, &config, &name, Some(at))
                        })?;
                    }
                }
            }
            for (whole, places) in boxes {
                update(&config, &mut proposed, &whole, |doc, file| {
                    annotations::set_parts(doc, file, &config, &whole, &places, false)
                })?;
            }
        }
        Operation::ResetPlaces { whole } => {
            if let Some(whole) = whole {
                update(&config, &mut proposed, &whole, |doc, file| {
                    annotations::set_parts(doc, file, &config, &whole, &BTreeMap::new(), true)
                })?;
            } else {
                for name in config.artifacts.keys().filter(|name| {
                    !config
                        .artifacts
                        .values()
                        .any(|artifact| artifact.parts.contains(*name))
                }) {
                    update(&config, &mut proposed, name, |doc, file| {
                        annotations::set_at(doc, file, &config, name, None)
                    })?;
                }
            }
        }
        Operation::SetComment { name, text } => {
            update(&config, &mut proposed, &name, |doc, file| {
                annotations::set_comment(doc, file, &text)
            })?
        }
        Operation::MoveComment { from, to } => {
            if from == to {
                return Err(forbidden("A comment already belongs to this node."));
            }
            let from_file = node_file(&config, &from)?;
            let to_file = node_file(&config, &to)?;
            let source = proposed[&from_file]
                .parse::<toml_edit::DocumentMut>()
                .expect("validated source");
            let target = proposed[&to_file]
                .parse::<toml_edit::DocumentMut>()
                .expect("validated source");
            let text = annotations::take_comment(&source, &from_file)?
                .ok_or_else(|| forbidden(format!("{from} has no comment to move.")))?;
            if annotations::take_comment(&target, &to_file)?.is_some() {
                return Err(forbidden(format!("{to} already has a comment.")));
            }
            update(&config, &mut proposed, &from, |doc, file| {
                annotations::set_comment(doc, file, "")
            })?;
            update(&config, &mut proposed, &to, |doc, file| {
                annotations::set_comment(doc, file, &text)
            })?;
        }
    }
    let changes = sources
        .keys()
        .chain(proposed.keys())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .filter(|&file| sources.get(file) != proposed.get(file))
        .map(|file| (file.clone(), proposed.get(file).cloned()))
        .collect::<BTreeMap<_, _>>();
    compare_changes(&root, based_on, &changes)?;
    discovery::read_sources(root.clone(), &proposed)?;
    let originals = changes
        .keys()
        .map(|file| Ok((file.clone(), disk(file)?)))
        .collect::<Result<BTreeMap<_, _>, Refusal>>()?;
    let mut staged = Vec::new();
    for (file, source) in &changes {
        if let Some(source) = source {
            let mut temp =
                tempfile::NamedTempFile::new_in(file.parent().expect("definition directory"))
                    .map_err(io)?;
            std::io::Write::write_all(&mut temp, source.as_bytes()).map_err(io)?;
            staged.push((file.clone(), temp));
        }
    }
    // Validation and staging may take time: compare again immediately before persistence.
    compare_changes(&root, based_on, &changes)?;
    let written = (|| -> Result<(), Refusal> {
        for (file, temp) in staged {
            temp.persist(&file).map_err(io)?;
        }
        for (file, source) in &changes {
            if source.is_none() {
                std::fs::remove_file(file).map_err(io)?;
            }
        }
        Ok(())
    })();
    if let Err(error) = written {
        for (file, source) in originals {
            if let Some(source) = source {
                std::fs::write(file, source).map_err(io)?;
            } else if file.exists() {
                std::fs::remove_file(file).map_err(io)?;
            }
        }
        return Err(error);
    }
    Ok(changes
        .into_iter()
        .map(|(file, source)| {
            (
                file.strip_prefix(&root).expect("project file").to_owned(),
                source,
            )
        })
        .collect())
}
fn change_part(
    doc: &mut toml_edit::DocumentMut,
    part: &ArtifactName,
    add: bool,
) -> Result<(), Refusal> {
    if doc.get("parts").is_none() {
        if !add {
            return Err(forbidden(format!("{part} is not a part of this Artifact.")));
        }
        doc["parts"] = toml_edit::value(toml_edit::Array::new());
    }
    let parts = doc["parts"].as_array_mut().expect("validated parts");
    let index = parts
        .iter()
        .position(|value| value.as_str() == Some(part.as_str()));
    match (add, index) {
        (true, None) => parts.push(part.as_str()),
        (false, Some(index)) => {
            parts.remove(index);
        }
        (true, Some(_)) => {
            return Err(forbidden(format!(
                "{part} is already a part of this Artifact."
            )));
        }
        (false, None) => return Err(forbidden(format!("{part} is not a part of this Artifact."))),
    }
    if parts.is_empty() {
        doc.remove("parts");
    }
    Ok(())
}
fn rename_item(
    item: &mut toml_edit::Item,
    old: &str,
    new: &str,
    artifact: &Artifact,
    path: &mut Vec<String>,
) {
    if path.first().is_some_and(|key| key == "annotations") {
        return;
    }
    match item {
        toml_edit::Item::Value(value) => rename_value(value, old, new, artifact, path),
        toml_edit::Item::Table(table) => {
            for (key, item) in table.iter_mut() {
                path.push(key.to_string());
                rename_item(item, old, new, artifact, path);
                path.pop();
            }
        }
        toml_edit::Item::ArrayOfTables(tables) => {
            for table in tables.iter_mut() {
                for (key, item) in table.iter_mut() {
                    path.push(key.to_string());
                    rename_item(item, old, new, artifact, path);
                    path.pop();
                }
            }
        }
        toml_edit::Item::None => (),
    }
}
fn rename_value(
    value: &mut toml_edit::Value,
    old: &str,
    new: &str,
    artifact: &Artifact,
    path: &mut Vec<String>,
) {
    match value {
        toml_edit::Value::String(text) => {
            let original = text.value();
            let updated = if path.as_slice() == ["parts"] && original == old {
                new.to_owned()
            } else if let Some(field) = reference_field(path, artifact) {
                match field {
                    ReferenceField::Instruction => rename_references(original, old, new),
                    ReferenceField::Arguments => rename_argument(original, old, new),
                    ReferenceField::HumanArguments if old != "artifactPath" => {
                        rename_argument(original, old, new)
                    }
                    ReferenceField::HumanArguments => original.clone(),
                }
            } else {
                return;
            };
            if updated != *original {
                let decor = value.decor().clone();
                *value = toml_edit::Value::from(updated);
                *value.decor_mut() = decor;
            }
        }
        toml_edit::Value::Array(array) => {
            for value in array.iter_mut() {
                rename_value(value, old, new, artifact, path);
            }
        }
        toml_edit::Value::InlineTable(table) => {
            for (key, value) in table.iter_mut() {
                path.push(key.to_string());
                rename_value(value, old, new, artifact, path);
                path.pop();
            }
        }
        _ => (),
    }
}
enum ReferenceField {
    Instruction,
    Arguments,
    HumanArguments,
}
fn reference_field(path: &[String], artifact: &Artifact) -> Option<ReferenceField> {
    let keys = path.iter().map(String::as_str).collect::<Vec<_>>();
    match keys.as_slice() {
        ["evals", _, "payload", "instruction"] => Some(ReferenceField::Instruction),
        ["evals", _, "profile", "args"]
        | ["evals", _, "profile_variants", _, "args"]
        | ["views", "agent_sessions", _, "start" | "stop", "args"]
        | ["fingerprint", "script", "args"] => Some(ReferenceField::Arguments),
        ["views", "agent_tools", tool, "args"] => match artifact.views.agent_tools.get(*tool) {
            Some(AgentTool::Command(tool)) if tool.protocol == ToolProtocol::Json => {
                Some(ReferenceField::Arguments)
            }
            _ => None, // Plain placeholders and fixed builtin operands are not Artifact references.
        },
        ["views", "human_tools", tool, "args"] => match artifact.views.human_tools.get(*tool) {
            Some(HumanTool::Builtin(tool)) if tool.builtin == Builtin::Help => None,
            _ => Some(ReferenceField::HumanArguments),
        },
        _ => None,
    }
}
fn rename_argument(text: &str, old: &str, new: &str) -> String {
    match text.parse::<crate::scope::Argument>() {
        Ok(crate::scope::Argument::Reference { prefix, name, .. }) if name.as_str() == old => {
            let start = prefix.len() + 1;
            format!("{}{}{}", &text[..start], new, &text[start + old.len()..])
        }
        _ => text.to_owned(),
    }
}
fn rename_references(text: &str, old: &str, new: &str) -> String {
    let mut result = String::new();
    let mut start = 0;
    for reference in crate::scope::reference_spans(text)
        .into_iter()
        .filter(|reference| reference.name.as_str() == old)
    {
        result.push_str(&text[start..reference.start]);
        result.push('{');
        result.push_str(new);
        result.push('}');
        start = reference.end;
    }
    result.push_str(&text[start..]);
    result
}
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Layout {
    pub nodes: BTreeMap<String, [f64; 2]>,
}
/// Branch-current places. The top level is hand-placed when a plain node key exists;
/// a box is hand-placed when a `whole/part` key exists. No layout file is read.
pub fn read_layout(project: &Path) -> Result<Layout, Refusal> {
    let read = read_project(project)?;
    let mut layout = Layout::default();
    for (name, node) in read.nodes {
        if let Some(at) = node.annotations.at {
            layout.nodes.insert(name.to_string(), at);
        }
        for (part, at) in node.annotations.parts {
            layout.nodes.insert(format!("{name}/{part}"), at);
        }
    }
    Ok(layout)
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Node {
    #[serde(with = "crate::platform::path_serde")]
    pub definition: PathBuf,
    pub identity: crate::types::Sha256Digest,
    /// Authored PATH, including its kind-bearing trailing slash or URI.
    pub target: String,
    #[serde(with = "crate::platform::path_serde")]
    pub path: PathBuf,
    pub kind: ArtifactKind,
    pub target_missing: bool,
    pub annotations: Annotations,
}
#[derive(Debug, Serialize)]
pub struct Project {
    pub files: BasedOn,
    pub nodes: BTreeMap<ArtifactName, Node>,
}
/// A file's exact content is its optimistic base; `identity` is SHA-256 for view clients.
/// Invalid annotation values are ignored for display and described in their error fields.
pub fn read_project(project: &Path) -> Result<Project, Refusal> {
    let root = crate::platform::canonicalize(project).map_err(io)?;
    let sources = discovery::sources(&root)?;
    let config = discovery::read_sources(root.clone(), &sources)?;
    let files = sources
        .into_iter()
        .map(|(file, source)| {
            (
                file.strip_prefix(&root)
                    .expect("definition below project")
                    .to_owned(),
                Some(source),
            )
        })
        .collect::<BasedOn>();
    let nodes = config
        .artifacts
        .iter()
        .map(|(name, artifact)| {
            let definition = artifact.declaration_path();
            let source = files[&definition].as_deref().expect("definition text");
            let identity = crate::types::Sha256Digest::of(source.as_bytes());
            (
                name.clone(),
                Node {
                    definition,
                    identity,
                    target: source
                        .parse::<toml_edit::DocumentMut>()
                        .expect("validated source")["path"]
                        .as_str()
                        .expect("validated PATH")
                        .to_owned(),
                    path: artifact.path.clone(),
                    kind: artifact.kind,
                    target_missing: artifact.target_error.is_some(),
                    annotations: annotations::read(source, &config, name),
                },
            )
        })
        .collect();
    Ok(Project { files, nodes })
}
fn node_file(config: &RepoConfig, name: &ArtifactName) -> Result<PathBuf, Refusal> {
    config
        .artifacts
        .get(name)
        .map(|artifact| config.root.join(artifact.declaration_path()))
        .ok_or_else(|| forbidden(format!("Unknown Artifact {name}.")))
}
fn update(
    config: &RepoConfig,
    proposed: &mut BTreeMap<PathBuf, String>,
    name: &ArtifactName,
    edit: impl FnOnce(&mut toml_edit::DocumentMut, &Path) -> Result<(), Refusal>,
) -> Result<(), Refusal> {
    let file = node_file(config, name)?;
    let mut doc = proposed[&file]
        .parse::<toml_edit::DocumentMut>()
        .expect("validated source");
    edit(&mut doc, &file)?;
    proposed.insert(file, doc.to_string());
    Ok(())
}

fn check_style(
    config: &RepoConfig,
    proposed: &mut BTreeMap<PathBuf, String>,
    name: &ArtifactName,
) -> Result<(), Refusal> {
    update(config, proposed, name, |doc, file| {
        annotations::validate_style(doc, file, config, name)
    })
}

fn placement(
    config: &RepoConfig,
    proposed: &BTreeMap<PathBuf, String>,
    part: &ArtifactName,
    whole: Option<&ArtifactName>,
) -> Option<[f64; 2]> {
    let owner = whole.unwrap_or(part);
    let file = node_file(config, owner).ok()?;
    let annotations = annotations::read(proposed.get(&file)?, config, owner);
    if whole.is_some() {
        annotations.parts.get(part).copied()
    } else {
        annotations.at
    }
}
