//! Project selectors, selection files, and profile variants.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs::OpenOptions,
    io::Read,
    os::unix::fs::OpenOptionsExt,
    path::Path,
};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::config::{Critic, RepoConfig, identifier};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase", deny_unknown_fields)]
pub enum Selection {
    All,
    Artifact {
        #[serde(rename = "artifactId")]
        artifact_id: String,
    },
    Critic {
        #[serde(rename = "criticId")]
        critic_id: String,
    },
    Artifacts {
        #[serde(rename = "artifactIds")]
        artifact_ids: Vec<String>,
    },
    Critics {
        #[serde(rename = "criticIds")]
        critic_ids: Vec<String>,
    },
}

#[derive(Debug)]
pub struct ResolvedSelection<'a> {
    pub critics: Vec<&'a Critic>,
    pub roots: Vec<&'a str>,
}

impl Selection {
    /// Preserve selector order, then declaration order, keeping each root/Critic once.
    pub fn resolve<'a>(&self, config: &'a RepoConfig) -> Result<ResolvedSelection<'a>, String> {
        let critics_by_id: BTreeMap<_, _> = config
            .critics
            .iter()
            .map(|critic| (critic.id.as_str(), critic))
            .collect();
        let mut critics_by_target: BTreeMap<&str, Vec<&Critic>> = BTreeMap::new();
        for critic in &config.critics {
            critics_by_target
                .entry(&critic.target)
                .or_default()
                .push(critic);
        }
        let mut result = ResolvedSelection {
            critics: Vec::new(),
            roots: Vec::new(),
        };
        match self {
            Self::All => {
                result.critics.extend(&config.critics);
                result
                    .roots
                    .extend(config.artifacts.keys().map(String::as_str));
            }
            Self::Artifact { artifact_id } => {
                result
                    .roots
                    .extend(selected_artifacts(config, artifact_id)?);
            }
            Self::Artifacts { artifact_ids } => {
                nonempty(artifact_ids)?;
                for id in artifact_ids {
                    result.roots.extend(selected_artifacts(config, id)?);
                }
            }
            Self::Critic { critic_id } => {
                result.critics.push(
                    *critics_by_id
                        .get(critic_id.as_str())
                        .ok_or_else(|| format!("Unknown Critic: {critic_id}"))?,
                );
            }
            Self::Critics { critic_ids } => {
                nonempty(critic_ids)?;
                for id in critic_ids {
                    result.critics.push(
                        *critics_by_id
                            .get(id.as_str())
                            .ok_or_else(|| format!("Unknown Critic: {id}"))?,
                    );
                }
            }
        }
        if matches!(self, Self::Critic { .. } | Self::Critics { .. }) {
            result
                .roots
                .extend(result.critics.iter().map(|critic| critic.target.as_str()));
        }
        let mut seen = BTreeSet::new();
        result.roots.retain(|id| seen.insert(*id));
        if matches!(self, Self::Artifact { .. } | Self::Artifacts { .. }) {
            for id in &result.roots {
                if let Some(critics) = critics_by_target.get(id) {
                    result.critics.extend(critics);
                }
            }
        }
        seen.clear();
        result
            .critics
            .retain(|critic| seen.insert(critic.id.as_str()));
        Ok(result)
    }
}

fn nonempty(ids: &[String]) -> Result<(), String> {
    if ids.is_empty() {
        return Err("Multi-root selection requires a nonempty array of IDs.".into());
    }
    Ok(())
}

fn selected_artifacts<'a>(config: &'a RepoConfig, id: &str) -> Result<Vec<&'a str>, String> {
    config
        .artifacts
        .get_key_value(id)
        .map(|(id, _)| vec![id.as_str()])
        .ok_or_else(|| format!("Unknown Artifact: {id}"))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ProfileSelection {
    Named(String),
    Critics(BTreeMap<String, String>),
}

/// Resolve complete declared profiles in owned configuration, never rewriting source files.
pub fn select_profiles(
    mut config: RepoConfig,
    selection: &Selection,
    profile: Option<&ProfileSelection>,
) -> Result<RepoConfig, String> {
    let Some(profile) = profile else {
        return Ok(config);
    };
    let selected = selection.resolve(&config)?;
    let included: BTreeMap<_, _> = selected
        .critics
        .iter()
        .map(|critic| (critic.id.as_str(), *critic))
        .collect();
    let mapping: Vec<(&str, &str)> = match profile {
        ProfileSelection::Named(name) => selected
            .critics
            .iter()
            .map(|critic| (critic.id.as_str(), name.as_str()))
            .collect(),
        ProfileSelection::Critics(mapping) => mapping
            .iter()
            .map(|(id, name)| (id.as_str(), name.as_str()))
            .collect(),
    };
    let mut profiles = BTreeMap::new();
    for (id, name) in mapping {
        let critic = included.get(id).ok_or_else(|| {
            format!("Profile selection is outside the submitted Critic scope: {id}")
        })?;
        let variant = critic
            .declaration
            .profile_variants
            .get(name)
            .filter(|_| identifier(name, "Profile variant name").is_ok())
            .ok_or_else(|| format!("Unknown profile variant for {id}: {name}"))?;
        profiles.insert(id.to_owned(), variant.clone());
    }
    for critic in &mut config.critics {
        if let Some(profile) = profiles.remove(&critic.id) {
            critic.declaration.profile = profile;
        }
    }
    // Runtime variants can introduce or remove argv references and dependency gates.
    crate::scope::resolve_config(&mut config).map_err(|error| error.to_string())?;
    Ok(config)
}

const MAX_FILE_BYTES: usize = 4 * 1024 * 1024;
const MAX_IDS: usize = 100_000;
const FILE_FORMAT_ERROR: &str =
    "Selection file must contain 1–100000 nonempty IDs, as a JSON string array or one ID per line.";

// Match ECMAScript trim, including BOM but excluding NEXT LINE.
fn trim(text: &str) -> &str {
    text.trim_matches(|ch| {
        matches!(ch,
            '\u{0009}'..='\u{000d}' | '\u{0020}' | '\u{00a0}' | '\u{1680}' |
            '\u{2000}'..='\u{200a}' | '\u{2028}' | '\u{2029}' | '\u{202f}' |
            '\u{205f}' | '\u{3000}' | '\u{feff}'
        )
    })
}

pub fn parse_selection_file(text: &str) -> Result<Vec<String>, String> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    if text.len() > MAX_FILE_BYTES {
        return Err("Selection file exceeds 4 MiB.".into());
    }
    let ids: Vec<String> = if trim(text).starts_with('[') {
        let value: Value = serde_json::from_str(text)
            .map_err(|_| "Selection file contains invalid JSON.".to_owned())?;
        let values = value.as_array().ok_or(FILE_FORMAT_ERROR)?;
        if values.is_empty() || values.len() > MAX_IDS {
            return Err(FILE_FORMAT_ERROR.into());
        }
        values
            .iter()
            .map(|value| {
                value
                    .as_str()
                    .map(str::to_owned)
                    .ok_or_else(|| FILE_FORMAT_ERROR.to_owned())
            })
            .collect::<Result<_, _>>()?
    } else {
        text.split('\n')
            .map(trim)
            .filter(|line| !line.is_empty())
            .take(MAX_IDS + 1)
            .map(str::to_owned)
            .collect()
    };
    if ids.is_empty()
        || ids.len() > MAX_IDS
        || ids.iter().any(|id| {
            id.is_empty()
                || trim(id) != id
                || id
                    .bytes()
                    .any(|byte| byte <= b' ' || byte == 127 || byte == b',')
        })
    {
        return Err(FILE_FORMAT_ERROR.into());
    }
    let mut seen = BTreeSet::new();
    Ok(ids
        .into_iter()
        .filter(|id| seen.insert(id.clone()))
        .collect())
}

pub fn read_selection_file(path: &Path) -> Result<Vec<String>, String> {
    // Nonblocking open lets us reject a FIFO without waiting for its writer.
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(path)
        .map_err(|error| format!("{}: {error}", path.display()))?;
    let info = file.metadata().map_err(|error| error.to_string())?;
    if !info.is_file() || info.len() > MAX_FILE_BYTES as u64 {
        return Err("Selection input must be a regular file no larger than 4 MiB.".into());
    }
    let mut bytes = Vec::new();
    file.take(MAX_FILE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() > MAX_FILE_BYTES {
        return Err("Selection file exceeds 4 MiB.".into());
    }
    parse_selection_file(&String::from_utf8_lossy(&bytes))
}

#[cfg(test)]
mod tests;
