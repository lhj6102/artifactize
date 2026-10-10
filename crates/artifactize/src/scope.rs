//! Mounts, aliases, artifact references, and canonical scoped paths.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::config::{Artifact, ConfigError, Eval, Fingerprint, Profile, RepoConfig};

mod human;
mod instruction;
pub(crate) use human::{resolve_human_argv, validate_human_args};
pub use instruction::instruction_references;

pub use artifactize_tools::scope::{ArtifactId, ScopeError, ScopedPath, scoped_path};
pub(crate) use artifactize_tools::scope::{logical_path, open_child, open_scoped};

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
    Dependency {
        #[serde(rename = "evalId")]
        eval_id: String,
        name: String,
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

#[derive(Debug)]
pub struct Scope<'a> {
    pub artifacts: BTreeMap<&'a str, &'a Artifact>,
}

impl Scope<'_> {
    /// Snapshot only access data; composition and eval dependencies are decided here.
    pub(crate) fn tool_scope(&self) -> artifactize_tools::scope::Scope {
        artifactize_tools::scope::Scope {
            artifacts: self
                .artifacts
                .iter()
                .map(|(id, artifact)| (tool_id(id), artifact.tool_scope()))
                .collect(),
        }
    }

    pub fn resolve_path(&self, artifact_id: &str, path: &str) -> Result<ScopedPath, ScopeError> {
        self.tool_scope()
            .resolve_path(&ArtifactId::new(artifact_id)?, path)
    }

    pub fn resolve_input(
        &self,
        root: &Path,
        artifact_id: &str,
        path: &str,
    ) -> Result<PathBuf, ScopeError> {
        self.tool_scope()
            .resolve_input(root, &ArtifactId::new(artifact_id)?, path)
    }
}

/// The tool-side ID of a configured Artifact. Configuration validates every Artifact name,
/// mount target and child, so a name that fails here is a bug in that validation.
pub(crate) fn tool_id(name: &str) -> ArtifactId {
    ArtifactId::new(name).expect("configuration validates Artifact names")
}

/// Revalidate file targets at every preparation/read boundary, without following links.
pub(crate) fn validate_file_target(root: &Path, artifact: &Artifact) -> Result<(), ScopeError> {
    if artifact.file_name().is_some() {
        let target = artifact
            .path
            .to_str()
            .ok_or_else(|| ScopeError("Artifact paths must be UTF-8.".into()))?;
        let file = open_scoped(root, target).map_err(|error| {
            ScopeError(format!(
                "File Artifact {} target {} is unavailable: {error}",
                artifact.name,
                artifact.path.display()
            ))
        })?;
        if !file
            .metadata()
            .map_err(|error| ScopeError(error.to_string()))?
            .is_file()
        {
            return Err(ScopeError(format!(
                "File Artifact {} target {} must remain a regular file.",
                artifact.name,
                artifact.path.display()
            )));
        }
    }
    Ok(())
}

fn reference_path(config: &RepoConfig, id: &str, path: &str) -> Result<(), ScopeError> {
    if config.artifacts[id].file_name().is_some() && !path.is_empty() {
        return Err(ScopeError(format!(
            "File Artifact {{{id}}} cannot have a /path suffix."
        )));
    }
    Ok(())
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

/// Validate scope-relative command paths before process lookup. Bare names and absolute
/// paths stay literal here; preflight and both launchers use the shared PATH/PATHEXT rule.
pub(crate) fn executable(
    root: &Path,
    scope: &Scope<'_>,
    owner: &str,
    command: &str,
) -> Result<std::ffi::OsString, String> {
    let artifact = scope.artifacts[owner];
    let cwd = scoped_path(root, artifact.folder()).map_err(|error| error.to_string())?;
    let program = if !Path::new(command).is_absolute() && command.contains('/') {
        let relative = command.strip_prefix("./").unwrap_or(command);
        let mounted = artifact
            .mounts
            .contains_key(relative.split('/').next().unwrap_or(""));
        let physical = if artifact.file_name().is_some() && !mounted {
            artifactize_tools::scope::logical_path(relative).map_err(|error| error.to_string())?;
            cwd.join(relative)
        } else {
            let resolved = scope
                .resolve_path(owner, relative)
                .map_err(|error| error.to_string())?;
            root.join(scope.artifacts[resolved.artifact_id.as_str()].folder())
                .join(resolved.path)
        };
        // Even an extensionless declaration must not hide a link behind PATHEXT.
        for path in physical.ancestors() {
            if path
                .symlink_metadata()
                .is_ok_and(|metadata| metadata.is_symlink())
            {
                return Err("Artifact symlinks are not supported.".into());
            }
        }
        let mut failure = None;
        artifactize_tools::program::candidates(
            Path::new(relative),
            std::env::var_os("PATHEXT").as_deref(),
        )
        .into_iter()
        .find_map(|candidate| {
            #[cfg(windows)]
            let candidate = if Path::new(relative).extension().is_none() {
                executable_spelling(root, scope, owner, &cwd, &candidate, mounted)?
            } else {
                candidate
            };
            let resolved = if artifact.file_name().is_some() && !mounted {
                scoped_path(&cwd, &candidate)
            } else {
                scope.resolve_input(root, owner, candidate.to_str()?)
            };
            match resolved {
                Ok(program) if program.is_file() => Some(program),
                Ok(_) => None,
                Err(error) => {
                    failure.get_or_insert_with(|| error.to_string());
                    None
                }
            }
        })
        .ok_or_else(|| {
            failure.unwrap_or_else(|| "Executable path is unavailable or outside scope.".into())
        })?
    } else {
        PathBuf::from(command)
    };
    Ok(program.into_os_string())
}

/// PATHEXT supplies a suffix, not a model-authored name: use its actual directory-entry
/// spelling before strict scoped validation. Parent components retain their input spelling.
#[cfg(windows)]
fn executable_spelling(
    root: &Path,
    scope: &Scope<'_>,
    owner: &str,
    cwd: &Path,
    candidate: &Path,
    mounted: bool,
) -> Option<PathBuf> {
    let parent = candidate.parent().unwrap_or(Path::new(""));
    let directory = if scope.artifacts[owner].file_name().is_some() && !mounted {
        scoped_path(cwd, parent).ok()?
    } else {
        scope.resolve_input(root, owner, parent.to_str()?).ok()?
    };
    let requested = candidate.file_name()?.to_str()?;
    std::fs::read_dir(directory)
        .ok()?
        .filter_map(Result::ok)
        .find_map(|entry| {
            let name = entry.file_name();
            name.to_str()?
                .eq_ignore_ascii_case(requested)
                .then(|| candidate.with_file_name(name))
        })
}

pub fn eval_scope<'a>(config: &'a RepoConfig, eval: &Eval) -> Result<Scope<'a>, ScopeError> {
    let roots: Vec<_> = std::iter::once(eval.target.as_str())
        .chain(eval.deps.iter().map(String::as_str))
        .collect();
    artifact_scope(config, &roots)
}

/// Admit an owner plus the explicit references in its argv, as an Eval's scope admits its deps.
pub fn argv_scope<'a>(
    config: &'a RepoConfig,
    owner: &str,
    args: &[String],
) -> Result<Scope<'a>, ScopeError> {
    let mut roots = vec![owner];
    for argument in args {
        if let Some(reference) = argument_reference(argument)? {
            roots.push(reference_target(config, owner, reference.name)?);
        }
    }
    artifact_scope(config, &roots)
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
            reference_path(config, id, reference.path)?;
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
        let error = |keys: &[&str], message: String| {
            ConfigError::declaration(config.root.join(artifact.declaration_path()), keys, message)
        };
        for (name, tool) in &artifact.views.agent_tools {
            if let crate::config::AgentTool::Command(tool) = tool
                && tool.protocol == crate::config::ToolProtocol::Json
            {
                for argument in &tool.args {
                    if let Some(reference) = argument_reference(argument).map_err(|failure| {
                        error(
                            &["views", "agent_tools", name, "args"],
                            format!("Agent tool {name}: {failure}"),
                        )
                    })? {
                        let target =
                            reference_target(config, id, reference.name).map_err(|failure| {
                                error(
                                    &["views", "agent_tools", name, "args"],
                                    format!("Agent tool {name}: {failure}"),
                                )
                            })?;
                        reference_path(config, target, reference.path).map_err(|failure| {
                            error(
                                &["views", "agent_tools", name, "args"],
                                format!("Agent tool {name}: {failure}"),
                            )
                        })?;
                    }
                }
            }
        }
        if let Some(Fingerprint::Script { args, .. }) = &artifact.fingerprint {
            for argument in args {
                if let Some(reference) = argument_reference(argument).map_err(|failure| {
                    error(
                        &["fingerprint", "script", "args"],
                        format!("fingerprint.script: {failure}"),
                    )
                })? {
                    let target =
                        reference_target(config, id, reference.name).map_err(|failure| {
                            error(
                                &["fingerprint", "script", "args"],
                                format!("fingerprint.script: {failure}"),
                            )
                        })?;
                    reference_path(config, target, reference.path).map_err(|failure| {
                        error(
                            &["fingerprint", "script", "args"],
                            format!("fingerprint.script: {failure}"),
                        )
                    })?;
                }
            }
        }
        for (path, source) in &artifact.children {
            relations.push(Relation {
                source: source.clone(),
                target: id.clone(),
                kind: RelationKind::Child { path: path.clone() },
            });
        }
        for (alias, source) in &artifact.mounts {
            if !config.artifacts.contains_key(source) {
                return Err(error(
                    &["mounts", alias],
                    format!("Unknown mount target {source} in {id}."),
                ));
            }
            if config.artifacts.contains_key(alias) && alias != source {
                return Err(error(
                    &["mounts", alias],
                    format!("Ambiguous mount alias {alias} in {id}."),
                ));
            }
            if artifact.file_name().is_some() {
                if artifact.file_name() == Some(alias.as_str()) {
                    return Err(error(
                        &["mounts", alias],
                        format!("Mount {id}/{alias} conflicts with the target file."),
                    ));
                }
            } else {
                match fs::symlink_metadata(config.root.join(artifact.folder()).join(alias)) {
                    Ok(_) => {
                        return Err(error(
                            &["mounts", alias],
                            format!("Mount {id}/{alias} conflicts with a physical entry."),
                        ));
                    }
                    Err(failure) if failure.kind() == std::io::ErrorKind::NotFound => {}
                    Err(failure) => return Err(error(&["mounts", alias], failure.to_string())),
                }
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
    for (id, artifact) in &config.artifacts {
        let Some(Fingerprint::Artifactsum { files: inputs, .. }) = &artifact.fingerprint else {
            continue;
        };
        let error = |message: String| {
            ConfigError::declaration(
                config.root.join(artifact.declaration_path()),
                &["fingerprint", "files"],
                format!("fingerprint.files: {message}"),
            )
        };
        let scope = artifact_scope(config, &[id]).map_err(|failure| error(failure.0))?;
        for input in inputs.iter().filter(|input| *input != ".") {
            if input.ends_with(".artf")
                && !fs::symlink_metadata(config.root.join(artifact.folder()).join(input))
                    .is_ok_and(|metadata| metadata.is_dir())
            {
                return Err(error(
                    "Artifact declarations (*.artf) cannot be explicit artifactsum inputs; declarations are excluded from artifactsum."
                        .into(),
                ));
            }
            let location = scope
                .resolve_path(id, input)
                .map_err(|failure| error(failure.0))?;
            if location.artifact_id != *id {
                return Err(error(format!(
                    "{input} belongs to Artifact {}; dependencies come from children, mounts and references.",
                    location.artifact_id
                )));
            }
        }
    }
    let mut resolved = Vec::new();
    for eval in &config.evals {
        let error = |field: &[&str], failure: ScopeError| {
            let mut keys = vec!["evals", &eval.declaration.id];
            keys.extend_from_slice(field);
            ConfigError::declaration(
                config
                    .root
                    .join(config.artifacts[&eval.target].declaration_path()),
                &keys,
                format!("Eval {}: {failure}", eval.id),
            )
        };
        let mut references = BTreeMap::new();
        let mut deps = BTreeSet::new();
        if let Profile::Dependency { depends_on } = &eval.declaration.profile {
            for name in depends_on {
                let source = reference_target(config, &eval.target, name)
                    .map_err(|failure| error(&["profile", "depends_on"], failure))?;
                if source == eval.target {
                    return Err(error(
                        &["profile", "depends_on"],
                        ScopeError("Dependency Evals cannot depend on their own Artifact.".into()),
                    ));
                }
                if !deps.insert(source.to_owned()) {
                    return Err(error(
                        &["profile", "depends_on"],
                        ScopeError(
                            "Dependency profile depends_on must resolve to unique Artifacts."
                                .into(),
                        ),
                    ));
                }
                relations.push(Relation {
                    source: source.to_owned(),
                    target: eval.target.clone(),
                    kind: RelationKind::Dependency {
                        eval_id: eval.id.clone(),
                        name: name.clone(),
                    },
                });
            }
        }
        let instruction = eval
            .declaration
            .payload
            .as_ref()
            .map_or("", |payload| payload.instruction.as_str());
        for reference in instruction::references(instruction) {
            let name = reference.name;
            let source = reference_target(config, &eval.target, name)
                .map_err(|failure| error(&["payload", "instruction"], failure))?;
            if instruction[reference.end..].starts_with('/') {
                reference_path(config, source, "/")
                    .map_err(|failure| error(&["payload", "instruction"], failure))?;
            }
            if references
                .insert(name.to_owned(), source.to_owned())
                .is_some()
            {
                continue;
            }
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
                let Some(reference) = argument_reference(argument)
                    .map_err(|failure| error(&["profile", "args"], failure))?
                else {
                    continue;
                };
                let source = reference_target(config, &eval.target, reference.name)
                    .map_err(|failure| error(&["profile", "args"], failure))?;
                reference_path(config, source, reference.path)
                    .map_err(|failure| error(&["profile", "args"], failure))?;
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
    human::validate_references(config)?;
    crate::graph::Graph::new(config).map_err(|error| ConfigError::new(&config.root, error))?;
    Ok(())
}

#[cfg(test)]
mod tests;
