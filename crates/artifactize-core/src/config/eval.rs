//! An Eval declaration: local identity, instruction, profiles, the priority at which an Agent
//! eval turns RED, and a Human eval's result schemas.

use super::{
    MAX_DECLARED_ITEMS, MutexTag, Profile, ProfileKind, ProfileVariantName,
    declaration::reject_non_json_values,
    identifier,
    location::{self, Location},
    mutex,
    profile::AgentProfileOverrides,
    schemas, settings,
    validation::{present, text},
};
use artifactize_tools::artf::{AliasInvocation, Masked};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;

/// Synthetic TOML prefix used to parse `!alias(<overrides>)` contents as an inline table.
const OVERRIDE_WRAPPER_PREFIX: &str = "_ = {";

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

/// A finding's priority: P0 is the most important and P3 the least. artifactize defines only
/// their order; the evaluator assigns them by its own judgement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Priority {
    P0,
    P1,
    P2,
    P3,
}

impl Priority {
    pub const ALL: [Self; 4] = [Self::P0, Self::P1, Self::P2, Self::P3];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::P0 => "P0",
            Self::P1 => "P1",
            Self::P2 => "P2",
            Self::P3 => "P3",
        }
    }
}

impl std::fmt::Display for Priority {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
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
    /// An Agent eval's bar: a finding at this priority or a more important one makes it RED.
    /// Without it, any finding does.
    #[serde(skip_serializing_if = "Option::is_none")]
    red_at: Option<Priority>,
    /// A Human eval's result schemas, each shared with every equal schema in the same read.
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
    pub fn red_at(&self) -> Option<Priority> {
        self.body.red_at
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
    red_at: Option<Priority>,
    #[serde(default, deserialize_with = "present")]
    pass_schema: Option<Map<String, Value>>,
    #[serde(default, deserialize_with = "present")]
    fail_schema: Option<Map<String, Value>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
struct EvalNonProfileFields {
    title: String,
    #[serde(default, deserialize_with = "present")]
    mutex: Option<Vec<MutexTag>>,
    #[serde(default, deserialize_with = "present")]
    payload: Option<EvalPayload>,
    #[serde(default, deserialize_with = "present")]
    red_at: Option<Priority>,
    #[serde(default, deserialize_with = "present")]
    pass_schema: Option<Map<String, Value>>,
    #[serde(default, deserialize_with = "present")]
    fail_schema: Option<Map<String, Value>>,
}

impl EvalDeclaration {
    /// Parse `[evals]` directly from spanned TOML so `!alias` and `!alias(...)` profiles resolve
    /// with exact source positions.
    pub(super) fn parse_map(
        location: &Location<'_, '_>,
        source: &str,
        masked: &Masked,
        settings: &settings::Settings,
        consumed: &mut [bool],
    ) -> Result<Vec<Self>, String> {
        let mut parsed = BTreeMap::new();
        for (id_key, _) in location.table()? {
            let id_str = id_key.get_ref().as_ref();
            let eval_location = location.key(id_str);
            let id: LocalEvalId = eval_location.check(id_str.parse())?;
            let eval_table = eval_location.table()?;
            if eval_table.get("profile").is_none() {
                return Err(eval_location.error("missing field `profile`"));
            }
            let profile = parse_profile(
                &eval_location.child("profile"),
                source,
                masked,
                settings,
                consumed,
            )?;
            let profile_variants = if eval_table.contains_key("profile_variants") {
                let variants_location = eval_location.child("profile_variants");
                let variants_table = variants_location.table()?;
                if variants_table.len() > MAX_DECLARED_ITEMS {
                    return Err(variants_location
                        .error("profile_variants must contain at most 64 named profiles."));
                }
                let mut variants = BTreeMap::new();
                for (variant_key, _) in variants_table {
                    let name = variant_key.get_ref().as_ref();
                    let name_location = variants_location.key(name);
                    let variant_name: ProfileVariantName = name_location.check(name.parse())?;
                    let variant_profile = parse_profile(
                        &variants_location.child(name),
                        source,
                        masked,
                        settings,
                        consumed,
                    )?;
                    variants.insert(variant_name, variant_profile);
                }
                Some(variants)
            } else {
                None
            };
            let mut rest_table = eval_table.clone();
            rest_table.remove("profile");
            rest_table.remove("profile_variants");
            let rest_value = toml::Spanned::new(
                eval_location.value().span(),
                toml::de::DeValue::Table(rest_table),
            );
            let rest =
                EvalNonProfileFields::deserialize(toml::de::ValueDeserializer::from(rest_value))
                    .map_err(|mut error| {
                        if !masked.invocations().is_empty() {
                            let offset = error.span().map_or(
                                eval_location.original_offset(eval_location.value().span().start),
                                |span| masked.original_offset(span.start),
                            );
                            return location::positioned(
                                source,
                                offset,
                                eval_location.path(),
                                error.message(),
                            );
                        }
                        error.set_input(Some(source));
                        eval_location.error(error)
                    })?;
            let fields = EvalFields {
                title: rest.title,
                mutex: rest.mutex,
                profile,
                profile_variants,
                payload: rest.payload,
                red_at: rest.red_at,
                pass_schema: rest.pass_schema,
                fail_schema: rest.fail_schema,
            };
            let declaration = eval_location.check(Self::new(id.clone(), fields))?;
            parsed.insert(id, declaration);
        }
        Ok(parsed.into_values().collect())
    }

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
        let agent = fields.profile.kind() == ProfileKind::Agent;
        if agent && (fields.pass_schema.is_some() || fields.fail_schema.is_some()) {
            return Err("An Agent eval reports prioritized findings; remove pass_schema and fail_schema, and set red_at for the priority that makes it RED.".into());
        }
        if !agent && fields.red_at.is_some() {
            return Err("red_at is supported only on agent Evals.".into());
        }
        let body = EvalBody {
            profile: fields.profile,
            profile_variants: fields.profile_variants.unwrap_or_default(),
            payload: fields
                .payload
                .ok_or("Eval payload is required for runtime, agent and human profiles.")?,
            red_at: fields.red_at,
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

fn parse_profile(
    location: &Location<'_, '_>,
    source: &str,
    masked: &Masked,
    settings: &settings::Settings,
    consumed: &mut [bool],
) -> Result<Profile, String> {
    if let toml::de::DeValue::String(text_value) = location.value().get_ref()
        && let Some(index) = masked.invocation_index(text_value)
    {
        consumed[index] = true;
        return resolve_alias(location, source, &masked.invocations()[index], settings);
    }
    if let Some(model) = location.value().get_ref().get("model")
        && let toml::de::DeValue::String(model) = model.get_ref()
    {
        location
            .child("model")
            .check(text(model, "Agent profile model"))?;
    }
    location.deserialize()
}

fn resolve_alias(
    location: &Location<'_, '_>,
    source: &str,
    invocation: &AliasInvocation,
    settings: &settings::Settings,
) -> Result<Profile, String> {
    let path = location.path();
    identifier(&invocation.name, "Profile alias name").map_err(|message| {
        location::positioned(source, invocation.name_span.start, path, message)
    })?;
    let base = settings
        .profiles
        .get(invocation.name.as_str())
        .ok_or_else(|| {
            location::positioned(
                source,
                invocation.name_span.start,
                path,
                format!(
                    "Unknown profile alias `{}`. Define `[profiles.{}]` in .artifactize/settings.toml.",
                    invocation.name, invocation.name
                ),
            )
        })?;
    let Some(span) = &invocation.overrides_span else {
        return Ok(base.clone());
    };
    let map_offset =
        |sub: usize| (span.start + sub.saturating_sub(OVERRIDE_WRAPPER_PREFIX.len())).min(span.end);
    let wrapped = format!("{OVERRIDE_WRAPPER_PREFIX}{}}}", &source[span.clone()]);
    let parsed = toml::de::DeTable::parse(&wrapped).map_err(|error| {
        let offset = error.span().map_or(span.start, |s| map_offset(s.start));
        location::positioned(source, offset, path, error.message())
    })?;
    let inner_spanned = parsed
        .get_ref()
        .get("_")
        .filter(|value| parsed.get_ref().len() == 1 && value.span().end == wrapped.len())
        .ok_or_else(|| {
            location::positioned(source, span.start, path, "Invalid profile alias override.")
        })?;
    let inner_table = inner_spanned.get_ref().as_table().ok_or_else(|| {
        location::positioned(source, span.start, path, "Invalid profile alias override.")
    })?;
    for (key, value) in inner_table {
        reject_non_json_values(
            value,
            &format!("{path}.{}", key.get_ref()),
            source,
            map_offset,
        )?;
    }
    if let Some((kind_key, _)) = inner_table.get_key_value("kind") {
        return Err(location::positioned(
            source,
            map_offset(kind_key.span().start),
            &format!("{path}.kind"),
            "Profile alias overrides cannot set `kind`.",
        ));
    }
    if let Some(model) = inner_table.get("model")
        && let toml::de::DeValue::String(model_text) = model.get_ref()
    {
        text(model_text, "Agent profile model").map_err(|message| {
            location::positioned(
                source,
                map_offset(model.span().start),
                &format!("{path}.model"),
                message,
            )
        })?;
    }
    let overrides = AgentProfileOverrides::deserialize(toml::de::ValueDeserializer::from(
        inner_spanned.clone(),
    ))
    .map_err(|error| {
        let offset = error.span().map_or(span.start, |s| map_offset(s.start));
        location::positioned(source, offset, path, error.message())
    })?;
    let resolved = base
        .apply_overrides(overrides)
        .map_err(|message| location::positioned(source, invocation.span.start, path, message))?;
    if let Profile::Agent {
        backend, reasoning, ..
    } = &resolved
    {
        if let Some(provider) = backend.builtin() {
            provider.validate_eval().map_err(|message| {
                let offset = inner_table
                    .get("backend")
                    .map_or(invocation.span.start, |value| {
                        map_offset(value.span().start)
                    });
                location::positioned(source, offset, &format!("{path}.backend"), message)
            })?;
        }
        if let (Some(reasoning), Some(provider)) = (reasoning, backend.builtin()) {
            crate::llm::Api::from(provider)
                .validate_reasoning(reasoning.as_str())
                .map_err(|message| {
                    let (offset, field) = if let Some(value) = inner_table.get("reasoning") {
                        (map_offset(value.span().start), "reasoning")
                    } else if let Some(value) = inner_table.get("backend") {
                        (map_offset(value.span().start), "backend")
                    } else {
                        (invocation.span.start, "reasoning")
                    };
                    location::positioned(source, offset, &format!("{path}.{field}"), message)
                })?;
        }
    }
    Ok(resolved)
}
