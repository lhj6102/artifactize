//! Readiness status wire forms and the public report's aggregate severity.
use super::*;
use serde_json::json;

#[test]
fn readiness_check_status_wire_and_display_are_pinned() {
    for (status, wire) in [
        (CheckStatus::Pass, "PASS"),
        (CheckStatus::Warn, "WARN"),
        (CheckStatus::Fail, "FAIL"),
    ] {
        assert_eq!(status.to_string(), wire);
        let check = Check {
            name: "fixture",
            status,
            message: "pinned message".into(),
            details: Details::State { writable: true },
        };
        assert_eq!(
            serde_json::to_value(check).unwrap(),
            json!({
                "name":"fixture",
                "status":wire,
                "message":"pinned message",
                "details":{"writable":true},
            })
        );
    }
}

#[tokio::test]
async fn later_ready_checks_do_not_erase_a_configuration_failure() {
    let root = crate::test_os::tempdir();
    let repo = root.path().join("repo");
    let state = root.path().join("state");
    fs::create_dir(&repo).unwrap();
    fs::write(repo.join(crate::config::CONFIG_FILE), "name = [").unwrap();
    let report = doctor(Some(&state), Some(&repo)).await.unwrap();
    assert!(!report.ok);
    assert_eq!(report.checks[0].name, "config");
    assert_eq!(report.checks[0].status, CheckStatus::Fail);
    for name in ["state", "schema"] {
        let check = report
            .checks
            .iter()
            .find(|check| check.name == name)
            .unwrap();
        assert_eq!(check.status, CheckStatus::Pass, "{name}");
    }
    assert!(!state.join(store::DATABASE).exists());
}

#[test]
fn typed_details_preserve_null_and_flat_auth_wire_forms() {
    assert_eq!(serde_json::to_value(Details::None).unwrap(), json!(null));
    assert_eq!(
        serde_json::to_value(Details::Schema { schema: None }).unwrap(),
        json!({"schema":null})
    );
    let codex = Details::Codex {
        status: auth::codex::Status::Absent,
        test_endpoint: Some("http://localhost".into()),
        test_auth_endpoint: None,
    };
    assert_eq!(
        serde_json::to_value(codex).unwrap(),
        json!({"source":"none","expiresAt":null,"expired":false,"testEndpoint":"http://localhost"})
    );
    assert_eq!(
        serde_json::to_value(Details::Desktop { path: None }).unwrap(),
        json!({"path":null})
    );
}

#[test]
fn a_newer_release_warns_with_its_update_and_an_unchecked_one_passes() {
    let version = |text| semver::Version::parse(text).unwrap();
    let page = "https://github.com/lhj6102/artifactize/releases/latest";
    let (status, message, details) = release_check(release::Status {
        running: version("0.15.1"),
        channel: release::Channel::Unknown,
        outcome: release::Outcome::Newer {
            latest: version("0.16.0"),
            update: release::Update::Page { url: page.into() },
        },
    });
    assert_eq!(status, CheckStatus::Warn);
    assert!(
        message.contains("0.16.0") && message.contains(page),
        "{message}"
    );
    assert_eq!(
        serde_json::to_value(details).unwrap(),
        json!({
            "channel":"unknown",
            "running":"0.15.1",
            "latest":"0.16.0",
            "update":{"kind":"page","url":page},
        })
    );
    let (status, _, details) = release_check(release::Status {
        running: version("0.15.1"),
        channel: release::Channel::Store,
        outcome: release::Outcome::Unchecked(release::Unchecked::Store),
    });
    assert_eq!(status, CheckStatus::Pass);
    assert_eq!(
        serde_json::to_value(details).unwrap(),
        json!({"channel":"store","running":"0.15.1"})
    );
}
