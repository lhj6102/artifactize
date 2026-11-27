//! Definitions live in `.artifactize/**/*.artf`, read together as one program with the
//! plugins' toolsets. Which file binds an Artifact only decides which meta-Artifacts cover it.

use super::{
    authored_target::AuthoredTarget,
    lang::{self, LangError, LoweredArtifact, LoweredEval, LoweredFingerprint, Piece, SlotRef, SlotTarget},
    *,
};
use std::{io::Read, sync::Arc};

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
    let mut config = read_sources_unresolved(root, sources)?;
    crate::scope::resolve_config(&mut config)?;
    crate::scope::validate_eval_tools(&config)?;
    Ok(config)
}

/// [`read_sources`] with the plugins `catalog` holds, for a migration that has not written its
/// converted plugins yet.
pub(super) fn read_sources_with(
    root: PathBuf,
    sources: &BTreeMap<PathBuf, String>,
    catalog: super::plugins::Catalog,
) -> Result<RepoConfig, ConfigError> {
    let program = program(&root, sources, &catalog)?;
    let mut config = build(root, program, catalog)?;
    crate::scope::resolve_config(&mut config)?;
    crate::scope::validate_eval_tools(&config)?;
    Ok(config)
}

/// Every Artifact and eval of the project, before relations are resolved.
pub(super) fn read_sources_unresolved(
    root: PathBuf,
    sources: &BTreeMap<PathBuf, String>,
) -> Result<RepoConfig, ConfigError> {
    let catalog = super::plugins::catalog(&root)?;
    let program = program(&root, sources, &catalog)?;
    build(root, program, catalog)
}

/// Parse and expand the project's files with the plugins' files.
pub(crate) fn program(
    root: &Path,
    sources: &BTreeMap<PathBuf, String>,
    catalog: &super::plugins::Catalog,
) -> Result<lang::Program, ConfigError> {
    let mut files = Vec::new();
    for (path, text) in sources {
        let relative = path.strip_prefix(root).unwrap_or(path).to_owned();
        if lang::looks_like_toml(text) {
            return Err(ConfigError::new(
                path,
                "line 1, column 1: This definition is in the TOML format of artifactize 0.26 and before; run artifactize config migrate to convert the project.",
            ));
        }
        files.push(lang::SourceFile {
            path: relative,
            text: text.clone(),
            origin: lang::Origin::Project,
        });
    }
    for plugin in catalog.values() {
        files.extend(plugin.source_files());
    }
    lang::read(files).map_err(|(sources, error)| {
        let mut located = sources.error(error);
        located.path = root.join(&located.path);
        located
    })
}

/// [`build`] for a program read elsewhere, such as one file in memory.
pub(super) fn build_program(
    root: PathBuf,
    program: lang::Program,
    catalog: super::plugins::Catalog,
) -> Result<RepoConfig, ConfigError> {
    build(root, program, catalog)
}

fn build(
    root: PathBuf,
    program: lang::Program,
    catalog: super::plugins::Catalog,
) -> Result<RepoConfig, ConfigError> {
    let program = Arc::new(program);
    let profiles = program
        .profiles
        .iter()
        .map(|(name, (profile, _))| {
            (
                name.parse().expect("a profile's name is an identifier"),
                profile.clone(),
            )
        })
        .collect();
    let mut config = RepoConfig {
        root,
        artifacts: BTreeMap::new(),
        evals: Vec::new(),
        relations: Vec::new(),
        profiles,
        program: Some(program.clone()),
        plugins: catalog,
    };
    let mut schemas = schemas::Schemas::default();
    for lowered in &program.artifacts {
        let artifact = artifact(&config, &program, lowered, &mut schemas)?;
        config.artifacts.insert(artifact.name.clone(), artifact);
    }
    published_names(&config, &program)?;
    for lowered in &program.artifacts {
        let name: ArtifactName = lowered.name.parse().expect("a validated Artifact name");
        for eval in &lowered.evals {
            let declaration = declaration(&config, &program, lowered, eval)?;
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
    }
    populate_nested(&mut config);
    Ok(config)
}

/// A provider sees `{fqn with . as _}_{artifact}`: two tools of one audience that meet there,
/// on one Artifact or two, could not both be offered to one review.
fn published_names(config: &RepoConfig, program: &lang::Program) -> Result<(), ConfigError> {
    let lowered: BTreeMap<&str, &LoweredArtifact> = program
        .artifacts
        .iter()
        .map(|artifact| (artifact.name.as_str(), artifact))
        .collect();
    for (audience, keyword) in [("agent", "agent"), ("human", "human")] {
        let mut seen: BTreeMap<super::ToolName, (&ArtifactName, &super::ToolFqn)> = BTreeMap::new();
        for (name, artifact) in &config.artifacts {
            let tools: Vec<&super::ToolFqn> = if audience == "agent" {
                artifact.views.agent_tools.keys().collect()
            } else {
                artifact.views.human_tools.keys().collect()
            };
            for fqn in tools {
                let published = fqn.published(name.as_str());
                if let Some((other, other_fqn)) = seen.insert(published.clone(), (name, fqn)) {
                    let this = lowered[name.as_str()];
                    let span = this
                        .tool_spans
                        .get(&format!("{keyword}.{fqn}"))
                        .copied()
                        .unwrap_or(this.name_span);
                    return Err(located(
                        &config.root,
                        program,
                        this,
                        span,
                        format!(
                            "The {audience} tool {fqn} of {name} and {other_fqn} of {other} are both offered as {published}; rename one of them."
                        ),
                    ));
                }
            }
        }
    }
    Ok(())
}

/// An error at `span` in the program, with the `artf` statement that made it when a macro did.
pub(crate) fn located(
    root: &Path,
    program: &lang::Program,
    artifact: &LoweredArtifact,
    span: lang::Span,
    message: impl Into<String>,
) -> ConfigError {
    let mut error = LangError::new(span, message);
    let binding = artifact.binding;
    if binding.file != span.file || span.start < binding.start || span.end > binding.end {
        error = error.note(binding, format!("in the expansion of `artf {}`", artifact.name));
    }
    let mut located = program.sources.error(error);
    located.path = root.join(&located.path);
    located
}

fn artifact(
    config: &RepoConfig,
    program: &lang::Program,
    lowered: &LoweredArtifact,
    schemas: &mut schemas::Schemas,
) -> Result<Artifact, ConfigError> {
    let fail = |span, message: String| located(&config.root, program, lowered, span, message);
    let name: ArtifactName = lowered
        .name
        .parse()
        .map_err(|error: String| fail(lowered.name_span, error))?;
    artifact_name(&lowered.name, "Artifact name").map_err(|error| fail(lowered.name_span, error))?;
    let (path_text, path_span) = &lowered.path;
    let (authored, target) = path_text
        .parse::<AuthoredTarget>()
        .map_err(|error| fail(*path_span, format!("path: {error}")))?
        .into_parts();
    let select = match &lowered.select {
        None => Vec::new(),
        Some((select, span)) => {
            if select.is_empty()
                || select.len() > MAX_DECLARED_ITEMS
                || select.iter().collect::<BTreeSet<_>>().len() != select.len()
            {
                return Err(fail(*span, "select must contain 1–64 unique paths relative to the folder.".into()));
            }
            select
                .iter()
                .map(|selected| {
                    validation::path(selected).map_err(|message| fail(*span, format!("select: {message}")))?;
                    selected.parse::<LogicalPath>().map_err(|message| fail(*span, format!("select: {message}")))
                })
                .collect::<Result<_, _>>()?
        }
    };
    let kind = match (target.kind(), lowered.select.as_ref()) {
        (ArtifactKind::Folder, Some(_)) => ArtifactKind::Aspect,
        (ArtifactKind::Live, Some((_, span))) => {
            return Err(fail(*span, "select is not supported on a Live target.".into()));
        }
        (ArtifactKind::File, Some((_, span))) => {
            return Err(fail(*span, "select requires a folder path ending in /.".into()));
        }
        (kind, _) => kind,
    };
    let definitions = target
        .local()
        .is_some_and(|local| local.starts_with(DEFINITION_DIR));
    let fingerprint_span = lowered.fingerprint.1.unwrap_or(lowered.binding);
    let fingerprint = match &lowered.fingerprint.0 {
        LoweredFingerprint::Disabled => None,
        LoweredFingerprint::Default => match kind {
            ArtifactKind::Live => None,
            // Definitions are compared by what they declare once expanded, not by their bytes.
            ArtifactKind::Folder | ArtifactKind::File if definitions => Some(Fingerprint::Graph),
            ArtifactKind::Folder | ArtifactKind::Graph => Some(Fingerprint::default()),
            // Without a fingerprint of its own, an aspect hashes what it selects.
            ArtifactKind::Aspect => Some(Fingerprint::Artifactsum {
                files: select.clone(),
                ignore: Default::default(),
            }),
            ArtifactKind::File => Some(Fingerprint::Artifactsum {
                files: vec![file_name(&target).parse().expect("a validated file name")],
                ignore: Default::default(),
            }),
        },
        LoweredFingerprint::Declared(declared, ignore_written) => {
            declared.check().map_err(|message| fail(fingerprint_span, message))?;
            match (kind, declared) {
                (ArtifactKind::Live, Fingerprint::Artifactsum { .. }) => {
                    return Err(fail(fingerprint_span, "A Live Artifact uses a fingerprint script or none, never artifactsum.".into()));
                }
                (ArtifactKind::File, Fingerprint::Artifactsum { files, ignore }) => {
                    if *ignore_written || !ignore.patterns().is_empty() {
                        return Err(fail(fingerprint_span, "fingerprint.ignore is not supported for a file Artifact.".into()));
                    }
                    let own = file_name(&target);
                    if files.is_empty() {
                        return Err(fail(fingerprint_span, format!("fingerprint.files for a file Artifact names its target {own}.")));
                    }
                    if let Some(other) = files.iter().find(|input| input.as_str() != own) {
                        return Err(fail(fingerprint_span, format!("fingerprint.files for a file Artifact may name only its target {own}, not {other}.")));
                    }
                }
                (ArtifactKind::Aspect, Fingerprint::Artifactsum { files, .. }) => {
                    let within = |input: &str| {
                        select.iter().any(|selected| {
                            input
                                .strip_prefix(selected.as_str())
                                .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
                        })
                    };
                    if let Some(outside) = files.iter().find(|input| !within(input)) {
                        return Err(fail(fingerprint_span, format!("fingerprint.files for an aspect must lie in its selection: {outside}.")));
                    }
                }
                _ => {}
            }
            Some(declared.clone())
        }
    };
    if lowered.basis == Some(true) && !lowered.evals.is_empty() {
        return Err(fail(lowered.name_span, format!("Basis Artifact {name} cannot own evals.")));
    }
    validation::tags(&lowered.tags).map_err(|message| fail(lowered.binding, message))?;
    let mut parts = BTreeSet::new();
    if lowered.parts.len() > MAX_DECLARED_ITEMS {
        return Err(fail(lowered.parts[0].1, "parts must name at most 64 Artifacts.".into()));
    }
    for (part, span) in &lowered.parts {
        if *part == lowered.name {
            return Err(fail(*span, format!("{name} cannot be a part of itself.")));
        }
        if !parts.insert(part.parse::<ArtifactName>().expect("a declared Artifact name")) {
            return Err(fail(*span, format!("parts names {part} twice.")));
        }
    }
    let mut views = lowered.views.clone();
    views.share(schemas);
    views.check(schemas).map_err(|(table, tool, message)| {
        fail(lang::span_of(lowered, &["views", table, tool.as_str()]), message)
    })?;
    composed_targets(&views, &lowered.toolsets, kind, &target)
        .map_err(|message| fail(lowered.tools.as_ref().map_or(lowered.binding, |tools| tools.1), message))?;
    let definition = program.sources.file(lowered.binding.file).path.clone();
    Ok(Artifact {
        definition,
        authored_path: Some(authored),
        target_error: None,
        path: target
            .with_kind(kind, select)
            .map_err(|error| fail(*path_span, error.to_string()))?,
        covering_part: None,
        nested: BTreeMap::new(),
        parts,
        name,
        tags: lowered.tags.clone(),
        views,
        tools: lowered.tools.as_ref().map(|(text, _)| text.clone()),
        toolsets: lowered.toolsets.clone(),
        binding: Some(Binding {
            line: program.sources.line_column(lowered.binding).0,
            macro_name: lowered.macro_name.as_ref().map(|name| format!("{name}!")),
        }),
        basis: lowered.basis,
        fingerprint,
    })
}

fn file_name(target: &Target) -> &str {
    target
        .local()
        .and_then(Path::file_name)
        .and_then(|name| name.to_str())
        .expect("a file target names a UTF-8 file")
}

/// What composed tools need of their target, checked once every toolset is in: a Live
/// Artifact is observed by commands, never read as files or opened in a viewer, and the
/// Artifactize Viewer reads only definitions.
fn composed_targets(
    views: &Views,
    toolsets: &[String],
    kind: ArtifactKind,
    target: &Target,
) -> Result<(), String> {
    let file_builtin = |builtin: Builtin| !matches!(builtin, Builtin::Graph | Builtin::Help | Builtin::Open);
    if kind == ArtifactKind::Live
        && (views
            .agent_tools
            .values()
            .any(|tool| matches!(tool, AgentTool::Builtin(tool) if file_builtin(tool.builtin)))
            || views.human_tools.values().any(|tool| match tool {
                HumanTool::Builtin(tool) => file_builtin(tool.builtin),
                HumanTool::Viewer(_) => true,
                HumanTool::Command(_) => false,
            }))
    {
        return Err("File builtins and viewers cannot read a Live Artifact; compose read-only observation commands.".into());
    }
    let definitions = target
        .local()
        .is_some_and(|local| local.starts_with(DEFINITION_DIR));
    if toolsets.iter().any(|toolset| toolset == "artifactize") && !definitions {
        return Err("The artifactize toolset reads definitions under .artifactize/ only.".into());
    }
    Ok(())
}

/// An eval with its instruction rendered: each slot's text, and what it names.
fn declaration(
    config: &RepoConfig,
    program: &lang::Program,
    lowered: &LoweredArtifact,
    eval: &LoweredEval,
) -> Result<EvalDeclaration, ConfigError> {
    let fail = |span, message: String| located(&config.root, program, lowered, span, message);
    let instruction = eval
        .instruction
        .as_ref()
        .map(|(pieces, _)| {
            let mut text = String::new();
            let mut slots = Vec::new();
            for piece in pieces {
                let slot = match piece {
                    Piece::Text(part) => {
                        text.push_str(part);
                        continue;
                    }
                    Piece::Slot(slot) => slot,
                };
                let name = match slot.target() {
                    SlotTarget::SelfRef => lowered.name.as_str(),
                    SlotTarget::Named(name, _) => name.as_str(),
                };
                let artifact = config
                    .artifacts
                    .get(name)
                    .ok_or_else(|| fail(slot.span(), format!("Unknown Artifact `{name}`.")))?;
                let start = text.len();
                let filled = match slot {
                    SlotRef::Artf { .. } => {
                        text.push_str(name);
                        eval::Slot::Artf {
                            artifact: artifact.name.clone(),
                        }
                    }
                    SlotRef::Path { path, span, .. } => {
                        let suffix: crate::scope::LogicalTarget = path
                            .parse()
                            .map_err(|error: crate::scope::ScopeError| fail(*span, error.to_string()))?;
                        if artifact.file_name().is_some() && !path.is_empty() {
                            return Err(fail(*span, format!("File Artifact {name} cannot have a /path suffix.")));
                        }
                        text.push_str(&shown_path(artifact, path));
                        eval::Slot::Path {
                            artifact: artifact.name.clone(),
                            path: suffix,
                        }
                    }
                    SlotRef::Tool {
                        audience, tool, span, ..
                    } => {
                        let fqn: ToolFqn = tool
                            .parse()
                            .map_err(|message: String| fail(*span, message))?;
                        let (audience, known): (eval::Audience, Vec<&ToolFqn>) = if audience == "agent" {
                            (eval::Audience::Agent, artifact.views.agent_tools.keys().collect())
                        } else {
                            (eval::Audience::Human, artifact.views.human_tools.keys().collect())
                        };
                        if !known.contains(&&fqn) {
                            return Err(fail(
                                *span,
                                format!(
                                    "Artifact {name} has no {} tool `{fqn}`; it has {}.",
                                    if audience == eval::Audience::Agent { "agent" } else { "human" },
                                    lang_names(&known)
                                ),
                            ));
                        }
                        match audience {
                            eval::Audience::Agent => text.push_str(fqn.published(name).as_str()),
                            eval::Audience::Human => text.push_str(fqn.as_str()),
                        }
                        eval::Slot::Tool {
                            artifact: artifact.name.clone(),
                            audience,
                            tool: fqn,
                        }
                    }
                };
                slots.push(eval::InstructionSlot {
                    range: start..text.len(),
                    slot: filled,
                });
            }
            Ok::<_, ConfigError>((text, slots))
        })
        .transpose()?;
    EvalDeclaration::new(eval::Declared {
        id: eval
            .id
            .parse()
            .map_err(|message: String| fail(eval.id_span, message))?,
        title: eval.title.clone(),
        mutex: eval.mutex.clone(),
        profile: eval.profile.clone(),
        profile_name: eval.profile_name.clone(),
        instruction,
        inputs: eval
            .inputs
            .iter()
            .map(|(input, _)| input.parse().expect("a declared Artifact name"))
            .collect(),
        red_at: eval.red_at,
    })
    .map_err(|message| fail(eval.id_span, format!("Eval {}: {message}", eval.id)))
}

fn lang_names(names: &[&ToolFqn]) -> String {
    if names.is_empty() {
        "none".into()
    } else {
        names
            .iter()
            .map(|name| format!("`{name}`"))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

/// How an instruction shows `@{path name/sub}`: the target as declared, then the sub path.
fn shown_path(artifact: &Artifact, sub: &str) -> String {
    let base = artifact
        .authored_path
        .clone()
        .unwrap_or_else(|| artifact.path.to_string());
    if sub.is_empty() {
        base
    } else if base.ends_with('/') {
        format!("{base}{sub}")
    } else {
        format!("{base}/{sub}")
    }
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

/// Every `.artf` file under `.artifactize/`, except installed plugins.
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
