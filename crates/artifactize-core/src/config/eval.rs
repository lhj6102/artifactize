//! An Eval declaration: local identity, instruction, profiles and verdict schemas.

use super::{
    MAX_DECLARED_ITEMS, MutexTag, Profile, ProfileKind, ProfileVariantName, identifier,
    location::Location,
    mutex, schemas,
    validation::{present, text},
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;

/// The required instruction is typed at the declaration and saved-request boundaries;
/// owner-defined context remains extensible JSON with the same object representation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalPayload {
    pub instruction: String,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// An Eval's id within its Artifact, such as `follows-style`: the key of its
/// `[evals.<id>]` table, validated when the declaration is read.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct LocalEvalId(String);

impl<'de> Deserialize<'de> for LocalEvalId {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

impl std::str::FromStr for LocalEvalId {
    type Err = String;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        identifier(value, "Eval id")?;
        Ok(Self(value.to_owned()))
    }
}

impl LocalEvalId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl PartialEq<&str> for LocalEvalId {
    fn eq(&self, other: &&str) -> bool {
        self.0 == *other
    }
}

impl std::ops::Deref for LocalEvalId {
    type Target = str;
    fn deref(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for LocalEvalId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EvalDeclaration {
    pub id: LocalEvalId,
    pub title: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub mutex: Vec<MutexTag>,
    #[serde(flatten)]
    body: EvalBody,
}

/// Every Eval has an instruction and a complete execution profile.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct EvalBody {
    profile: Profile,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    profile_variants: BTreeMap<ProfileVariantName, Profile>,
    payload: EvalPayload,
    /// Each shared with every equal verdict schema in the same read.
    pass_schema: Option<std::sync::Arc<Map<String, Value>>>,
    fail_schema: Option<std::sync::Arc<Map<String, Value>>>,
}

impl EvalDeclaration {
    pub fn profile(&self) -> &Profile {
        &self.body.profile
    }
    pub fn payload(&self) -> Option<&EvalPayload> {
        Some(&self.body.payload)
    }
    pub fn profile_variants(&self) -> &BTreeMap<ProfileVariantName, Profile> {
        &self.body.profile_variants
    }
    pub fn pass_schema(&self) -> Option<&Map<String, Value>> {
        self.body.pass_schema.as_deref()
    }
    pub fn fail_schema(&self) -> Option<&Map<String, Value>> {
        self.body.fail_schema.as_deref()
    }
    #[cfg(test)]
    pub(crate) fn profile_mut(&mut self) -> &mut Profile {
        &mut self.body.profile
    }
    #[cfg(test)]
    pub(crate) fn set_pass_schema(&mut self, schema: Option<Map<String, Value>>) {
        self.body.pass_schema = schema.map(std::sync::Arc::new);
    }
    #[cfg(test)]
    pub(crate) fn set_fail_schema(&mut self, schema: Option<Map<String, Value>>) {
        self.body.fail_schema = schema.map(std::sync::Arc::new);
    }
    pub(crate) fn select_profile(&mut self, name: &ProfileVariantName) -> Result<(), String> {
        self.body.profile = self
            .body
            .profile_variants
            .get(name)
            .ok_or_else(|| format!("Unknown profile variant: {name}"))?
            .clone();
        Ok(())
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
struct EvalFields {
    title: String,
    #[serde(default, deserialize_with = "present")]
    mutex: Option<Vec<MutexTag>>,
    profile: Profile,
    #[serde(default, deserialize_with = "present")]
    profile_variants: Option<BTreeMap<ProfileVariantName, Profile>>,
    #[serde(default, deserialize_with = "present")]
    payload: Option<EvalPayload>,
    #[serde(default, deserialize_with = "present")]
    pass_schema: Option<Map<String, Value>>,
    #[serde(default, deserialize_with = "present")]
    fail_schema: Option<Map<String, Value>>,
}

impl EvalDeclaration {
    /// Check the fields of the `[evals.<id>]` table that every profile needs together.
    fn new(id: LocalEvalId, fields: EvalFields) -> Result<Self, String> {
        if fields.mutex.is_some()
            && !matches!(
                fields.profile.kind(),
                ProfileKind::Runtime | ProfileKind::Agent
            )
        {
            return Err("mutex is supported only on runtime and agent Evals.".into());
        }
        let body = EvalBody {
            profile: fields.profile,
            profile_variants: fields.profile_variants.unwrap_or_default(),
            payload: fields
                .payload
                .ok_or("Eval payload is required for runtime, agent and human profiles.")?,
            pass_schema: fields.pass_schema.map(std::sync::Arc::new),
            fail_schema: fields.fail_schema.map(std::sync::Arc::new),
        };
        Ok(Self {
            id,
            title: fields.title,
            mutex: fields.mutex.unwrap_or_default(),
            body,
        })
    }
}

impl EvalDeclaration {
    pub(super) fn share(&mut self, schemas: &mut schemas::Schemas) {
        for schema in [&mut self.body.pass_schema, &mut self.body.fail_schema]
            .into_iter()
            .flatten()
        {
            *schema = schemas.verdict(schema);
        }
    }

    pub(super) fn validate(
        &self,
        location: &Location<'_, '_>,
        schemas: &mut schemas::Schemas,
    ) -> Result<(), String> {
        location
            .child("title")
            .check(text(&self.title, "Eval title"))?;
        if let Some(payload) = self.payload() {
            location
                .child("payload")
                .child("instruction")
                .check(text(&payload.instruction, "Eval payload.instruction"))?;
        }
        location
            .child("mutex")
            .check(mutex::validate(&self.mutex))?;
        self.profile().validate(&location.child("profile"))?;
        for (key, schema) in [
            ("pass_schema", &self.body.pass_schema),
            ("fail_schema", &self.body.fail_schema),
        ] {
            if let Some(schema) = schema {
                location
                    .child(key)
                    .check(schemas.check(schema, crate::agent::verdict::validate_schema))?;
                // A person enters a Human result in a form: only shapes it can enter.
                if self.profile().kind() == ProfileKind::Human {
                    location
                        .child(key)
                        .check(crate::review::Specification::parse(Some(schema)).map(drop))?;
                }
            }
        }
        if self.profile_variants().len() > MAX_DECLARED_ITEMS {
            return Err(location
                .child("profile_variants")
                .error("profile_variants must contain at most 64 named profiles."));
        }
        for (name, profile) in self.profile_variants() {
            let variant = location.child("profile_variants").child(name);
            location
                .child("profile_variants")
                .key(name)
                .check(identifier(name, "Profile variant name"))?;
            profile.validate(&variant)?;
            if std::mem::discriminant(profile) != std::mem::discriminant(self.profile()) {
                return Err(
                    variant.error("Profile variants must retain the declared reviewer kind.")
                );
            }
        }
        Ok(())
    }
}

/// Eval ids live in table keys, not fields; BTreeMap also fixes their discovery order.
pub(super) fn declared_evals<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<EvalDeclaration>, D::Error> {
    use serde::de::Error;
    BTreeMap::<String, EvalFields>::deserialize(deserializer)?
        .into_iter()
        .map(|(id, fields)| {
            let id = id.parse().map_err(D::Error::custom)?;
            EvalDeclaration::new(id, fields).map_err(D::Error::custom)
        })
        .collect()
}
