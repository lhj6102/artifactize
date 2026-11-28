//! Pure Eval criteria hashes, dependency coverage, and reuse keys.

use super::{PreparedFingerprint, content};
use crate::config::{ArtifactName, Eval, EvalDeclaration, Profile, RepoConfig};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

/// Domain-separated kind-aware key format; changes invalidate persisted cache keys.
const KIND_KEY_FORMAT_PREFIX: &[u8] = b"artifactize-kind-key-v1\n";
/// Composition keys have new coverage semantics. Invalidate old whole results even when
/// their eval already named every part. Stored rows need no migration: old keys miss naturally.
const PARTS_KEY_FORMAT_PREFIX: &[u8] = b"artifactize-parts-key-v1\n";
/// Domain-separate and version the reuse-key hash input, including its terminating newline.
/// Changing these bytes invalidates existing reuse keys; this is not a config/session/DB version.
const REUSE_KEY_FORMAT_PREFIX: &str = "artifactize-key-v3\n";

/// Hash the evaluation criteria: what is asked and how the answer is judged, never how the eval
/// is executed. Execution options (backend, model, reasoning, limits, the named profile), the
/// eval's id and title, a runtime eval's instruction and tool views stay out.
pub fn eval_definition_hash(eval: &EvalDeclaration) -> crate::types::DefinitionHash {
    let mut criteria = match &eval.profile() {
        Profile::Agent { .. } => json!({"kind": "agent", "payload": eval.payload()}),
        Profile::Human {} => json!({"kind": "human", "payload": eval.payload()}),
        // A runtime eval's verdict is its command's exit code; its prose instruction is not.
        Profile::Runtime { command, args, .. } => json!({
            "kind": "runtime",
            "command": command,
            "args": args,
            "inputs": eval.inputs(),
        }),
    };
    // Only an Agent eval declares a bar, so other evals keep their criteria hashes.
    if let Some(red_at) = eval.red_at() {
        criteria["redAt"] = json!(red_at);
    }
    criteria.sort_all_objects();
    content::hex(&Sha256::digest(
        serde_json::to_vec(&criteria).expect("evaluation criteria is JSON"),
    ))
    .parse()
    .expect("a SHA-256 digest in hex is a definition hash")
}

/// An eval's reuse key: `hash(evaluation criteria, sorted (name, fingerprint) of the Artifacts it
/// depends on)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Key {
    pub value: crate::types::ReuseKey,
    pub eval_def_hash: crate::types::DefinitionHash,
    /// Each Artifact the eval depends on, its target included, with its fingerprint.
    pub fingerprints: BTreeMap<crate::types::ArtifactName, crate::types::Fingerprint>,
    pub artifact_kinds: BTreeMap<crate::types::ArtifactName, crate::config::ArtifactKind>,
}

/// Why an eval has no reuse key: an Artifact it depends on declares `fingerprint: false`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unkeyed {
    /// The eval's own target.
    Target,
    /// A child or referenced Artifact.
    Dependency(ArtifactName),
    /// Runtime observations of external state always execute.
    LiveObservation(ArtifactName),
    /// A Live input has no observed-state snapshot.
    LiveUnobserved(ArtifactName),
}

/// Key inputs: the target, every part it is made of, and the Artifacts the eval names.
/// Composition is recursive; references stay one hop and grant no additional tool access.
pub fn dependencies<'a>(config: &'a RepoConfig, eval: &'a Eval) -> BTreeSet<&'a ArtifactName> {
    std::iter::once(&eval.target)
        .chain(config.descendants(&eval.target))
        .chain(&eval.deps)
        .collect()
}

/// Restrict fingerprint preparation to the required Eval key inputs.
pub(crate) fn fingerprint_targets<'a>(
    config: &'a RepoConfig,
    required: &BTreeSet<&'a ArtifactName>,
) -> BTreeSet<&'a ArtifactName> {
    config
        .evals
        .iter()
        .filter(|eval| required.contains(&eval.target))
        .flat_map(|eval| dependencies(config, eval))
        .collect()
}

/// A Live input is an observation boundary, not a file. Composition counts as coverage too.
pub(crate) fn live_input<'a>(config: &'a RepoConfig, eval: &'a Eval) -> Option<&'a ArtifactName> {
    dependencies(config, eval)
        .into_iter()
        .find(|name| config.artifacts[*name].kind() == crate::config::ArtifactKind::Live)
}

/// The reuse key of an Eval definition hash over these fingerprints.
pub fn key(
    eval_def_hash: &crate::types::DefinitionHash,
    fingerprints: &BTreeMap<crate::types::ArtifactName, crate::types::Fingerprint>,
) -> crate::types::ReuseKey {
    let mut digest = Sha256::new();
    digest.update(format!("{REUSE_KEY_FORMAT_PREFIX}eval {eval_def_hash}\n"));
    // Names are identifiers and fingerprints never contain spaces or line breaks.
    for (name, fingerprint) in fingerprints {
        digest.update(format!("artifact {name} {fingerprint}\n"));
    }
    content::hex(&digest.finalize())
        .parse()
        .expect("SHA-256 is a reuse key")
}

/// Scope kinds distinguish file/folder observation without coupling reuse to repo paths.
pub fn key_with_kinds(
    eval_def_hash: &crate::types::DefinitionHash,
    fingerprints: &BTreeMap<crate::types::ArtifactName, crate::types::Fingerprint>,
    kinds: &BTreeMap<crate::types::ArtifactName, crate::config::ArtifactKind>,
) -> crate::types::ReuseKey {
    if kinds.is_empty() {
        return key(eval_def_hash, fingerprints);
    }
    let mut digest = Sha256::new();
    digest.update(KIND_KEY_FORMAT_PREFIX);
    digest.update(key(eval_def_hash, fingerprints).as_bytes());
    digest.update(serde_json::to_vec(kinds).expect("Artifact kinds are JSON"));
    content::hex(&digest.finalize())
        .parse()
        .expect("SHA-256 is a reuse key")
}

/// Versioned composition format over the complete, kind-aware input map. The map already
/// contains each part once; no content is rehashed and no persistence schema changes.
pub(crate) fn parts_key(value: &crate::types::ReuseKey) -> crate::types::ReuseKey {
    let mut digest = Sha256::new();
    digest.update(PARTS_KEY_FORMAT_PREFIX);
    digest.update(value.as_bytes());
    content::hex(&digest.finalize())
        .parse()
        .expect("SHA-256 is a reuse key")
}

/// The eval's reuse key from prepared fingerprints. Without a fingerprint on every Artifact
/// it depends on, the eval has no key, so it is never reused or published.
pub fn eval_key(
    config: &RepoConfig,
    eval: &Eval,
    fingerprints: &BTreeMap<&ArtifactName, PreparedFingerprint>,
) -> Result<Key, Unkeyed> {
    if matches!(eval.declaration.profile(), Profile::Runtime { .. })
        && let Some(id) = live_input(config, eval)
    {
        return Err(Unkeyed::LiveObservation(id.clone()));
    }
    if !fingerprints.contains_key(&eval.target) {
        return Err(
            if config.artifacts[&eval.target].kind() == crate::config::ArtifactKind::Live {
                Unkeyed::LiveUnobserved(eval.target.clone())
            } else {
                Unkeyed::Target
            },
        );
    }
    let values = dependencies(config, eval)
        .into_iter()
        .map(|id| {
            let fingerprint = fingerprints.get(id).ok_or_else(|| {
                if config.artifacts[id].kind() == crate::config::ArtifactKind::Live {
                    Unkeyed::LiveUnobserved(id.clone())
                } else {
                    Unkeyed::Dependency(id.clone())
                }
            })?;
            Ok((id.clone(), fingerprint.value.clone()))
        })
        .collect::<Result<BTreeMap<_, _>, Unkeyed>>()?;
    let eval_def_hash = eval_definition_hash(&eval.declaration);
    let artifact_kinds = values
        .keys()
        .map(|id| (id.clone(), config.artifacts[id].kind()))
        .collect();
    let value = key_with_kinds(&eval_def_hash, &values, &artifact_kinds);
    let value = if config.artifacts[&eval.target].parts.is_empty() {
        value
    } else {
        parts_key(&value)
    };
    Ok(Key {
        value,
        eval_def_hash,
        fingerprints: values,
        artifact_kinds,
    })
}

/// Keys of every eval whose dependency Artifacts were all prepared, by eval id.
pub fn eval_keys<'a>(
    config: &'a RepoConfig,
    fingerprints: &BTreeMap<&ArtifactName, PreparedFingerprint>,
) -> BTreeMap<&'a crate::types::EvalId, Key> {
    config
        .evals
        .iter()
        .filter_map(|eval| Some((&eval.id, eval_key(config, eval, fingerprints).ok()?)))
        .collect()
}
