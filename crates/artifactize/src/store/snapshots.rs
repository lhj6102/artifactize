//! Saved review and validation snapshots, parsed once at the store boundary.
//! Missing and null saved fields stay distinct; owner extension fields remain JSON.

use crate::{
    config::Field,
    types::{ArtifactName, EvalId, Fingerprint, RequestStatus},
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::{collections::BTreeMap, path::PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChildIdentity {
    pub pid: crate::process::ProcessId,
    pub start_time: u64,
}
impl From<crate::process::ChildIdentity> for ChildIdentity {
    fn from(child: crate::process::ChildIdentity) -> Self {
        Self {
            pid: child.pid,
            start_time: child.start_time,
        }
    }
}

/// A dependency blocker is either an Artifact or a workspace-qualified Eval.
/// Its saved representation remains the original untagged string.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub enum Blocker {
    Artifact(ArtifactName),
    Eval(EvalId),
}
impl TryFrom<String> for Blocker {
    type Error = String;
    fn try_from(value: String) -> Result<Self, String> {
        if value.contains('/') {
            value.parse().map(Self::Eval)
        } else {
            value.parse().map(Self::Artifact)
        }
    }
}
impl From<Blocker> for String {
    fn from(value: Blocker) -> String {
        value.to_string()
    }
}
impl std::fmt::Display for Blocker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Artifact(id) => id.fmt(f),
            Self::Eval(id) => id.fmt(f),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HumanDefinition {
    #[serde(
        default,
        skip_serializing_if = "Field::missing",
        with = "super::definitions::path_field"
    )]
    pub repo: Field<PathBuf>,
    pub eval: HumanEval,
    #[serde(default, skip_serializing_if = "Field::missing")]
    pub artifacts: Field<BTreeMap<ArtifactName, super::definitions::Artifact>>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HumanEval {
    #[serde(default, skip_serializing_if = "Field::missing")]
    pub id: Field<EvalId>,
    #[serde(default, skip_serializing_if = "Field::missing")]
    pub target: Field<ArtifactName>,
    #[serde(default, skip_serializing_if = "Field::missing")]
    pub references: Field<BTreeMap<String, ArtifactName>>,
    #[serde(default, skip_serializing_if = "Field::missing")]
    pub deps: Field<Vec<ArtifactName>>,
    #[serde(default, skip_serializing_if = "Field::missing")]
    pub declaration: Field<super::definitions::Declaration>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}
impl PartialEq for HumanDefinition {
    fn eq(&self, other: &Self) -> bool {
        // Owner JSON schemas are structural values; compare their preserved saved forms.
        serde_json::to_value(self).expect("Human definition is JSON")
            == serde_json::to_value(other).expect("Human definition is JSON")
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Validation(pub Option<ValidationSnapshot>);
impl Validation {
    pub fn snapshot(&self) -> Option<&ValidationSnapshot> {
        self.0.as_ref()
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidationSnapshot {
    #[serde(default, skip_serializing_if = "Field::missing")]
    pub selection: Field<super::definitions::SavedSelection>,
    #[serde(default, skip_serializing_if = "Field::missing")]
    pub recursive: Field<bool>,
    #[serde(default, skip_serializing_if = "Field::missing")]
    pub force: Field<bool>,
    #[serde(default, skip_serializing_if = "Field::missing")]
    pub ignore_gates: Field<bool>,
    #[serde(default, skip_serializing_if = "Field::missing")]
    pub selected_eval_ids: Field<Vec<EvalId>>,
    #[serde(default, skip_serializing_if = "Field::missing")]
    pub included_eval_ids: Field<Vec<EvalId>>,
    #[serde(default, skip_serializing_if = "Field::missing")]
    pub satisfied: Field<bool>,
    #[serde(default, skip_serializing_if = "Field::missing")]
    pub obligations: Field<Vec<ArtifactName>>,
    #[serde(default, skip_serializing_if = "Field::missing")]
    pub artifacts: Field<Vec<ArtifactValidation>>,
    #[serde(default, skip_serializing_if = "Field::missing")]
    pub evals: Field<Vec<EvalValidation>>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactValidation {
    pub id: ArtifactName,
    #[serde(default, skip_serializing_if = "Field::missing")]
    pub status: Field<crate::types::ArtifactValidationStatus>,
    #[serde(default, skip_serializing_if = "Field::missing")]
    pub passed: Field<u64>,
    #[serde(default, skip_serializing_if = "Field::missing")]
    pub total: Field<u64>,
    #[serde(default, skip_serializing_if = "Field::missing")]
    pub satisfied: Field<bool>,
    #[serde(
        default,
        rename = "fingerprintKind",
        skip_serializing_if = "Field::missing"
    )]
    pub fingerprint_kind: Field<crate::types::FingerprintKind>,
    #[serde(default, skip_serializing_if = "Field::missing")]
    pub fingerprint: Field<Fingerprint>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EvalValidation {
    pub id: EvalId,
    #[serde(default, skip_serializing_if = "Field::missing")]
    pub status: Field<RequestStatus>,
    #[serde(default, skip_serializing_if = "Field::missing")]
    pub blocked_by: Field<Vec<Blocker>>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn human_snapshot_preserves_partial_legacy_and_owner_extension_fields() {
        for value in [
            json!({"eval":{"declaration":{"failSchema":{"type":"object"}}}}),
            json!({"repo":"saved/workspace","eval":{"id":"app/review","target":"app","references":{"spec":"spec"},"deps":["spec"],"declaration":{"payload":{"instruction":"Review.","owner":{"v":1}},"passSchema":{"required":["approved"]},"owner":[1,2]}},"artifacts":{"app":{"path":"app","views":{"humanTools":{}},"extension":true}},"extension":{"v":2}}),
        ] {
            let saved: HumanDefinition = serde_json::from_value(value.clone()).unwrap();
            assert_eq!(serde_json::to_value(saved).unwrap(), value);
        }
        assert!(
            serde_json::from_value::<HumanDefinition>(json!({"eval":{"id":"not-qualified"}}))
                .is_err()
        );
        let child: ChildIdentity = serde_json::from_value(json!({"pid":7,"startTime":9})).unwrap();
        assert_eq!(
            serde_json::to_value(child).unwrap(),
            json!({"pid":7,"startTime":9})
        );
    }
    #[test]
    fn validation_preserves_legacy_null_missing_and_extension_fields() {
        for value in [
            Value::Null,
            json!({}),
            json!({"satisfied":null,"extension":{"owner":true}}),
            json!({"satisfied":false,"artifacts":[{"id":"app","status":"STALE","fingerprintKind":"script","fingerprint":"v1"}],"evals":[{"id":"app/test","status":"GREEN","blockedBy":["dep","dep/test"]}]}),
        ] {
            let typed: Validation = serde_json::from_value(value.clone()).unwrap();
            assert_eq!(serde_json::to_value(typed).unwrap(), value);
        }
        assert!(serde_json::from_value::<Validation>(json!({"evals":[{"id":"bad"}]})).is_err());
        assert!(serde_json::from_value::<Blocker>(json!("../escape")).is_err());
        assert_eq!(
            serde_json::to_value(Blocker::try_from("app/test".to_owned()).unwrap()).unwrap(),
            json!("app/test")
        );
    }
}
