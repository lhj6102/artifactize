//! Artifact references and canonical scoped paths.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::config::{Artifact, ArtifactName, ConfigError, Eval, Fingerprint, Profile, RepoConfig};

mod argument;
pub(crate) use argument::PATH_SLOT;
pub use argument::{Argument, LogicalSuffix, LogicalTarget};

mod human;
pub(crate) use human::{builtin_args, resolve_human_argv};

pub use artifactize_tools::scope::{ArtifactId, ScopeError, ScopedPath, scoped_path};
pub(crate) use artifactize_tools::scope::{logical_path, open_child, open_scoped};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Relation {
    /// Input Artifact.
    pub source: ArtifactName,
    /// Consumer Artifact.
    pub target: ArtifactName,
    #[serde(flatten)]
    pub kind: RelationKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum RelationKind {
    /// The source is a part of the target: its folder holds it, or the target's `parts`
    /// names it.
    Part,
    Instruction {
        #[serde(rename = "evalId")]
        eval_id: crate::config::EvalId,
        name: crate::config::ReferenceName,
    },
    #[serde(rename = "argv")]
    Argument {
        #[serde(rename = "evalId")]
        eval_id: crate::config::EvalId,
        index: usize,
        name: crate::config::ReferenceName,
        path: LogicalTarget,
    },
    /// A runtime eval's declared input.
    Input {
        #[serde(rename = "evalId")]
        eval_id: crate::config::EvalId,
        name: crate::config::ReferenceName,
    },
}

#[derive(Debug, Clone)]
pub struct Scope<'a> {
    pub artifacts: BTreeMap<ArtifactName, &'a Artifact>,
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

    pub fn resolve_path(
        &self,
        artifact_id: &ArtifactName,
        path: &str,
    ) -> Result<ScopedPath, ScopeError> {
        self.tool_scope().resolve_path(&tool_id(artifact_id), path)
    }

    pub fn resolve_input(
        &self,
        root: &Path,
        artifact_id: &ArtifactName,
        path: &str,
    ) -> Result<PathBuf, ScopeError> {
        self.tool_scope()
            .resolve_input(root, &tool_id(artifact_id), path)
    }
}

/// The tool-side ID of a configured Artifact. Configuration validates every Artifact name
/// and child, so a name that fails here is a bug in that validation.
pub(crate) fn tool_id(name: &ArtifactName) -> ArtifactId {
    ArtifactId::new(name.as_str()).expect("configuration validates Artifact names")
}

/// Revalidate file targets at every preparation/read boundary, without following links.
pub(crate) fn validate_file_target(root: &Path, artifact: &Artifact) -> Result<(), ScopeError> {
    if matches!(
        artifact.kind(),
        crate::config::ArtifactKind::Folder | crate::config::ArtifactKind::Aspect
    ) {
        let directory = scoped_path(root, artifact.folder())?;
        let opened = crate::platform::open_directory(&directory).map_err(|error| {
            ScopeError(format!(
                "Artifact {} target {} must be a folder: {error}",
                artifact.name, artifact.path
            ))
        })?;
        if !opened
            .metadata()
            .map_err(|error| ScopeError(error.to_string()))?
            .is_dir()
        {
            return Err(ScopeError(format!(
                "Artifact {} target {} must be a folder.",
                artifact.name, artifact.path
            )));
        }
        for selected in artifact.select() {
            let file = open_scoped(&directory, selected).map_err(|error| {
                ScopeError(format!(
                    "Artifact {} selection {selected} is unavailable: {error}",
                    artifact.name
                ))
            })?;
            let metadata = file
                .metadata()
                .map_err(|error| ScopeError(error.to_string()))?;
            if !metadata.is_file() && !metadata.is_dir() {
                return Err(ScopeError(format!(
                    "Artifact {} selection {selected} must be a file or folder.",
                    artifact.name
                )));
            }
        }
    }
    if artifact.file_name().is_some() {
        artifact
            .path
            .local()
            .expect("file target")
            .to_str()
            .ok_or_else(|| ScopeError("Artifact paths must be UTF-8.".into()))?;
        let target = artifact.path.to_string();
        let file = open_scoped(root, &target).map_err(|error| {
            ScopeError(format!(
                "File Artifact {} target {} is unavailable: {error}",
                artifact.name, artifact.path
            ))
        })?;
        if !file
            .metadata()
            .map_err(|error| ScopeError(error.to_string()))?
            .is_file()
        {
            return Err(ScopeError(format!(
                "File Artifact {} target {} must remain a regular file.",
                artifact.name, artifact.path
            )));
        }
    }
    Ok(())
}

fn reference_path(config: &RepoConfig, id: &ArtifactName, path: &str) -> Result<(), ScopeError> {
    if config.artifacts[id].file_name().is_some() && !path.is_empty() {
        return Err(ScopeError(format!(
            "File Artifact {{{id}}} cannot have a /path suffix."
        )));
    }
    Ok(())
}

/// A path under `{source}` that lies in a nested folder Artifact is that Artifact's input,
/// which only a reference of its own brings into a review.
fn nested_path(
    everything: &artifactize_tools::scope::Scope,
    source: &ArtifactName,
    path: &str,
) -> Result<(), ScopeError> {
    if everything.artifacts[&tool_id(source)].kind() == crate::config::ArtifactKind::Live {
        return Ok(());
    }
    let resolved = everything.resolve_path(&tool_id(source), path)?;
    if resolved.artifact_id.as_str() != source.as_str() {
        return Err(ScopeError(format!(
            "{{{source}}}/{path} is in Artifact {owner}; reference {{{owner}}}/{rest} instead.",
            owner = resolved.artifact_id,
            rest = resolved.path,
        )));
    }
    Ok(())
}

/// Exactly the given Artifacts. Neither composition nor a referenced Artifact's Eval
/// instructions grant access: a path into a part or a nested folder Artifact needs a
/// reference of its own, so that what a review reads is what its reuse key covers.
pub fn artifact_scope<'a>(
    config: &'a RepoConfig,
    roots: &[&ArtifactName],
) -> Result<Scope<'a>, ScopeError> {
    let artifacts = roots
        .iter()
        .map(|&id| {
            config
                .artifacts
                .get_key_value(id)
                .map(|(id, artifact)| (id.clone(), artifact))
                .ok_or_else(|| ScopeError(format!("Unknown Artifact: {id}")))
        })
        .collect::<Result<_, _>>()?;
    Ok(Scope { artifacts })
}

/// Validate scope-relative command paths before process lookup. Bare names and absolute
/// paths stay literal here; preflight and both launchers use the shared PATH/PATHEXT rule.
pub(crate) fn executable(
    root: &Path,
    scope: &Scope<'_>,
    owner: &ArtifactName,
    command: &str,
) -> Result<std::ffi::OsString, String> {
    let artifact = scope.artifacts[owner];
    let cwd = scoped_path(root, artifact.folder()).map_err(|error| error.to_string())?;
    let command = crate::platform::program_path(command).map_err(|error| error.to_string())?;
    let relative = crate::platform::program::folder_relative(&command)?;
    let program = if let Some(relative) = relative.as_deref() {
        let physical = if artifact.file_name().is_some()
            || artifact.kind() == crate::config::ArtifactKind::Live
        {
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
        crate::platform::program::reject_link_ancestors(&physical)?;
        let resolve = |candidate: &Path| {
            if artifact.file_name().is_some()
                || artifact.kind() == crate::config::ArtifactKind::Live
            {
                scoped_path(&cwd, candidate).map_err(|error| error.to_string())
            } else {
                let logical = artifactize_tools::scope::logical_from_native(candidate)
                    .ok_or_else(|| "Executable path is not portable text.".to_owned())?;
                scope
                    .resolve_input(root, owner, &logical)
                    .map_err(|error| error.to_string())
            }
        };
        crate::platform::program::resolve_scoped(Path::new(relative), resolve)?
    } else {
        command
    };
    Ok(program.into_os_string())
}

pub fn eval_scope<'a>(config: &'a RepoConfig, eval: &Eval) -> Result<Scope<'a>, ScopeError> {
    let roots: Vec<_> = std::iter::once(&eval.target)
        .chain(eval.deps.iter())
        .collect();
    artifact_scope(config, &roots)
}

/// Admit an owner plus the explicit references in its argv, as an Eval's scope admits its deps.
pub fn argv_scope<'a>(
    config: &'a RepoConfig,
    owner: &ArtifactName,
    args: &[Argument],
) -> Result<Scope<'a>, ScopeError> {
    let mut roots = vec![owner];
    for argument in args {
        if let Argument::Reference { name, .. } = argument {
            roots.push(reference_target(config, owner, name)?);
        }
    }
    artifact_scope(config, &roots)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstructionPart {
    Text(String),
    Artifact(ArtifactName),
}

/// The instruction with its `@{artf …}` slots as the Artifacts they name; paths and tools
/// read as their rendered text.
pub(crate) fn artifact_instruction(eval: &Eval) -> Vec<InstructionPart> {
    let Some(source) = eval.declaration.instruction() else {
        return Vec::new();
    };
    let mut parts = Vec::new();
    let mut start = 0;
    for slot in eval.declaration.slots() {
        let crate::config::Slot::Artf { artifact } = &slot.slot else {
            continue;
        };
        if start < slot.range.start {
            parts.push(InstructionPart::Text(
                source[start..slot.range.start].into(),
            ));
        }
        parts.push(InstructionPart::Artifact(artifact.clone()));
        start = slot.range.end;
    }
    if start < source.len() {
        parts.push(InstructionPart::Text(source[start..].into()));
    }
    parts
}

fn reference_target<'a>(
    config: &'a RepoConfig,
    owner: &ArtifactName,
    name: &crate::config::ReferenceName,
) -> Result<&'a ArtifactName, ScopeError> {
    if !config.artifacts.contains_key(owner) {
        return Err(ScopeError(format!("Unknown Artifact: {owner}")));
    }
    let target = config.artifacts.get_key_value(name.as_str());
    target
        .map(|(id, _)| id)
        .ok_or_else(|| ScopeError(format!("Unknown Artifact in `@{{path {name}}}`.")))
}

/// One native argument and its diagnostic spelling. Only resolved path portions are
/// normalized for display; flags, opaque Live addresses and literal arguments are unchanged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ResolvedArgument {
    Literal(String),
    Path { prefix: String, path: PathBuf },
}

impl ResolvedArgument {
    pub(crate) fn native(&self) -> String {
        match self {
            Self::Literal(text) => text.clone(),
            Self::Path { prefix, path } => {
                format!(
                    "{prefix}{}",
                    crate::platform::os_text(path.as_os_str()).expect("validated UTF-8 path")
                )
            }
        }
    }

    pub(crate) fn shown(&self) -> String {
        match self {
            Self::Literal(text) => text.clone(),
            Self::Path { prefix, path } => format!("{prefix}{}", crate::platform::path_text(path)),
        }
    }
}

/// Resolve `{name}[/path]` and `--flag={name}[/path]` operands to existing scoped inputs.
/// Other arguments, including escaped braces, stay literal. Never interpolate the command.
pub fn resolve_argv(
    config: &RepoConfig,
    scope: &Scope<'_>,
    owner: &ArtifactName,
    args: &[Argument],
) -> Result<Vec<String>, ScopeError> {
    resolved_argv(config, scope, owner, args)
        .map(|args| args.iter().map(ResolvedArgument::native).collect())
}

pub(crate) fn resolved_argv(
    config: &RepoConfig,
    scope: &Scope<'_>,
    owner: &ArtifactName,
    args: &[Argument],
) -> Result<Vec<ResolvedArgument>, ScopeError> {
    args.iter()
        .map(|argument| {
            let Argument::Reference { prefix, name, path } = argument else {
                let literal = argument.literal().expect("a literal argument");
                return Ok(ResolvedArgument::Literal(literal.to_owned()));
            };
            let id = reference_target(config, owner, name)?;
            reference_path(config, id, path)?;
            argument_value(config, scope, id, path, prefix)
        })
        .collect()
}

/// Resolve a scoped argument once. Live addresses are opaque and never normalized as paths.
pub(crate) fn argument_value(
    config: &RepoConfig,
    scope: &Scope<'_>,
    id: &ArtifactName,
    path: &str,
    prefix: &str,
) -> Result<ResolvedArgument, ScopeError> {
    let artifact = scope
        .artifacts
        .get(id)
        .ok_or_else(|| ScopeError(format!("Artifact is outside this review: {id}")))?;
    if let Some(uri) = artifact.path.uri() {
        return Ok(ResolvedArgument::Literal(if path.is_empty() {
            format!("{prefix}{uri}")
        } else {
            format!("{prefix}{uri}/{path}")
        }));
    }
    let resolved = scope.resolve_input(&config.root, id, path)?;
    if crate::platform::os_text(resolved.as_os_str()).is_none() {
        return Err(ScopeError("Artifact paths must be UTF-8.".into()));
    }
    Ok(ResolvedArgument::Path {
        prefix: prefix.into(),
        path: resolved,
    })
}

/// Static relationships only; scheduling and dependency closure belong to graph.
pub(crate) fn resolve_config(config: &mut RepoConfig) -> Result<(), ConfigError> {
    let mut relations = Vec::new();
    for (id, artifact) in &config.artifacts {
        let error = |keys: &[&str], message: String| config.error_at(id, keys, message);
        for (name, tool) in &artifact.views.agent_tools {
            if let crate::config::AgentTool::Builtin(tool) = tool
                && let Some(args) = &tool.args
            {
                let scope = artifact_scope(config, &[id]).map_err(|failure| {
                    error(&["views", "agent_tools", name, "args"], failure.to_string())
                })?;
                let owner = ArtifactId::new(id.as_str()).map_err(|failure| {
                    error(&["views", "agent_tools", name, "args"], failure.to_string())
                })?;
                artifactize_tools::builtin::validate_target(
                    tool.builtin,
                    args,
                    &config.root,
                    &scope.tool_scope(),
                    &owner,
                )
                .map_err(|failure| error(&["views", "agent_tools", name, "args"], failure))?;
            }
            if let crate::config::AgentTool::Command(tool) = tool
                && tool.protocol == crate::config::ToolProtocol::Json
            {
                for argument in &tool.args {
                    if let Argument::Reference {
                        name: ref_name,
                        path,
                        ..
                    } = argument
                    {
                        let target = reference_target(config, id, ref_name).map_err(|failure| {
                            error(
                                &["views", "agent_tools", name, "args"],
                                format!("Agent tool {name}: {failure}"),
                            )
                        })?;
                        reference_path(config, target, path).map_err(|failure| {
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
                if let Argument::Reference {
                    name: ref_name,
                    path,
                    ..
                } = argument
                {
                    let target = reference_target(config, id, ref_name).map_err(|failure| {
                        error(
                            &["fingerprint", "script", "args"],
                            format!("fingerprint.script: {failure}"),
                        )
                    })?;
                    reference_path(config, target, path).map_err(|failure| {
                        error(
                            &["fingerprint", "script", "args"],
                            format!("fingerprint.script: {failure}"),
                        )
                    })?;
                }
            }
        }
    }
    for (id, artifact) in &config.artifacts {
        let Some(Fingerprint::Artifactsum { files: inputs, .. }) = &artifact.fingerprint else {
            continue;
        };
        let error = |message: String| {
            config.error_at(
                id,
                &["fingerprint", "files"],
                format!("fingerprint.files: {message}"),
            )
        };
        // A covering folder part subtracts every input, including a file/aspect's defaults.
        if artifact.covering_part.is_some() {
            continue;
        }
        let scope = artifact_scope(config, &[id]).map_err(|failure| error(failure.0))?;
        for input in inputs.iter().filter(|input| *input != ".") {
            let location = scope
                .resolve_path(id, input)
                .map_err(|failure| error(failure.0))?;
            if location.artifact_id.as_str() != id.as_str() {
                return Err(error(format!(
                    "{input} belongs to Artifact {}; a fingerprint covers only its own Artifact's files.",
                    location.artifact_id
                )));
            }
        }
    }
    let everything = everything(config);
    let mut resolved = Vec::new();
    for eval in &config.evals {
        let mut found = eval_references(config, &everything, eval)?;
        relations.append(&mut found.relations);
        resolved.push(found);
    }
    for (eval, found) in config.evals.iter_mut().zip(resolved) {
        eval.references = found.references;
        eval.deps = found.deps;
    }
    for artifact in config.artifacts.values() {
        for part in &artifact.parts {
            let error = |message: String| config.error_at(&artifact.name, &["parts"], message);
            match config.artifacts.get(part) {
                None => return Err(error(format!("Unknown Artifact {part}."))),
                Some(part) if part.kind() == crate::config::ArtifactKind::Graph => {
                    return Err(error(format!(
                        "The graph Artifact {} is a part of nothing.",
                        part.name
                    )));
                }
                Some(_) => relations.push(Relation {
                    source: part.clone(),
                    target: artifact.name.clone(),
                    kind: RelationKind::Part,
                }),
            }
        }
    }
    parts_acyclic(config)?;
    config.relations = relations;
    human::validate_references(config)?;
    crate::graph::Graph::new(config).map_err(|error| ConfigError::new(&config.root, error))?;
    Ok(())
}

/// Tools are required only for what the instruction can inspect, not for implicit parts
/// whose results affect reuse but are not present in the review's tool scope.
pub(crate) fn missing_eval_tools(
    config: &RepoConfig,
) -> impl Iterator<Item = (&Eval, &Artifact, &'static str)> {
    config.evals.iter().flat_map(move |eval| {
        std::iter::once(&eval.target)
            .chain(&eval.deps)
            .filter_map(move |id| {
                let artifact = &config.artifacts[id];
                let kind = match eval.declaration.profile() {
                    Profile::Agent { .. } if artifact.views.agent_tools.is_empty() => "Agent",
                    Profile::Human {} if artifact.views.human_tools.is_empty() => "Human",
                    _ => return None,
                };
                Some((eval, artifact, kind))
            })
    })
}

pub(crate) fn validate_eval_tools(config: &RepoConfig) -> Result<(), ConfigError> {
    let Some((eval, artifact, kind)) = missing_eval_tools(config).next() else {
        return Ok(());
    };
    let (article, audience) = if kind == "Agent" {
        ("an", "agent")
    } else {
        ("a", "human")
    };
    let toolset_fix = if artifact.kind() == crate::config::ArtifactKind::Live {
        String::new()
    } else {
        "; for a local file or folder, `: files` gives both".to_owned()
    };
    Err(config.error_at(
        &eval.target,
        &["evals", eval.declaration.id.as_ref()],
        format!(
            "Eval {} includes Artifact {} without {article} {kind} tool. Compose a toolset with `{audience}` tools on {}{toolset_fix}.",
            eval.id, artifact.name, artifact.name
        ),
    ))
}

/// Every Artifact of `config` as one tool scope, which nested paths are checked against.
fn everything(config: &RepoConfig) -> artifactize_tools::scope::Scope {
    Scope {
        artifacts: config
            .artifacts
            .iter()
            .map(|(id, artifact)| (id.clone(), artifact))
            .collect(),
    }
    .tool_scope()
}

/// What an eval's instruction, runtime arguments and inputs reference.
struct EvalReferences {
    references: BTreeMap<crate::config::ReferenceName, ArtifactName>,
    /// The referenced Artifacts other than the eval's target.
    deps: Vec<ArtifactName>,
    /// The relations that record the deps.
    relations: Vec<Relation>,
}

fn eval_references(
    config: &RepoConfig,
    everything: &artifactize_tools::scope::Scope,
    eval: &Eval,
) -> Result<EvalReferences, ConfigError> {
    let mut relations = Vec::new();
    let error = |field: &[&str], failure: ScopeError| {
        let mut keys = vec!["evals", eval.declaration.id.as_ref()];
        keys.extend_from_slice(field);
        config.error_at(&eval.target, &keys, format!("Eval {}: {failure}", eval.id))
    };
    let mut references = BTreeMap::new();
    let mut deps = BTreeSet::new();
    // Every slot that names another Artifact admits it: its tools join the review and its
    // fingerprint the reuse key.
    for slot in eval.declaration.slots() {
        let source = slot.slot.artifact();
        if let crate::config::Slot::Path { path, .. } = &slot.slot {
            nested_path(everything, source, path)
                .map_err(|failure| error(&["instruction"], failure))?;
        }
        let name = crate::config::ReferenceName::from(source);
        if references.insert(name.clone(), source.clone()).is_some() || source == &eval.target {
            continue;
        }
        deps.insert(source.clone());
        relations.push(Relation {
            source: source.clone(),
            target: eval.target.clone(),
            kind: RelationKind::Instruction {
                eval_id: eval.id.clone(),
                name,
            },
        });
    }
    for input in eval.declaration.inputs() {
        if input == &eval.target {
            return Err(error(
                &["inputs"],
                ScopeError(format!(
                    "{input} is the eval's own Artifact; list other Artifacts in inputs."
                )),
            ));
        }
        let name = crate::config::ReferenceName::from(input);
        references.insert(name.clone(), input.clone());
        if deps.insert(input.clone()) {
            relations.push(Relation {
                source: input.clone(),
                target: eval.target.clone(),
                kind: RelationKind::Input {
                    eval_id: eval.id.clone(),
                    name,
                },
            });
        }
    }
    if let Profile::Runtime { args, .. } = eval.declaration.profile() {
        for (index, argument) in args.iter().enumerate() {
            let Argument::Reference {
                name: ref_name,
                path,
                ..
            } = argument
            else {
                continue;
            };
            let source = reference_target(config, &eval.target, ref_name)
                .map_err(|failure| error(&["profile", "args"], failure))?;
            reference_path(config, source, path)
                .map_err(|failure| error(&["profile", "args"], failure))?;
            nested_path(everything, source, path)
                .map_err(|failure| error(&["profile", "args"], failure))?;
            if source != &eval.target {
                deps.insert(source.clone());
                relations.push(Relation {
                    source: source.clone(),
                    target: eval.target.clone(),
                    kind: RelationKind::Argument {
                        eval_id: eval.id.clone(),
                        index,
                        name: ref_name.clone(),
                        path: path.clone(),
                    },
                });
            }
        }
    }
    Ok(EvalReferences {
        references,
        deps: deps.into_iter().collect(),
        relations,
    })
}

/// Nothing is a part of itself, however many parts lie between.
fn parts_acyclic(config: &RepoConfig) -> Result<(), ConfigError> {
    for artifact in config.artifacts.values() {
        if config.descendants(&artifact.name).contains(&&artifact.name) {
            return Err(config.error_at(
                &artifact.name,
                &["parts"],
                format!("{} is a part of itself through its parts.", artifact.name),
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
