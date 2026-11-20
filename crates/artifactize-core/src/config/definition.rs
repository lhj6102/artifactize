//! Kind-specific defaults and validation shared by discovery and migration.
use super::*;
pub(super) struct Input<'a> {
    pub kind: ArtifactKind,
    pub path: &'a Path,
    pub file: &'a Path,
    pub source: &'a str,
    pub name: Option<&'a ArtifactName>,
}
/// Validate a parsed declaration as its kind declares it.
pub(super) fn parse_table(
    found: &Input<'_>,
    schemas: &mut super::schemas::Schemas,
    table: toml::Spanned<toml::de::DeTable<'_>>,
) -> Result<ArtifactDeclaration, ConfigError> {
    let file = found.file;
    let source = found.source;
    let fields = table.get_ref();
    let has_fingerprint = fields.contains_key("fingerprint");
    let has_parts = fields.contains_key("parts");
    let has_select = fields.contains_key("select");
    let has_file_ignore = fields
        .get("fingerprint")
        .and_then(|value| value.get_ref().get("ignore"))
        .is_some();
    let parse = |schemas: &mut super::schemas::Schemas| {
        declaration::parse_declaration_table(source, schemas, found.name, table)
    };
    if found.kind != ArtifactKind::Aspect && has_select {
        return Err(found_error(
            found,
            &["select"],
            "select declares an aspect, which only a <label>.artf whose label names no file declares.",
        ));
    }
    match found.kind {
        ArtifactKind::Folder => parse(schemas).map_err(|error| ConfigError::new(file, error)),
        ArtifactKind::Live => {
            let mut declaration = parse(schemas).map_err(|error| ConfigError::new(file, error))?;
            if !has_fingerprint {
                declaration.fingerprint = None;
            }
            if matches!(
                declaration.fingerprint,
                Some(Fingerprint::Artifactsum { .. } | Fingerprint::Graph)
            ) {
                return Err(found_error(
                    found,
                    &["fingerprint"],
                    "A Live Artifact uses a fingerprint script or no snapshot, never artifactsum.",
                ));
            }
            let file_builtin = |builtin: Builtin| {
                !matches!(builtin, Builtin::Graph | Builtin::Help | Builtin::Open)
            };
            if declaration
                .views
                .agent_tools
                .values()
                .any(|tool| matches!(tool, AgentTool::Builtin(tool) if file_builtin(tool.builtin)))
                || declaration.views.human_tools.values().any(
                    |tool| matches!(tool, HumanTool::Builtin(tool) if file_builtin(tool.builtin)),
                )
            {
                return Err(found_error(
                    found,
                    &["views"],
                    "File builtins cannot read a Live Artifact; use read-only observation commands.",
                ));
            }
            Ok(declaration)
        }
        ArtifactKind::Aspect => {
            let mut declaration = parse(schemas).map_err(|error| ConfigError::new(file, error))?;
            let select = declaration.select.clone().unwrap_or_default();
            if !has_fingerprint {
                // Without a fingerprint of its own, an aspect hashes what it selects.
                declaration.fingerprint = Some(Fingerprint::Artifactsum {
                    files: select,
                    ignore: Default::default(),
                });
            } else if let Some(Fingerprint::Artifactsum { files, .. }) = &declaration.fingerprint {
                let within = |input: &str| {
                    select.iter().any(|selected| {
                        input
                            .strip_prefix(selected.as_str())
                            .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
                    })
                };
                if let Some(outside) = files.iter().find(|input| !within(input)) {
                    return Err(found_error(
                        found,
                        &["fingerprint", "files"],
                        format!(
                            "fingerprint.files for an aspect must lie in its selection: {outside}."
                        ),
                    ));
                }
            }
            Ok(declaration)
        }
        ArtifactKind::Graph => {
            if has_fingerprint {
                return Err(found_error(
                    found,
                    &["fingerprint"],
                    "The graph Artifact cannot declare fingerprint.",
                ));
            }
            if has_parts {
                return Err(found_error(
                    found,
                    &["parts"],
                    "The graph Artifact is made of nothing; it cannot declare parts.",
                ));
            }
            let mut declaration = parse(schemas).map_err(|error| ConfigError::new(file, error))?;
            declaration.fingerprint = Some(Fingerprint::Graph);
            Ok(declaration)
        }
        ArtifactKind::File => {
            let target = found
                .path
                .file_name()
                .and_then(|target| target.to_str())
                .expect("a walk names file Artifacts by their UTF-8 target");
            let mut declaration = parse(schemas).map_err(|error| ConfigError::new(file, error))?;
            if !has_fingerprint {
                declaration.fingerprint = Some(Fingerprint::Artifactsum {
                    files: vec![target.parse().expect("validated target logical path")],
                    ignore: Default::default(),
                });
            }
            if let Some(Fingerprint::Artifactsum { files, .. }) = &declaration.fingerprint {
                if has_file_ignore {
                    return Err(found_error(
                        found,
                        &["fingerprint", "ignore"],
                        "fingerprint.ignore is not supported for a file Artifact.",
                    ));
                }
                if files.iter().any(|input| input != target) {
                    return Err(found_error(
                        found,
                        &["fingerprint", "files"],
                        format!(
                            "fingerprint.files for a file Artifact may name only its target {target}."
                        ),
                    ));
                }
            }
            Ok(declaration)
        }
    }
}

fn found_error(found: &Input<'_>, keys: &[&str], message: impl ToString) -> ConfigError {
    ConfigError::new(
        found.file,
        location::error_at(found.source, keys, &message.to_string()),
    )
}
