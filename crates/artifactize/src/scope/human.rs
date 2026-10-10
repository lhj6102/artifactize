use super::{
    ArgumentReference, RepoConfig, Scope, ScopeError, argument_reference, reference_target,
};

fn reference(argument: &str) -> Result<Option<ArgumentReference<'_>>, ScopeError> {
    let reference = argument_reference(argument)?;
    if argument.contains(['{', '}'])
        && (reference.is_none()
            || argument.bytes().filter(|byte| *byte == b'{').count() != 1
            || argument.bytes().filter(|byte| *byte == b'}').count() != 1)
    {
        return Err(ScopeError(
            "Human arguments support only {artifactPath} or {name}[/path] scope placeholders (optionally --flag=)."
                .into(),
        ));
    }
    Ok(reference)
}

pub(crate) fn validate_human_args(args: &[String]) -> Result<(), ScopeError> {
    for argument in args {
        reference(argument)?;
    }
    Ok(())
}

pub(super) fn validate_references(config: &RepoConfig) -> Result<(), super::ConfigError> {
    for (owner, artifact) in &config.artifacts {
        for (name, tool) in &artifact.views.human_tools {
            let validate = || {
                for argument in tool.args() {
                    if let Some(reference) = reference(argument)? {
                        let target = if reference.name == "artifactPath" {
                            &config.artifacts[owner.as_str()].name
                        } else {
                            reference_target(config, owner, reference.name)?
                        };
                        super::reference_path(config, target, reference.path)?;
                    }
                }
                if let crate::config::HumanTool::Builtin(tool) = tool {
                    let scope = Scope {
                        artifacts: config
                            .artifacts
                            .iter()
                            .map(|(id, artifact)| (id.clone(), artifact))
                            .collect(),
                    };
                    builtin_args(config, &scope, owner, tool)?;
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
    args: &[String],
) -> Result<Vec<String>, ScopeError> {
    args.iter()
        .map(|argument| {
            let Some(reference) = reference(argument)? else {
                return Ok(argument.clone());
            };
            let id = if reference.name == "artifactPath" {
                &config.artifacts[owner].name
            } else {
                reference_target(config, owner, reference.name)?
            };
            super::reference_path(config, id, reference.path)?;
            let path = scope.resolve_input(&config.root, id, reference.path)?;
            path.to_str()
                .ok_or_else(|| ScopeError("Artifact paths must be UTF-8.".into()))?;
            let path = crate::platform::path_text(&path);
            Ok(format!("{}{path}", reference.prefix))
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
    let id = if let Some(reference) = reference(&args[0])? {
        if !reference.prefix.is_empty() {
            return Err(ScopeError(
                "Builtin targets cannot have a flag prefix.".into(),
            ));
        }
        let target = if reference.name == "artifactPath" {
            &config.artifacts[owner].name
        } else {
            reference_target(config, owner, reference.name)?
        };
        super::reference_path(config, target, reference.path)?;
        args[0] = reference.path.to_owned();
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
