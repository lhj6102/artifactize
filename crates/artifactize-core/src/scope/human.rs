use super::{Argument, RepoConfig, Scope, ScopeError, reference_target};

/// A literal's own text, or an Artifact path's declared spelling until it resolves.
fn spelled(argument: &Argument) -> String {
    argument
        .literal()
        .map_or_else(|| argument.to_string(), str::to_owned)
}

pub(super) fn validate_references(config: &RepoConfig) -> Result<(), super::ConfigError> {
    for (owner, artifact) in &config.artifacts {
        for (name, tool) in &artifact.views.human_tools {
            let validate = || {
                match tool {
                    crate::config::HumanTool::Viewer(_) => {}
                    crate::config::HumanTool::Command(command) => {
                        for argument in &command.args {
                            if let Argument::Reference {
                                name: ref_name,
                                path,
                                ..
                            } = argument
                            {
                                let target = reference_target(config, owner, ref_name)?;
                                super::reference_path(config, target, path)?;
                            }
                        }
                    }
                    crate::config::HumanTool::Builtin(tool) => {
                        for argument in &tool.args {
                            if let Argument::Reference {
                                name: ref_name,
                                path,
                                ..
                            } = argument
                            {
                                let target = reference_target(config, owner, ref_name)?;
                                super::reference_path(config, target, path)?;
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
                config.error_at(
                    owner,
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
    owner: &crate::config::ArtifactName,
    args: &[Argument],
) -> Result<Vec<super::ResolvedArgument>, ScopeError> {
    args.iter()
        .map(|argument| {
            let Argument::Reference { prefix, name, path } = argument else {
                return Ok(super::ResolvedArgument::Literal(spelled(argument)));
            };
            let id = reference_target(config, owner, name)?;
            super::reference_path(config, id, path)?;
            super::argument_value(config, scope, id, path, prefix)
        })
        .collect()
}

/// Resolve builtin paths back to logical scope paths so reads keep no-follow access.
pub(crate) fn builtin_args(
    config: &RepoConfig,
    scope: &Scope<'_>,
    owner: &crate::config::ArtifactName,
    tool: &crate::config::HumanBuiltinTool,
) -> Result<(crate::config::ArtifactName, Vec<String>), ScopeError> {
    let mut args: Vec<String> = tool.args.iter().map(spelled).collect();
    if tool.builtin == crate::config::Builtin::Graph
        || tool.builtin == crate::config::Builtin::Help
        || tool.builtin == crate::config::Builtin::Open
            && matches!(&tool.args[0], Argument::Literal(text) if artifactize_tools::builtin::is_url(text))
    {
        return Ok((owner.clone(), args));
    }
    let id = if let Argument::Reference { prefix, name, path } = &tool.args[0] {
        if !prefix.is_empty() {
            return Err(ScopeError(
                "Builtin targets cannot have a flag prefix.".into(),
            ));
        }
        let target = reference_target(config, owner, name)?;
        super::reference_path(config, target, path)?;
        if tool.builtin == crate::config::Builtin::Open
            && config.artifacts[target].path.uri().is_some()
        {
            args[0] = super::argument_value(config, scope, target, path, "")?.native();
            if !artifactize_tools::builtin::is_url(&args[0]) {
                return Err(ScopeError(
                    "The open builtin accepts only HTTP(S) Live addresses.".into(),
                ));
            }
            return Ok((owner.clone(), args));
        }
        args[0] = path.to_string();
        target
    } else {
        owner
    };
    artifactize_tools::builtin::validate_target(
        tool.builtin,
        &args,
        &config.root,
        &scope.tool_scope(),
        &super::tool_id(id),
    )
    .map_err(ScopeError)?;
    Ok((id.clone(), args))
}
