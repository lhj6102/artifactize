//! One candidate edit implementation for both preview footprints and transactional writes.
use super::*;

pub(super) fn propose(
    root: &Path,
    sources: &BTreeMap<PathBuf, String>,
    operation: Operation,
) -> Result<BTreeMap<PathBuf, String>, Refusal> {
    let mut proposed = sources.clone();
    // Source can repair a broken definition after its current text has been reloaded.
    // Every operation still validates the complete prospective project before writing.
    let config = if let Operation::SaveSource { name, source } = &operation {
        let file = sources
            .keys()
            .find(|file| file.file_stem().and_then(|stem| stem.to_str()) == Some(name.as_str()))
            .ok_or_else(|| forbidden(format!("Unknown Artifact {name}.")))?;
        proposed.insert(file.clone(), source.clone());
        discovery::read_sources(root.to_owned(), &proposed)?
    } else {
        discovery::read_sources(root.to_owned(), sources)?
    };
    match operation {
        Operation::Create { name, path } => {
            discovery::definition_name(&name).map_err(forbidden)?;
            if config
                .artifacts
                .keys()
                .any(|existing| existing.eq_ignore_ascii_case(&name))
            {
                return Err(forbidden(format!("Artifact {name} already exists.")));
            }
            let mut doc = toml_edit::DocumentMut::new();
            doc["path"] = toml_edit::value(path.as_str());
            proposed.insert(
                root.join(format!(".artifactize/{name}.artf")),
                doc.to_string(),
            );
        }
        Operation::SetPath { name, path } => update(&config, &mut proposed, &name, |doc, _| {
            if doc["path"].as_str() != Some(path.as_str()) {
                let decor = doc["path"]
                    .as_value()
                    .expect("validated PATH")
                    .decor()
                    .clone();
                let mut value = toml_edit::Value::from(path.as_str());
                *value.decor_mut() = decor;
                doc["path"] = toml_edit::Item::Value(value);
            }
            Ok(())
        })?,
        Operation::SaveSource { .. } => {}
        Operation::AddPart { whole, part, at } => {
            let at = at.or_else(|| placement(&config, &proposed, &part, None));
            check_style(&config, &mut proposed, &whole)?;
            check_style(&config, &mut proposed, &part)?;
            update(&config, &mut proposed, &whole, |doc, _| {
                change_part(doc, &part, true)
            })?;
            let next = discovery::read_sources(root.to_owned(), &proposed)?;
            update(&config, &mut proposed, &whole, |doc, file| {
                annotations::set_parts(
                    doc,
                    file,
                    &next,
                    &whole,
                    &at.map(|at| [(part.clone(), at)].into()).unwrap_or_default(),
                    false,
                )
            })?;
            update(&config, &mut proposed, &part, |doc, file| {
                annotations::set_at(doc, file, &next, &part, None)
            })?;
        }
        Operation::RemovePart { whole, part } => {
            check_style(&config, &mut proposed, &whole)?;
            update(&config, &mut proposed, &whole, |doc, _| {
                change_part(doc, &part, false)
            })?;
            let next = discovery::read_sources(root.to_owned(), &proposed)?;
            update(&config, &mut proposed, &whole, |doc, file| {
                annotations::set_parts(doc, file, &next, &whole, &BTreeMap::new(), false)
            })?;
        }
        Operation::MovePart { part, from, to, at } => {
            let at = at.or_else(|| placement(&config, &proposed, &part, from.as_ref()));
            check_style(&config, &mut proposed, &part)?;
            for whole in from.iter().chain(to.iter()) {
                check_style(&config, &mut proposed, whole)?;
            }
            if from == to {
                return Err(forbidden("The source and destination whole are the same."));
            }
            if let Some(from) = &from {
                update(&config, &mut proposed, from, |doc, _| {
                    change_part(doc, &part, false)
                })?;
            }
            if let Some(to) = &to {
                update(&config, &mut proposed, to, |doc, _| {
                    change_part(doc, &part, true)
                })?;
            }
            let next = discovery::read_sources(root.to_owned(), &proposed)?;
            if let Some(from) = from {
                update(&config, &mut proposed, &from, |doc, file| {
                    annotations::set_parts(doc, file, &next, &from, &BTreeMap::new(), false)
                })?;
            }
            if let Some(to) = &to {
                update(&config, &mut proposed, to, |doc, file| {
                    annotations::set_parts(
                        doc,
                        file,
                        &next,
                        to,
                        &at.map(|at| [(part.clone(), at)].into()).unwrap_or_default(),
                        false,
                    )
                })?;
            }
            update(&config, &mut proposed, &part, |doc, file| {
                annotations::set_at(
                    doc,
                    file,
                    &next,
                    &part,
                    if to.is_none() { at } else { None },
                )
            })?;
        }
        Operation::Rename { old, new } => {
            discovery::definition_name(&new).map_err(forbidden)?;
            if config
                .artifacts
                .keys()
                .any(|existing| existing.eq_ignore_ascii_case(&new))
            {
                return Err(forbidden(format!("Artifact {new} already exists.")));
            }
            let old_file = node_file(&config, &old)?;
            for (owner, artifact) in &config.artifacts {
                let file = root.join(artifact.declaration_path());
                let mut doc = proposed[&file]
                    .parse::<toml_edit::DocumentMut>()
                    .expect("validated source");
                references::rename(&mut doc, artifact, &old, &new);
                if artifact.parts.contains(&old) {
                    annotations::rename_part(&mut doc, &file, &config, owner, &old, &new)?;
                }
                proposed.insert(file, doc.to_string());
            }
            let source = proposed.remove(&old_file).expect("known node");
            proposed.insert(old_file.with_file_name(format!("{new}.artf")), source);
        }
        Operation::Delete { name } => {
            let file = node_file(&config, &name)?;
            let users = config
                .artifacts
                .iter()
                .filter_map(|(other, artifact)| {
                    if other == &name {
                        return None;
                    }
                    let doc = sources[&root.join(artifact.declaration_path())]
                        .parse::<toml_edit::DocumentMut>()
                        .expect("validated source");
                    references::names(&doc, artifact, &name).then(|| other.to_string())
                })
                .collect::<Vec<_>>();
            if !users.is_empty() {
                return Err(forbidden(format!(
                    "{name} is still named by: {}.",
                    users.join(", ")
                )));
            }
            proposed.remove(&file);
        }
        Operation::SavePlaces { places } => {
            let mut boxes = BTreeMap::<ArtifactName, BTreeMap<ArtifactName, [f64; 2]>>::new();
            for (key, at) in places {
                match key {
                    PlacementKey::Part { whole, part } => {
                        if config.artifacts.contains_key(&whole) {
                            boxes.entry(whole).or_default().insert(part, at);
                        }
                    }
                    PlacementKey::Node(name) => {
                        if config.artifacts.contains_key(&name) {
                            update(&config, &mut proposed, &name, |doc, file| {
                                annotations::set_at(doc, file, &config, &name, Some(at))
                            })?;
                        }
                    }
                }
            }
            for (whole, places) in boxes {
                update(&config, &mut proposed, &whole, |doc, file| {
                    annotations::set_parts(doc, file, &config, &whole, &places, false)
                })?;
            }
        }
        Operation::ResetPlaces { whole } => {
            if let Some(whole) = whole {
                update(&config, &mut proposed, &whole, |doc, file| {
                    annotations::set_parts(doc, file, &config, &whole, &BTreeMap::new(), true)
                })?;
            } else {
                for name in config.artifacts.keys().filter(|name| {
                    !config
                        .artifacts
                        .values()
                        .any(|artifact| artifact.parts.contains(*name))
                }) {
                    update(&config, &mut proposed, name, |doc, file| {
                        annotations::set_at(doc, file, &config, name, None)
                    })?;
                }
            }
        }
        Operation::SetComment { name, text } => {
            update(&config, &mut proposed, &name, |doc, file| {
                annotations::set_comment(doc, file, &text)
            })?
        }
        Operation::MoveComment { from, to } => {
            if from == to {
                return Err(forbidden("A comment already belongs to this node."));
            }
            let from_file = node_file(&config, &from)?;
            let to_file = node_file(&config, &to)?;
            let source = proposed[&from_file]
                .parse::<toml_edit::DocumentMut>()
                .expect("validated source");
            let target = proposed[&to_file]
                .parse::<toml_edit::DocumentMut>()
                .expect("validated source");
            let text = annotations::take_comment(&source, &from_file)?
                .ok_or_else(|| forbidden(format!("{from} has no comment to move.")))?;
            if annotations::take_comment(&target, &to_file)?.is_some() {
                return Err(forbidden(format!("{to} already has a comment.")));
            }
            update(&config, &mut proposed, &from, |doc, file| {
                annotations::set_comment(doc, file, "")
            })?;
            update(&config, &mut proposed, &to, |doc, file| {
                annotations::set_comment(doc, file, &text)
            })?;
        }
    }
    Ok(proposed)
}

pub(super) fn changes(
    sources: &BTreeMap<PathBuf, String>,
    proposed: &BTreeMap<PathBuf, String>,
) -> BTreeMap<PathBuf, Option<String>> {
    sources
        .keys()
        .chain(proposed.keys())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .filter(|&file| sources.get(file) != proposed.get(file))
        .map(|file| (file.clone(), proposed.get(file).cloned()))
        .collect()
}
