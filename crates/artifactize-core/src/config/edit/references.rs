//! Rewrite or detect structural Artifact references without disturbing TOML decorations.

use super::*;

pub(super) fn rename(
    doc: &mut toml_edit::DocumentMut,
    artifact: &Artifact,
    old: &ArtifactName,
    new: &ArtifactName,
) {
    rename_item(doc.as_item_mut(), old, new, artifact, &mut Vec::new());
}

pub(super) fn names(
    doc: &toml_edit::DocumentMut,
    artifact: &Artifact,
    name: &ArtifactName,
) -> bool {
    names_artifact(doc.as_item(), artifact, name, &mut Vec::new())
}

fn rename_item(
    item: &mut toml_edit::Item,
    old: &str,
    new: &str,
    artifact: &Artifact,
    path: &mut Vec<String>,
) {
    if path.first().is_some_and(|key| key == "annotations") {
        return;
    }
    match item {
        toml_edit::Item::Value(value) => rename_value(value, old, new, artifact, path),
        toml_edit::Item::Table(table) => {
            for (key, item) in table.iter_mut() {
                path.push(key.to_string());
                rename_item(item, old, new, artifact, path);
                path.pop();
            }
        }
        toml_edit::Item::ArrayOfTables(tables) => {
            for table in tables.iter_mut() {
                for (key, item) in table.iter_mut() {
                    path.push(key.to_string());
                    rename_item(item, old, new, artifact, path);
                    path.pop();
                }
            }
        }
        toml_edit::Item::None => (),
    }
}
fn rename_value(
    value: &mut toml_edit::Value,
    old: &str,
    new: &str,
    artifact: &Artifact,
    path: &mut Vec<String>,
) {
    match value {
        toml_edit::Value::String(text) => {
            let original = text.value();
            let updated = if path.as_slice() == ["parts"] && original == old {
                new.to_owned()
            } else if let Some(field) = reference_field(path, artifact) {
                match field {
                    ReferenceField::Instruction => rename_references(original, old, new),
                    ReferenceField::Arguments => rename_argument(original, old, new),
                    ReferenceField::HumanArguments if old != "artifactPath" => {
                        rename_argument(original, old, new)
                    }
                    ReferenceField::HumanArguments => original.clone(),
                }
            } else {
                return;
            };
            if updated != *original {
                let decor = value.decor().clone();
                *value = toml_edit::Value::from(updated);
                *value.decor_mut() = decor;
            }
        }
        toml_edit::Value::Array(array) => {
            for value in array.iter_mut() {
                rename_value(value, old, new, artifact, path);
            }
        }
        toml_edit::Value::InlineTable(table) => {
            for (key, value) in table.iter_mut() {
                path.push(key.to_string());
                rename_value(value, old, new, artifact, path);
                path.pop();
            }
        }
        _ => (),
    }
}
fn names_artifact(
    item: &toml_edit::Item,
    artifact: &Artifact,
    name: &ArtifactName,
    keys: &mut Vec<String>,
) -> bool {
    let mut in_table = |table: &toml_edit::Table| {
        table.iter().any(|(key, item)| {
            keys.push(key.to_owned());
            let found = names_artifact(item, artifact, name, keys);
            keys.pop();
            found
        })
    };
    match item {
        toml_edit::Item::Table(table) => in_table(table),
        toml_edit::Item::ArrayOfTables(tables) => tables.iter().any(in_table),
        toml_edit::Item::Value(value) => names_artifact_value(value, artifact, name, keys),
        toml_edit::Item::None => false,
    }
}

fn names_artifact_value(
    value: &toml_edit::Value,
    artifact: &Artifact,
    name: &ArtifactName,
    keys: &mut Vec<String>,
) -> bool {
    if keys.first().is_some_and(|key| key == "annotations") {
        return false;
    }
    match value {
        toml_edit::Value::String(text) => {
            let text = text.value();
            let argument = || {
                matches!(text.parse::<crate::scope::Argument>(),
                Ok(crate::scope::Argument::Reference { name: target, .. }) if target.as_str() == name.as_str())
            };
            match reference_field(keys, artifact) {
                _ if keys.as_slice() == ["parts"] => text == name.as_str(),
                Some(ReferenceField::Instruction) => crate::scope::reference_spans(text)
                    .iter()
                    .any(|reference| reference.name.as_str() == name.as_str()),
                Some(ReferenceField::Arguments) => argument(),
                Some(ReferenceField::HumanArguments) if name.as_str() != "artifactPath" => {
                    argument()
                }
                _ => false,
            }
        }
        toml_edit::Value::Array(array) => array
            .iter()
            .any(|value| names_artifact_value(value, artifact, name, keys)),
        toml_edit::Value::InlineTable(table) => table.iter().any(|(key, value)| {
            keys.push(key.to_owned());
            let found = names_artifact_value(value, artifact, name, keys);
            keys.pop();
            found
        }),
        _ => false,
    }
}

enum ReferenceField {
    Instruction,
    Arguments,
    HumanArguments,
}
fn reference_field(path: &[String], artifact: &Artifact) -> Option<ReferenceField> {
    let keys = path.iter().map(String::as_str).collect::<Vec<_>>();
    match keys.as_slice() {
        ["evals", _, "payload", "instruction"] => Some(ReferenceField::Instruction),
        ["evals", _, "profile", "args"]
        | ["evals", _, "profile_variants", _, "args"]
        | ["views", "agent_sessions", _, "start" | "stop", "args"]
        | ["fingerprint", "script", "args"] => Some(ReferenceField::Arguments),
        ["views", "agent_tools", tool, "args"] => match artifact.views.agent_tools.get(*tool) {
            Some(AgentTool::Command(tool)) if tool.protocol == ToolProtocol::Json => {
                Some(ReferenceField::Arguments)
            }
            _ => None, // Plain placeholders and fixed builtin operands are not Artifact references.
        },
        ["views", "human_tools", tool, "args"] => match artifact.views.human_tools.get(*tool) {
            Some(HumanTool::Builtin(tool)) if tool.builtin == Builtin::Help => None,
            _ => Some(ReferenceField::HumanArguments),
        },
        _ => None,
    }
}
fn rename_argument(text: &str, old: &str, new: &str) -> String {
    match text.parse::<crate::scope::Argument>() {
        Ok(crate::scope::Argument::Reference { prefix, name, .. }) if name.as_str() == old => {
            let start = prefix.len() + 1;
            format!("{}{}{}", &text[..start], new, &text[start + old.len()..])
        }
        _ => text.to_owned(),
    }
}
fn rename_references(text: &str, old: &str, new: &str) -> String {
    let mut result = String::new();
    let mut start = 0;
    for reference in crate::scope::reference_spans(text)
        .into_iter()
        .filter(|reference| reference.name.as_str() == old)
    {
        result.push_str(&text[start..reference.start]);
        result.push('{');
        result.push_str(new);
        result.push('}');
        start = reference.end;
    }
    result.push_str(&text[start..]);
    result
}
