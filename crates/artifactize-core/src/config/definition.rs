//! Kind-specific defaults and validation shared by discovery and migration.
use super::*;
pub(super) struct Input<'a> {
    pub kind: ArtifactKind,
    pub path: &'a Path,
    pub file: &'a Path,
    pub source: &'a str,
    pub name: Option<&'a ArtifactName>,
}
/// Parse and validate the declaration a walk found, as its kind declares it.
pub(super) fn parse(
    found: &Input<'_>,
    schemas: &mut super::schemas::Schemas,
) -> Result<ArtifactDeclaration, ConfigError> {
    let file = found.file;
    let source = &found.source;
    let parse = |schemas: &mut super::schemas::Schemas| {
        parse_declaration_named(source, schemas, found.name)
    };
    if found.kind != ArtifactKind::Aspect && selects(source) {
        return Err(found_error(
            found,
            &["select"],
            "select declares an aspect, which only a <label>.artf whose label names no file declares.",
        ));
    }
    match found.kind {
        ArtifactKind::Folder => parse(schemas).map_err(|error| ConfigError::new(file, error)),
        ArtifactKind::Aspect => {
            let mut declaration = parse(schemas).map_err(|error| ConfigError::new(file, error))?;
            let select = declaration.select.clone().unwrap_or_default();
            if !declares(source, "fingerprint") {
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
            let table: toml::Table =
                toml::from_str(source).map_err(|error| ConfigError::new(file, error))?;
            if table.contains_key("fingerprint") {
                return Err(found_error(
                    found,
                    &["fingerprint"],
                    "The graph Artifact cannot declare fingerprint.",
                ));
            }
            if table.contains_key("parts") {
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
            let value =
                toml::de::DeTable::parse(source).map_err(|error| ConfigError::new(file, error))?;
            if value.get_ref().get("fingerprint").is_none() {
                declaration.fingerprint = Some(Fingerprint::Artifactsum {
                    files: vec![target.parse().expect("validated target logical path")],
                    ignore: Default::default(),
                });
            }
            if let Some(Fingerprint::Artifactsum { files, .. }) = &declaration.fingerprint {
                if value
                    .get_ref()
                    .get("fingerprint")
                    .and_then(|value| value.get_ref().get("ignore"))
                    .is_some()
                {
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

/// Whether a declaration's source declares `select`, which makes it an aspect.
pub(super) fn selects(source: &str) -> bool {
    declares(source, "select")
}

/// Whether a declaration's source has the top-level `key`. A source that does not parse has
/// none; parsing it reports why.
fn declares(source: &str, key: &str) -> bool {
    toml::de::DeTable::parse(source).is_ok_and(|table| table.get_ref().contains_key(key))
}
