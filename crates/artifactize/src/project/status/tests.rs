//! Independent old status output literals pin every actual state/action branch.
use super::*;
use crate::graph::{ArtifactStatus, EvalStatus};
use serde_json::json;

#[test]
fn all_artifact_and_eval_projections_keep_json_and_text_api_literals() {
    for (status, expected) in [
        (ArtifactStatus::Basis, "BASIS"),
        (ArtifactStatus::Green, "PASS"),
        (ArtifactStatus::Incomplete, "INCOMPLETE"),
        (ArtifactStatus::Error, "ERROR"),
        (ArtifactStatus::Red, "RED"),
        (ArtifactStatus::Blocked, "BLOCKED"),
        (ArtifactStatus::Wait, "WAIT_DEPENDENCY"),
        (ArtifactStatus::Stale, "STALE"),
        (ArtifactStatus::Unreviewed, "UNREVIEWED"),
    ] {
        let state = ArtifactCondition::from(status);
        assert_eq!(state.to_string(), expected);
        assert_eq!(serde_json::to_value(state).unwrap(), json!(expected));
    }
    for (status, expected) in [
        (EvalStatus::Green, "PASS"),
        (EvalStatus::Red, "RED"),
        (EvalStatus::Error, "ERROR"),
        (EvalStatus::Stale, "STALE"),
        (EvalStatus::Unreviewed, "UNREVIEWED"),
        (EvalStatus::Wait, "WAIT_DEPENDENCY"),
        (EvalStatus::Blocked, "BLOCKED"),
    ] {
        let state = EvalCondition::from(status);
        assert_eq!(state.to_string(), expected);
        assert_eq!(serde_json::to_value(state).unwrap(), json!(expected));
    }
    assert_eq!(EvalCondition::WaitingHuman.to_string(), "WAITING_HUMAN");
    assert_eq!(
        serde_json::to_value(EvalCondition::WaitingHuman).unwrap(),
        json!("WAITING_HUMAN")
    );
}

#[test]
fn actions_count_exhaustively_and_count_object_order_matches_old_string_keys() {
    let mut counts = Counts::default();
    for (action, expected) in [
        (VerifyAction::Execute, "execute"),
        (VerifyAction::Derive, "derive"),
        (VerifyAction::Reuse, "reuse"),
        (VerifyAction::Wait, "wait"),
        (VerifyAction::Blocked, "blocked"),
    ] {
        assert_eq!(action.to_string(), expected);
        assert_eq!(serde_json::to_value(action).unwrap(), json!(expected));
        counts.action(action);
    }
    counts.artifacts = BTreeMap::from([
        (ArtifactCondition::WaitDependency, 1),
        (ArtifactCondition::Unreviewed, 1),
        (ArtifactCondition::Stale, 1),
        (ArtifactCondition::Red, 1),
        (ArtifactCondition::Pass, 1),
        (ArtifactCondition::Incomplete, 1),
        (ArtifactCondition::Error, 1),
        (ArtifactCondition::Blocked, 1),
        (ArtifactCondition::Basis, 1),
    ]);
    counts.evals = BTreeMap::from([
        (EvalCondition::WaitingHuman, 1),
        (EvalCondition::WaitDependency, 1),
        (EvalCondition::Unreviewed, 1),
        (EvalCondition::Stale, 1),
        (EvalCondition::Red, 1),
        (EvalCondition::Pass, 1),
        (EvalCondition::Error, 1),
        (EvalCondition::Blocked, 1),
    ]);
    let expected = r#"{"artifacts":{"BASIS":1,"BLOCKED":1,"ERROR":1,"INCOMPLETE":1,"PASS":1,"RED":1,"STALE":1,"UNREVIEWED":1,"WAIT_DEPENDENCY":1},"evals":{"BLOCKED":1,"ERROR":1,"PASS":1,"RED":1,"STALE":1,"UNREVIEWED":1,"WAIT_DEPENDENCY":1,"WAITING_HUMAN":1},"execute":1,"derive":1,"reuse":1,"wait":1,"blocked":1}"#;
    assert_eq!(serde_json::to_string(&counts).unwrap(), expected);
}
