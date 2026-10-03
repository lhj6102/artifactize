//! Mounts, aliases, artifact references, and canonical scoped paths.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::CString;
use std::fs::{self, File};
use std::os::{
    fd::{AsRawFd, FromRawFd},
    unix::ffi::OsStrExt,
};
use std::path::{Component, Path, PathBuf};

use serde::Serialize;
use thiserror::Error;

use crate::config::{Artifact, CONFIG_FILE, ConfigError, Eval, Profile, RepoConfig};

mod instruction;
pub use instruction::instruction_references;

#[derive(Debug, Error)]
#[error("{0}")]
pub struct ScopeError(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Relation {
    /// Input Artifact.
    pub source: String,
    /// Consumer Artifact.
    pub target: String,
    #[serde(flatten)]
    pub kind: RelationKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum RelationKind {
    Child {
        path: String,
    },
    Mount {
        alias: String,
    },
    Instruction {
        #[serde(rename = "evalId")]
        eval_id: String,
        name: String,
    },
    #[serde(rename = "argv")]
    Argument {
        #[serde(rename = "evalId")]
        eval_id: String,
        index: usize,
        name: String,
        path: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopedPath {
    pub artifact_id: String,
    pub path: String,
}

#[derive(Debug)]
pub struct Scope<'a> {
    pub artifacts: BTreeMap<&'a str, &'a Artifact>,
}

impl Scope<'_> {
    /// Pure logical resolution. Each mount or child hop consumes path components.
    pub fn resolve_path(&self, artifact_id: &str, path: &str) -> Result<ScopedPath, ScopeError> {
        self.resolve_path_inner(artifact_id, path, false)
    }

    pub(crate) fn resolve_listing(
        &self,
        artifact_id: &str,
        path: &str,
    ) -> Result<ScopedPath, ScopeError> {
        self.resolve_path_inner(artifact_id, path, true)
    }

    fn resolve_path_inner(
        &self,
        artifact_id: &str,
        path: &str,
        listing: bool,
    ) -> Result<ScopedPath, ScopeError> {
        logical_path(path)?;
        let mut current = artifact_id;
        let mut remaining = path;
        loop {
            let artifact = self
                .artifacts
                .get(current)
                .ok_or_else(|| ScopeError(format!("Artifact is outside this review: {current}")))?;
            if remaining.is_empty() {
                break;
            }
            let (first, rest) = remaining.split_once('/').unwrap_or((remaining, ""));
            if let Some(target) = artifact.mounts.get(first) {
                current = target;
                remaining = rest;
                continue;
            }
            if let Some((prefix, target)) = artifact.children.iter().find(|(prefix, _)| {
                remaining == prefix.as_str()
                    || remaining
                        .strip_prefix(prefix.as_str())
                        .is_some_and(|rest| rest.starts_with('/'))
            }) {
                current = target;
                remaining = remaining[prefix.len()..].strip_prefix('/').unwrap_or("");
                continue;
            }
            for (child, id) in &artifact.children {
                if let Some(family) = self
                    .artifacts
                    .get(id.as_str())
                    .and_then(|child| child.family.as_ref())
                    && let Some(folder) = child.strip_suffix(&format!("/{id}"))
                    && (remaining == folder
                        || remaining
                            .strip_prefix(folder)
                            .is_some_and(|rest| rest.starts_with('/')))
                {
                    if listing && remaining == folder {
                        break;
                    }
                    return Err(ScopeError(format!(
                        "Artifact path is inside the folder of Artifact family {}; address one of its instances as {folder}/<instance>/<path>.",
                        family.name
                    )));
                }
            }
            break;
        }
        Ok(ScopedPath {
            artifact_id: current.to_owned(),
            path: remaining.to_owned(),
        })
    }

    /// Existing file or directory, without traversing links even inside the owner.
    pub fn resolve_input(
        &self,
        root: &Path,
        artifact_id: &str,
        path: &str,
    ) -> Result<PathBuf, ScopeError> {
        let location = self.resolve_path(artifact_id, path)?;
        let artifact = self.artifacts[location.artifact_id.as_str()];
        let owner = scoped_path(root, &artifact.path)?;
        scoped_path(&owner, Path::new(&location.path))
    }
}

/// Open each component relative to its pinned parent, so replacement cannot redirect a read through a link.
pub(crate) fn open_input(root: &Path, artifact: &Artifact, path: &str) -> Result<File, ScopeError> {
    logical_path(path)?;
    if !root.is_absolute() || artifact.path.is_absolute() {
        return Err(ScopeError(
            "Artifact roots must be absolute and owner paths relative.".into(),
        ));
    }
    let target = root.join(&artifact.path).join(path);
    let mut directory = File::open("/").map_err(|e| ScopeError(e.to_string()))?;
    for component in target.components() {
        let name = match component {
            Component::RootDir => continue,
            Component::Normal(name) => name,
            _ => {
                return Err(ScopeError(
                    "Artifact path must not traverse parent directories.".into(),
                ));
            }
        };
        let name = CString::new(name.as_bytes())
            .map_err(|_| ScopeError("Invalid Artifact path.".into()))?;
        // O_NONBLOCK avoids waiting on a FIFO before its type can be rejected.
        let fd = unsafe {
            libc::openat(
                directory.as_raw_fd(),
                name.as_ptr(),
                libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC,
            )
        };
        if fd < 0 {
            return Err(ScopeError(
                "Cannot open Artifact input without symlink traversal.".into(),
            ));
        }
        directory = unsafe { File::from_raw_fd(fd) };
        let metadata = directory
            .metadata()
            .map_err(|e| ScopeError(e.to_string()))?;
        if !metadata.is_file() && !metadata.is_dir() {
            return Err(ScopeError(
                "Artifact input must be a regular file or directory.".into(),
            ));
        }
    }
    Ok(directory)
}

/// Composition grants access; a referenced Artifact's Eval instructions do not.
pub fn artifact_scope<'a>(config: &'a RepoConfig, roots: &[&str]) -> Result<Scope<'a>, ScopeError> {
    let mut artifacts = BTreeMap::new();
    let mut pending = roots.to_vec();
    while let Some(id) = pending.pop() {
        let (id, artifact) = config
            .artifacts
            .get_key_value(id)
            .ok_or_else(|| ScopeError(format!("Unknown Artifact: {id}")))?;
        if artifacts.insert(id.as_str(), artifact).is_some() {
            continue;
        }
        pending.extend(artifact.children.values().map(String::as_str));
        pending.extend(artifact.mounts.values().map(String::as_str));
    }
    Ok(Scope { artifacts })
}

pub fn eval_scope<'a>(config: &'a RepoConfig, eval: &Eval) -> Result<Scope<'a>, ScopeError> {
    let roots: Vec<_> = std::iter::once(eval.target.as_str())
        .chain(eval.deps.iter().map(String::as_str))
        .collect();
    artifact_scope(config, &roots)
}

fn logical_path(path: &str) -> Result<(), ScopeError> {
    if path.encode_utf16().count() > 4096
        || path
            .bytes()
            .any(|byte| byte.is_ascii_control() || matches!(byte, b'\\' | b':'))
        || (!path.is_empty()
            && path
                .split('/')
                .any(|part| part.is_empty() || part == "." || part == ".."))
    {
        return Err(ScopeError(
            "Artifact path must be a safe relative logical path.".into(),
        ));
    }
    Ok(())
}

/// Resolve a physical input below a canonical root. No component may be a symlink.
pub fn scoped_path(root: &Path, path: &Path) -> Result<PathBuf, ScopeError> {
    let path = path
        .to_str()
        .ok_or_else(|| ScopeError("Artifact paths must be UTF-8.".into()))?;
    if path.contains(['\0', '\\'])
        || Path::new(path).is_absolute()
        || path.split('/').any(|part| part == "." || part == "..")
    {
        return Err(ScopeError(
            "Artifact path must be relative to its declared root.".into(),
        ));
    }
    let mut target = root.to_owned();
    for component in path.split('/').filter(|part| !part.is_empty()) {
        target.push(component);
        let metadata = fs::symlink_metadata(&target)
            .map_err(|error| ScopeError(format!("{}: {error}", target.display())))?;
        if metadata.is_symlink() {
            return Err(ScopeError("Artifact symlinks are not supported.".into()));
        }
    }
    let actual = fs::canonicalize(&target)
        .map_err(|error| ScopeError(format!("{}: {error}", target.display())))?;
    if !actual.starts_with(root) {
        return Err(ScopeError(
            "Artifact path escapes its declared root.".into(),
        ));
    }
    let metadata = fs::metadata(&actual)
        .map_err(|error| ScopeError(format!("{}: {error}", actual.display())))?;
    if !metadata.is_file() && !metadata.is_dir() {
        return Err(ScopeError(
            "Artifact input must be a file or directory.".into(),
        ));
    }
    Ok(actual)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstructionPart {
    Text(String),
    Artifact(String),
}

/// Presentation tokens only: never expand content or mutate the authored payload.
pub fn parse_artifact_instruction(
    source: &str,
    scope: &Scope<'_>,
    references: &BTreeMap<String, String>,
) -> Vec<InstructionPart> {
    let mut parts = Vec::new();
    let mut start = 0;
    for reference in instruction::references(source) {
        let id = references
            .get(reference.name)
            .map_or(reference.name, String::as_str);
        if !scope.artifacts.contains_key(id) {
            continue;
        }
        if start < reference.start {
            parts.push(InstructionPart::Text(source[start..reference.start].into()));
        }
        parts.push(InstructionPart::Artifact(id.into()));
        start = reference.end;
    }
    if start < source.len() {
        parts.push(InstructionPart::Text(source[start..].into()));
    }
    parts
}

struct ArgumentReference<'a> {
    name: &'a str,
    prefix: &'a str,
    path: &'a str,
}

fn argument_reference(argument: &str) -> Result<Option<ArgumentReference<'_>>, ScopeError> {
    let references = instruction::references(argument);
    let Some(reference) = references.first() else {
        return Ok(None);
    };
    let prefix = &argument[..reference.start];
    let flag = prefix
        .strip_prefix("--")
        .and_then(|flag| flag.strip_suffix('='))
        .is_some_and(|flag| {
            !flag.is_empty()
                && flag
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        });
    if references.len() != 1 || (!prefix.is_empty() && !flag) {
        return Err(ScopeError(
            "Artifact arguments must be {name}[/path] or --flag={name}[/path].".into(),
        ));
    }
    let suffix = &argument[reference.end..];
    let path = if suffix.is_empty() {
        ""
    } else {
        suffix
            .strip_prefix('/')
            .filter(|path| !path.is_empty())
            .ok_or_else(|| {
                ScopeError("Artifact argument suffix must be a nonempty /path.".into())
            })?
    };
    logical_path(path)?;
    Ok(Some(ArgumentReference {
        name: reference.name,
        prefix,
        path,
    }))
}

fn reference_target<'a>(
    config: &'a RepoConfig,
    owner: &str,
    name: &str,
) -> Result<&'a str, ScopeError> {
    let artifact = config
        .artifacts
        .get(owner)
        .ok_or_else(|| ScopeError(format!("Unknown Artifact: {owner}")))?;
    let target = artifact.mounts.get(name).map_or(name, String::as_str);
    if config.families.contains_key(target) {
        return Err(ScopeError(format!(
            "Reference {{{name}}} names an Artifact family; reference one of its instances."
        )));
    }
    config
        .artifacts
        .get_key_value(target)
        .map(|(id, _)| id.as_str())
        .ok_or_else(|| {
            ScopeError(format!(
                "Unknown Artifact reference {{{name}}}. Escape literal braces with a backslash."
            ))
        })
}

/// Resolve `{name}[/path]` and `--flag={name}[/path]` operands to existing scoped inputs.
/// Other arguments, including escaped braces, stay literal. Never interpolate the command.
/// Call at execution preparation, after static config validation; it opens input metadata.
pub fn resolve_argv(
    config: &RepoConfig,
    scope: &Scope<'_>,
    owner: &str,
    args: &[String],
) -> Result<Vec<String>, ScopeError> {
    args.iter()
        .map(|argument| {
            let Some(reference) = argument_reference(argument)? else {
                return Ok(argument.clone());
            };
            let id = reference_target(config, owner, reference.name)?;
            let path = scope.resolve_input(&config.root, id, reference.path)?;
            let path = path
                .to_str()
                .ok_or_else(|| ScopeError("Artifact paths must be UTF-8.".into()))?;
            Ok(format!("{}{path}", reference.prefix))
        })
        .collect()
}

/// Static relationships only; scheduling and dependency closure belong to graph.
pub(crate) fn resolve_config(config: &mut RepoConfig) -> Result<(), ConfigError> {
    let mut relations = Vec::new();
    for (id, artifact) in &config.artifacts {
        let error = |message: String| {
            ConfigError::new(config.root.join(&artifact.path).join(CONFIG_FILE), message)
        };
        for (path, source) in &artifact.children {
            relations.push(Relation {
                source: source.clone(),
                target: id.clone(),
                kind: RelationKind::Child { path: path.clone() },
            });
        }
        for (alias, source) in &artifact.mounts {
            if config.families.contains_key(source) {
                return Err(error(format!(
                    "Mount target {source} in {id} is an Artifact family; mount one of its instances."
                )));
            }
            if !config.artifacts.contains_key(source) {
                return Err(error(format!("Unknown mount target {source} in {id}.")));
            }
            if (config.artifacts.contains_key(alias) || config.families.contains_key(alias))
                && alias != source
            {
                return Err(error(format!("Ambiguous mount alias {alias} in {id}.")));
            }
            match fs::symlink_metadata(config.root.join(&artifact.path).join(alias)) {
                Ok(_) => {
                    return Err(error(format!(
                        "Mount {id}/{alias} conflicts with a physical entry."
                    )));
                }
                Err(failure) if failure.kind() == std::io::ErrorKind::NotFound => {}
                Err(failure) => return Err(error(failure.to_string())),
            }
            relations.push(Relation {
                source: source.clone(),
                target: id.clone(),
                kind: RelationKind::Mount {
                    alias: alias.clone(),
                },
            });
        }
    }
    let mut resolved = Vec::new();
    for eval in &config.evals {
        let error = |failure: ScopeError| {
            ConfigError::new(
                config
                    .root
                    .join(&config.artifacts[&eval.target].path)
                    .join(CONFIG_FILE),
                format!("Eval {}: {failure}", eval.id),
            )
        };
        let mut references = BTreeMap::new();
        let mut deps = BTreeSet::new();
        let instruction = eval.declaration.payload["instruction"].as_str().unwrap();
        for name in instruction_references(instruction) {
            let source = reference_target(config, &eval.target, name).map_err(error)?;
            references.insert(name.to_owned(), source.to_owned());
            if source != eval.target {
                deps.insert(source.to_owned());
                relations.push(Relation {
                    source: source.to_owned(),
                    target: eval.target.clone(),
                    kind: RelationKind::Instruction {
                        eval_id: eval.id.clone(),
                        name: name.to_owned(),
                    },
                });
            }
        }
        if let Profile::Runtime { args, .. } = &eval.declaration.profile {
            for (index, argument) in args.iter().enumerate() {
                let Some(reference) = argument_reference(argument).map_err(error)? else {
                    continue;
                };
                let source =
                    reference_target(config, &eval.target, reference.name).map_err(error)?;
                if source != eval.target {
                    deps.insert(source.to_owned());
                    relations.push(Relation {
                        source: source.to_owned(),
                        target: eval.target.clone(),
                        kind: RelationKind::Argument {
                            eval_id: eval.id.clone(),
                            index,
                            name: reference.name.to_owned(),
                            path: reference.path.to_owned(),
                        },
                    });
                }
            }
        }
        resolved.push((references, deps.into_iter().collect()));
    }
    for (eval, (references, deps)) in config.evals.iter_mut().zip(resolved) {
        eval.references = references;
        eval.deps = deps;
    }
    config.relations = relations;
    Ok(())
}

#[cfg(test)]
mod tests;
