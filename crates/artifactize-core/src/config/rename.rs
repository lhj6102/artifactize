//! `artifactize config rename`: give an Artifact a new name where it is bound and everywhere a
//! definition names it, as the language resolved those names, and nothing else.

use std::{collections::BTreeMap, path::PathBuf};

use super::{ArtifactName, ConfigError, discovery, lang};

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Rename {
    pub old: ArtifactName,
    pub new: ArtifactName,
    /// Each changed file, relative to the project, with how many names changed in it.
    #[serde(serialize_with = "crate::platform::path_serde::counts")]
    pub files: Vec<(PathBuf, usize)>,
    /// The definition file renamed with the Artifact, when it was named after it.
    #[serde(
        skip_serializing_if = "Option::is_none",
        serialize_with = "crate::platform::path_serde::optional_pair"
    )]
    pub moved: Option<(PathBuf, PathBuf)>,
}

/// Rename `old` to `new`, checking that the result reads before writing anything.
pub fn rename(
    repo: &std::path::Path,
    old: &ArtifactName,
    new: &ArtifactName,
) -> Result<Rename, ConfigError> {
    let root =
        crate::platform::canonicalize(repo).map_err(|error| ConfigError::new(repo, error))?;
    let sources = discovery::sources(&root)?;
    let catalog = super::plugins::catalog(&root)?;
    let program = discovery::program(&root, &sources, &catalog)?;
    let lowered = program
        .artifacts
        .iter()
        .find(|artifact| artifact.name == old.as_str())
        .ok_or_else(|| ConfigError::new(&root, format!("Unknown Artifact {old}.")))?;
    if let Some(taken) = program.artifacts.iter().find(|artifact| {
        artifact.name.eq_ignore_ascii_case(new.as_str()) && artifact.name != old.as_str()
    }) {
        return Err(ConfigError::new(
            &root,
            format!("Artifact {} is already declared.", taken.name),
        ));
    }
    let binding_file = lowered.binding.file;
    let mut edits: BTreeMap<lang::FileId, Vec<std::ops::Range<usize>>> = BTreeMap::new();
    for (span, name) in &program.names {
        if name == old.as_str() {
            edits.entry(span.file).or_default().push(span.range());
        }
    }
    let mut changed = BTreeMap::new();
    let mut files = Vec::new();
    for (file, mut ranges) in edits {
        let source = program.sources.file(file);
        if !matches!(source.origin, lang::Origin::Project) {
            return Err(ConfigError::new(
                &source.path,
                format!(
                    "Plugin {} names Artifact {old}; change the plugin instead.",
                    crate::platform::path_text(&source.path)
                ),
            ));
        }
        ranges.sort_by_key(|range| std::cmp::Reverse(range.start));
        ranges.dedup();
        let mut text = source.text.clone();
        for range in &ranges {
            text.replace_range(range.clone(), new.as_str());
        }
        files.push((source.path.clone(), ranges.len()));
        changed.insert(root.join(&source.path), text);
    }
    // A file named after the Artifact it alone binds moves with it.
    let bound = program.sources.file(binding_file);
    let only = program
        .artifacts
        .iter()
        .filter(|artifact| artifact.binding.file == binding_file)
        .count()
        == 1;
    let moved = (only
        && bound
            .path
            .file_stem()
            .is_some_and(|stem| stem == old.as_str()))
    .then(|| {
        (
            bound.path.clone(),
            bound.path.with_file_name(format!("{new}.artf")),
        )
    });
    let mut next: BTreeMap<PathBuf, String> = sources.clone();
    next.extend(changed.clone());
    if let Some((from, to)) = &moved {
        if next.contains_key(&root.join(to)) {
            return Err(ConfigError::new(
                root.join(to),
                "A definition file of that name exists.",
            ));
        }
        let text = next
            .remove(&root.join(from))
            .expect("the bound file is a source");
        next.insert(root.join(to), text);
    }
    discovery::read_sources(root.clone(), &next)?;
    for (path, text) in &changed {
        crate::platform::paths::restore_file(path, text.as_bytes())
            .map_err(|error| ConfigError::new(path, error))?;
    }
    if let Some((from, to)) = &moved {
        std::fs::rename(root.join(from), root.join(to))
            .map_err(|error| ConfigError::new(root.join(from), error))?;
    }
    Ok(Rename {
        old: old.clone(),
        new: new.clone(),
        files,
        moved,
    })
}

#[cfg(test)]
mod tests;
