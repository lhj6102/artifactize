//! Project-level settings stored in `.artifactize/settings.toml`, such as reusable evaluator
//! profile aliases referenced via `!alias` or `!alias(...)` in `.artf` files.

use std::{collections::BTreeMap, ffi::OsStr, path::Path};

use super::{
    ConfigError, MAX_DECLARED_ITEMS, Profile, ProfileAliasName,
    declaration::reject_non_json_values,
    discovery::{DEFINITION_DIR, read_regular_text},
    identifier,
    location::Location,
    validation::text,
};

pub const SETTINGS_FILE: &str = "settings.toml";

/// Project settings declared in `.artifactize/settings.toml`.
#[derive(Debug, Clone, Default)]
pub struct Settings {
    pub profiles: BTreeMap<ProfileAliasName, Profile>,
}

/// Validate a `.artifactize/settings.toml` document from source text.
pub fn parse_settings(source: &str) -> Result<Settings, String> {
    let table = toml::de::DeTable::parse(source).map_err(|mut error| {
        error.set_input(Some(source));
        error.to_string()
    })?;
    let document = toml::Spanned::new(table.span(), toml::de::DeValue::Table(table.into_inner()));
    let location = Location::root(source, &document);
    for (key, value) in document.get_ref().as_table().unwrap() {
        reject_non_json_values(value, key.get_ref().as_ref(), source, |offset| offset)?;
    }
    let mut profiles = BTreeMap::new();
    for (key, _) in location.table()? {
        let field = key.get_ref().as_ref();
        match field {
            "profiles" => {
                profiles = parse_profiles(&location.child("profiles"))?;
            }
            other => {
                return Err(location
                    .key(other)
                    .error(format!("unknown field `{other}`, expected `profiles`")));
            }
        }
    }
    Ok(Settings { profiles })
}

fn parse_profiles(
    location: &Location<'_, '_>,
) -> Result<BTreeMap<ProfileAliasName, Profile>, String> {
    let table = location.table()?;
    if table.len() > MAX_DECLARED_ITEMS {
        return Err(location.error("profiles must contain at most 64 named profile aliases."));
    }
    let mut profiles = BTreeMap::new();
    for (key, value) in table {
        let name = key.get_ref().as_ref();
        location
            .key(name)
            .check(identifier(name, "Profile alias name"))?;
        let alias_name: ProfileAliasName = location.key(name).check(name.parse())?;
        let alias_location = location.child(name);
        if let Some(kind) = value.get_ref().get("kind")
            && let toml::de::DeValue::String(kind) = kind.get_ref()
            && kind.as_ref() != "agent"
        {
            return Err(alias_location
                .child("kind")
                .error("Profile aliases support only agent profiles (kind = \"agent\")."));
        }
        if let Some(model) = value.get_ref().get("model")
            && let toml::de::DeValue::String(model) = model.get_ref()
        {
            alias_location
                .child("model")
                .check(text(model, "Agent profile model"))?;
        }
        let profile: Profile = alias_location.deserialize()?;
        if !matches!(profile, Profile::Agent { .. }) {
            return Err(alias_location
                .child("kind")
                .error("Profile aliases support only agent profiles (kind = \"agent\")."));
        }
        profile.validate(&alias_location)?;
        profiles.insert(alias_name, profile);
    }
    Ok(profiles)
}

/// Read `.artifactize/settings.toml` if present under `root`.
///
/// Exact filename matching via directory enumeration keeps behavior identical across
/// case-sensitive and case-insensitive filesystems, and rejects symlinks or directories.
pub(super) fn read(root: &Path) -> Result<Settings, ConfigError> {
    let definitions = root.join(DEFINITION_DIR);
    if !crate::platform::paths::reusable_directory(root, DEFINITION_DIR)
        .map_err(|error| ConfigError::new(&definitions, error))?
    {
        return Ok(Settings::default());
    }
    let opened = crate::platform::open_directory(&definitions)
        .map_err(|error| ConfigError::new(&definitions, error))?;
    let entries = crate::platform::read_dir(&opened)
        .map_err(|error| ConfigError::new(&definitions, error))?;
    for entry in entries {
        let entry = entry.map_err(|error| ConfigError::new(&definitions, error))?;
        if entry.file_name() != OsStr::new(SETTINGS_FILE) {
            continue;
        }
        let path = definitions.join(SETTINGS_FILE);
        let kind = entry
            .file_type()
            .map_err(|error| ConfigError::new(&path, error))?;
        if kind != crate::platform::FileKind::File {
            return Err(ConfigError::new(
                &path,
                "line 1, column 1: Settings file must be a regular file, not a link, directory or special file.",
            ));
        }
        let source = read_regular_text(&path).map_err(|error| ConfigError::new(&path, error))?;
        return parse_settings(&source).map_err(|error| ConfigError::new(&path, error));
    }
    Ok(Settings::default())
}

#[cfg(test)]
mod tests;
