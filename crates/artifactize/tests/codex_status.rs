//! Independent wire fixtures for offline sign-in states; no credential files are opened.

use artifactize::auth::codex::{FileExpiry, Status, StoredExpiry};
use serde_json::Value;

#[test]
fn enum_states_preserve_the_existing_status_json_contract() {
    let states = [
        Status::Absent,
        Status::Refused {
            reason: "Fixture storage refusal.".into(),
        },
        Status::File {
            path: "fixture-auth.json".into(),
            expiry: FileExpiry::Usable { expires_at: None },
        },
        Status::File {
            path: "fixture-auth.json".into(),
            expiry: FileExpiry::Usable {
                expires_at: Some(2_000_000_000),
            },
        },
        Status::File {
            path: "fixture-auth.json".into(),
            expiry: FileExpiry::Expired,
        },
        Status::Stored {
            expiry: StoredExpiry::Usable {
                expires_at: 2_000_000_000,
            },
        },
        Status::Stored {
            expiry: StoredExpiry::Expired {
                expires_at: 1_000_000_000,
            },
        },
    ];
    let expected: Vec<Value> =
        serde_json::from_str(include_str!("fixtures/codex_status.json")).unwrap();
    for (state, expected) in states.into_iter().zip(expected) {
        assert_eq!(serde_json::to_value(state).unwrap(), expected);
    }
}
