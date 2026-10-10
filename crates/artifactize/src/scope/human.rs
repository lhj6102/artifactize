use super::{Argument, RepoConfig, Scope, ScopeError, reference_target};

/// Parse `argument` the permissive (Agent-tool) way, then additionally reject any unescaped
/// `{`/`}` that didn't form the single recognized reference: Human arguments, unlike Agent and
/// Runtime ones, never pass stray brace characters through as literal text.
fn reference(argument: &str) -> Result<Option<Argument>, ScopeError> {
    let parsed: Argument = argument.parse()?;
    if argument.contains(['{', '}'])
        && (matches!(parsed, Argument::Literal(_))
            || argument.bytes().filter(|byte| *byte == b'{').count() != 1
            || argument.bytes().filter(|byte| *byte == b'}').count() != 1)
    {
        return Err(ScopeError(
            "Human arguments support only {artifactPath} or {name}[/path] scope placeholders (optionally --flag=)."
                .into(),
        ));
    }
    Ok(match parsed {
        Argument::Literal(_) => None,
        reference @ Argument::Reference { .. } => Some(reference),
    })
}

/// The same stray-brace rejection as [`reference`], for an argument already parsed into its
/// typed form (a `HumanCommandTool`'s declared [`Argument`]s).
pub(crate) fn validate_human_argument(argument: &Argument) -> Result<(), ScopeError> {
    if let Argument::Literal(text) = argument
        && text.contains(['{', '}'])
    {
        return Err(ScopeError(
            "Human arguments support only {artifactPath} or {name}[/path] scope placeholders (optionally --flag=)."
                .into(),
        ));
    }
    Ok(())
}

/// Validate a `HumanBuiltinTool`'s still string-typed args (its placeholders, including the
/// reserved `{artifactPath}`, are resolved by [`builtin_args`], not by [`super::resolve_argv`]).
pub(crate) fn validate_human_args(args: &[String]) -> Result<(), ScopeError> {
    for argument in args {
        reference(argument)?;
    }
    Ok(())
}

fn reference_target_or_owner<'a>(
    config: &'a RepoConfig,
    owner: &str,
    name: &str,
) -> Result<&'a crate::config::ArtifactName, ScopeError> {
    if name == "artifactPath" {
        Ok(&config.artifacts[owner].name)
    } else {
        reference_target(config, owner, name)
    }
}

pub(super) fn validate_references(config: &RepoConfig) -> Result<(), super::ConfigError> {
    for (owner, artifact) in &config.artifacts {
        for (name, tool) in &artifact.views.human_tools {
            let validate = || {
                match tool {
                    crate::config::HumanTool::Command(command) => {
                        for argument in &command.args {
                            validate_human_argument(argument)?;
                            if let Argument::Reference {
                                name: ref_name,
                                path,
                                ..
                            } = argument
                            {
                                let target = reference_target_or_owner(config, owner, ref_name)?;
                                super::reference_path(config, target, path)?;
                            }
                        }
                    }
                    crate::config::HumanTool::Builtin(tool) => {
                        for argument in &tool.args {
                            if let Some(Argument::Reference {
                                name: ref_name,
                                path,
                                ..
                            }) = reference(argument)?
                            {
                                let target = reference_target_or_owner(config, owner, &ref_name)?;
                                super::reference_path(config, target, &path)?;
                            }
                        }
                        let scope = Scope {
                            artifacts: config
                                .artifacts
                                .iter()
                                .map(|(id, artifact)| (id.clone(), artifact))
                                .collect(),
                        };
                        builtin_args(config, &scope, owner, tool)?;
                    }
                }
                Ok::<_, ScopeError>(())
            };
            validate().map_err(|error| {
                super::ConfigError::declaration(
                    config.root.join(artifact.declaration_path()),
                    &["views", "human_tools", name, "args"],
                    format!("Human tool {name}: {error}"),
                )
            })?;
        }
    }
    Ok(())
}

pub(crate) fn resolve_human_argv(
    config: &RepoConfig,
    scope: &Scope<'_>,
    owner: &str,
    args: &[Argument],
) -> Result<Vec<String>, ScopeError> {
    args.iter()
        .map(|argument| {
            let Argument::Reference { prefix, name, path } = argument else {
                return Ok(argument.to_string());
            };
            let id = reference_target_or_owner(config, owner, name)?;
            super::reference_path(config, id, path)?;
            let resolved = scope.resolve_input(&config.root, id, path)?;
            resolved
                .to_str()
                .ok_or_else(|| ScopeError("Artifact paths must be UTF-8.".into()))?;
            let resolved = crate::platform::path_text(&resolved);
            Ok(format!("{prefix}{resolved}"))
        })
        .collect()
}

/// Resolve builtin paths back to logical scope paths so reads keep no-follow access.
pub(crate) fn builtin_args(
    config: &RepoConfig,
    scope: &Scope<'_>,
    owner: &str,
    tool: &crate::config::HumanBuiltinTool,
) -> Result<(String, Vec<String>), ScopeError> {
    if tool.builtin == crate::config::Builtin::Help
        || tool.builtin == crate::config::Builtin::Open
            && artifactize_tools::builtin::is_url(&tool.args[0])
    {
        return Ok((owner.into(), tool.args.clone()));
    }
    let mut args = tool.args.clone();
    let id = if let Some(Argument::Reference { prefix, name, path }) = reference(&args[0])? {
        if !prefix.is_empty() {
            return Err(ScopeError(
                "Builtin targets cannot have a flag prefix.".into(),
            ));
        }
        let target = reference_target_or_owner(config, owner, &name)?;
        super::reference_path(config, target, &path)?;
        args[0] = path;
        target
    } else {
        owner
    };
    artifactize_tools::builtin::validate_target(
        tool.builtin,
        &args,
        &config.root,
        &scope.tool_scope(),
        &super::ArtifactId::new(id)?,
    )
    .map_err(ScopeError)?;
    Ok((id.into(), args))
}
