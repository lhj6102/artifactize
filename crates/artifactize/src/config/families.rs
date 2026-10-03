//! Static family declarations, parameters, variants, and instance expansion.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use serde::Deserialize;
use serde_json::{Map, Value};

use super::{ArtifactDeclaration, CONFIG_FILE, identifier, validation};
use crate::scope::scoped_path;

pub const MAX_FAMILY_INSTANCES: usize = 10_000;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FamilyDeclaration {
    pub instances: InstanceSource,
    #[serde(default)]
    pub params: Map<String, Value>,
    #[serde(default)]
    pub variants: BTreeMap<String, Map<String, Value>>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum InstanceSource {
    File(String),
    Inline(BTreeMap<String, InstanceDeclaration>),
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InstanceDeclaration {
    #[serde(default, deserialize_with = "validation::present")]
    pub variant: Option<String>,
    #[serde(default)]
    pub params: Map<String, Value>,
    #[serde(default)]
    pub material: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FamilyMembership {
    pub name: String,
    /// Owner-relative list file, when instances are not declared inline.
    pub instances: Option<String>,
    /// Sorted owner-relative material; scripts still see the shared physical folder.
    pub material: Vec<String>,
}

pub(super) fn parameterized(value: &Value) -> bool {
    match value {
        Value::Array(items) => items.iter().any(parameterized),
        Value::Object(object) => {
            object.contains_key("$param") || object.values().any(parameterized)
        }
        _ => false,
    }
}

fn parameter<'a>(params: &'a Value, pointer: &str) -> Result<&'a Value, String> {
    if (!pointer.is_empty() && !pointer.starts_with('/'))
        || pointer
            .split('~')
            .skip(1)
            .any(|suffix| !suffix.starts_with(['0', '1']))
    {
        return Err("$param must be an RFC 6901 JSON Pointer using only ~0 and ~1 escapes.".into());
    }
    params
        .pointer(pointer)
        .ok_or_else(|| format!("No parameter at {pointer:?}."))
}

fn substitute(value: &mut Value, params: &Value) -> Result<(), String> {
    match value {
        Value::Array(items) => {
            for item in items {
                substitute(item, params)?;
            }
        }
        Value::Object(object) => {
            if let Some(pointer) = object.get("$param") {
                let pointer = pointer.as_str().filter(|_| object.len() == 1).ok_or(
                    "A parameter reference must be exactly {\"$param\": \"/json/pointer\"}.",
                )?;
                *value = parameter(params, pointer)?.clone();
            } else {
                for item in object.values_mut() {
                    substitute(item, params)?;
                }
            }
        }
        _ => {}
    }
    Ok(())
}

pub(super) fn expand(
    root: &Path,
    relative: &Path,
    mut template: Map<String, Value>,
) -> Result<Vec<(ArtifactDeclaration, FamilyMembership)>, String> {
    if relative.as_os_str().is_empty() {
        return Err(
            "An Artifact family cannot be the workspace root; place it in a subfolder.".into(),
        );
    }
    let name = template
        .get("name")
        .and_then(Value::as_str)
        .ok_or("Artifact family name must be a safe identifier.")?
        .to_owned();
    identifier(&name, "Artifact family name")?;
    if template.contains_key("reviewPolicy") {
        return Err("An Artifact family cannot declare reviewPolicy.".into());
    }
    let family: FamilyDeclaration = serde_json::from_value(template.remove("family").unwrap())
        .map_err(|error| error.to_string())?;
    for variant in family.variants.keys() {
        identifier(variant, "Family variant name")?;
    }
    let owner = root.join(relative);
    let (instances, file) = match family.instances {
        InstanceSource::Inline(instances) => (instances, None),
        InstanceSource::File(file) => {
            validation::path(&file)?;
            if file == CONFIG_FILE {
                return Err(format!(
                    "family.instances must name a file other than {CONFIG_FILE}."
                ));
            }
            let path = scoped_path(&owner, Path::new(&file)).map_err(|error| error.to_string())?;
            if !path.is_file() {
                return Err(format!("Instance list {file} must be a regular file."));
            }
            let text = fs::read_to_string(&path).map_err(|error| error.to_string())?;
            let instances = serde_json::from_str(&text)
                .map_err(|error| format!("Instance list {file}: {error}"))?;
            (instances, Some(file))
        }
    };
    if instances.is_empty() || instances.len() > MAX_FAMILY_INSTANCES {
        return Err(format!(
            "family.instances must list 1–{MAX_FAMILY_INSTANCES} instances by Artifact name."
        ));
    }
    let physical = fs::read_dir(&owner)
        .map_err(|error| error.to_string())?
        .map(|entry| entry.map(|entry| entry.file_name()))
        .collect::<Result<BTreeSet<_>, _>>()
        .map_err(|error| error.to_string())?;
    let mut members = Vec::with_capacity(instances.len());
    for (id, instance) in instances {
        let member = (|| {
            identifier(&id, "Instance name")?;
            if id == name {
                return Err("An instance cannot reuse its family name.".into());
            }
            if physical.contains(std::ffi::OsStr::new(&id)) {
                return Err("Instance name conflicts with a physical entry in its family folder.".into());
            }
            validation::paths(&instance.material, "Instance material")?;
            for material in &instance.material {
                if material == CONFIG_FILE || Some(material) == file.as_ref() {
                    return Err("Instance material cannot claim the family declaration or instance list.".into());
                }
                scoped_path(&owner, Path::new(material)).map_err(|error| {
                    format!("Material {material} must exist inside its family folder without symlinks: {error}")
                })?;
            }
            let mut params = family.params.clone();
            if let Some(variant) = instance.variant {
                params.extend(
                    family.variants.get(&variant)
                        .ok_or_else(|| format!("Unknown family variant: {variant}."))?
                        .clone(),
                );
            }
            params.extend(instance.params);
            let params = Value::Object(params);
            let mut expanded = template.clone();
            expanded.insert("name".into(), Value::String(id.clone()));
            for key in ["views", "evals"] {
                if let Some(value) = expanded.get_mut(key) {
                    substitute(value, &params)?;
                }
            }
            let declaration = super::validated_declaration(Value::Object(expanded))?;
            let mut material = instance.material;
            material.sort();
            Ok((declaration, FamilyMembership {
                name: name.clone(), instances: file.clone(), material,
            }))
        })().map_err(|error: String| format!("Instance {id}: {error}"))?;
        members.push(member);
    }
    Ok(members)
}

#[cfg(test)]
mod tests;
