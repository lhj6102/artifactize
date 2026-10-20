use super::*;
use serde_json::{Value, json};

const DEFINED: &str = r#"{"artifacts":{"app":{"path":"app"}},"evals":[{"id":"app/check","target":"app","deps":[],"declaration":{"title":"Check"}}]}"#;
/// Definitions that do not decode: Artifacts that are not a map.
const BROKEN: &str = r#"{"artifacts":7,"evals":[]}"#;

/// A Run record saved with `definitions` (JSON text) and `jobs` executions.
fn record(definitions: &str, jobs: u32) -> String {
    let mut head = json!({
        "id":"run-1","repoPath":"/repo","stateDir":"/state","status":"GREEN",
        "createdAt":"2026-01-01T00:00:00Z","completedAt":"2026-01-01T00:00:01Z",
        "selection":{"kind":"all"},"validation":null,"evidence":{},"jobs":jobs
    })
    .to_string();
    head.pop();
    format!(r#"{head},"definitions":{definitions}}}"#)
}

#[test]
fn split_takes_the_definitions_out_by_their_text() {
    let data = record(DEFINED, 1);
    let (rest, definitions) = split(&data).unwrap();
    let (text, digest) = definitions.unwrap();
    assert_eq!(text, DEFINED);
    assert_eq!(digest, Sha256Bytes::of(DEFINED.as_bytes()));
    let rest: Value = serde_json::from_str(&rest).unwrap();
    assert!(rest.get(DEFINITIONS).is_none());
    assert_eq!(rest["jobs"], 1);
}

#[test]
fn definitions_named_twice_stay_in_the_rest_and_read_as_the_whole_record_would() {
    let data = record(DEFINED, 1).replacen('{', &format!(r#"{{"definitions":{DEFINED},"#), 1);
    let (rest, definitions) = split(&data).unwrap();
    assert!(definitions.is_none());
    assert_eq!(rest.matches("\"definitions\"").count(), 2);
    let whole = decode::<Run>("run", "run-1", &data)
        .map(|_| ())
        .map_err(|error| error.reason);
    let parts = Seen::default()
        .read("run-1", &data)
        .map(|_| ())
        .map_err(|error| error.reason);
    assert_eq!(parts, whole);
}

#[test]
fn read_shares_definitions_it_has_seen_and_decodes_new_ones() {
    let mut seen = Seen::default();
    let first = seen.read("run-1", &record(DEFINED, 1)).unwrap();
    let again = seen.read("run-1", &record(DEFINED, 2)).unwrap();
    assert_eq!(again.jobs, Some(2));
    assert!(again.definitions.shared(&first.definitions));
    let other = DEFINED.replace("Check", "Other");
    let changed = seen.read("run-1", &record(&other, 2)).unwrap();
    assert!(!changed.definitions.shared(&first.definitions));
    let title = &changed.definitions.graph().unwrap().evals.value().unwrap()[0];
    assert_eq!(
        serde_json::to_value(title).unwrap()["declaration"]["title"],
        "Other"
    );
}

#[test]
fn check_remembers_each_definitions_verdict_and_facts_never_decodes_them() {
    let mut seen = Seen::default();
    let broken = record(BROKEN, 1);
    let reason = seen.check("run-1", &broken).unwrap_err().reason;
    assert_eq!(seen.verdicts.len(), 1);
    // The same definitions saved again are judged by their digest, with the same reason.
    assert_eq!(
        seen.check("run-1", &record(BROKEN, 2)).unwrap_err().reason,
        reason
    );
    assert_eq!(seen.verdicts.len(), 1);
    assert_eq!(seen.read("run-1", &broken).unwrap_err().reason, reason);
    seen.check("run-1", &record(DEFINED, 1)).unwrap();
    assert_eq!(seen.verdicts.len(), 2);
    // A Run's facts read without its definitions, broken or not.
    let (run, digest) = seen.facts("run-1", &broken).unwrap();
    assert_eq!(run.jobs, Some(1));
    assert_eq!(digest, Some(Sha256Bytes::of(BROKEN.as_bytes())));
    // Verdicts of definitions no Run holds any more are forgotten.
    seen.retain(&BTreeSet::from([Sha256Bytes::of(DEFINED.as_bytes())]));
    assert_eq!(seen.verdicts.len(), 1);
}

#[test]
fn a_record_that_is_not_an_object_is_unreadable() {
    let mut seen = Seen::default();
    for data in ["[]", "{", "null"] {
        assert_eq!(seen.read("run-1", data).unwrap_err().kind, "run");
        assert!(seen.check("run-1", data).is_err());
        assert!(seen.facts("run-1", data).is_err());
    }
}
