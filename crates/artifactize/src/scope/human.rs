use super::{Argument, RepoConfig, Scope, ScopeError, reference_target};

/// Human declarations reject stray braces at their input edge, including extra braces in a
/// parsed reference's path. Runtime and Agent arguments deliberately remain permissive.
pub(crate) fn human_arguments<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<Argument>, D::Error> {
    use serde::{Deserialize, de::Error};
    Vec::<String>::deserialize(deserializer)?
        .into_iter()
        .map(|text| {
            let argument: Argument = text.parse().map_err(D::Error::custom)?;
            validate_human_argument(&argument).map_err(D::Error::custom)?;
            Ok(argument)
        })
        .collect()
}

pub(crate) fn validate_human_argument(argument: &Argument) -> Result<(), ScopeError> {
    let stray = match argument {
        Argument::Literal(text) => text.contains(['{', '}']),
        Argument::Reference { prefix, path, .. } => {
            prefix.contains(['{', '}']) || path.contains(['{', '}'])
        }
    };
    if stray {
        return Err(ScopeError(
            "Human arguments support only {artifactPath} or {name}[/path] scope placeholders (optionally --flag=)."
                .into(),
        ));
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
    let mut args: Vec<String> = tool.args.iter().map(ToString::to_string).collect();
    if tool.builtin == crate::config::Builtin::Help
        || tool.builtin == crate::config::Builtin::Open
            && matches!(&tool.args[0], Argument::Literal(text) if artifactize_tools::builtin::is_url(text))
    {
        return Ok((owner.into(), args));
    }
    let id = if let Argument::Reference { prefix, name, path } = &tool.args[0] {
        if !prefix.is_empty() {
            return Err(ScopeError(
                "Builtin targets cannot have a flag prefix.".into(),
            ));
        }
        let target = reference_target_or_owner(config, owner, name)?;
        super::reference_path(config, target, path)?;
        args[0] = path.to_string();
        target.as_str()
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
