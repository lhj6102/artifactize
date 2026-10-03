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
            "Human arguments support only {artifactPath} or {name}[/path] scope placeholders (optionally --flag=).".into(),
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
                for argument in &tool.args {
                    if let Some(reference) = reference(argument)?
                        && reference.name != "artifactPath"
                    {
                        reference_target(config, owner, reference.name)?;
                    }
                }
                Ok::<_, ScopeError>(())
            };
            validate().map_err(|error| {
                super::ConfigError::new(
                    config.root.join(&artifact.path).join(super::CONFIG_FILE),
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
                owner
            } else {
                reference_target(config, owner, reference.name)?
            };
            let path = scope.resolve_input(&config.root, id, reference.path)?;
            let path = path
                .to_str()
                .ok_or_else(|| ScopeError("Artifact paths must be UTF-8.".into()))?;
            Ok(format!("{}{path}", reference.prefix))
        })
        .collect()
}
