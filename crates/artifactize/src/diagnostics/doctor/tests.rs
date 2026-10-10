//! All finite readiness statuses preserve their exact JSON/text and report severity.
use super::*;
use serde_json::json;

#[test]
fn readiness_check_status_wire_display_and_severity_are_pinned() {
    for (status, wire, ready) in [
        (CheckStatus::Pass, "PASS", true),
        (CheckStatus::Warn, "WARN", true),
        (CheckStatus::Fail, "FAIL", false),
    ] {
        assert_eq!(status.to_string(), wire);
        assert_eq!(serde_json::to_value(status).unwrap(), json!(wire));
        let mut report = DoctorReport {
            ok: true,
            state_dir: "/fixture-state".into(),
            checks: Vec::new(),
        };
        report.add(
            "fixture",
            status,
            "pinned message",
            Details::State { writable: true },
        );
        assert_eq!(report.ok, ready);
        assert_eq!(
            serde_json::to_value(&report.checks).unwrap(),
            json!([
                {
                    "name":"fixture",
                    "status":wire,
                    "message":"pinned message",
                    "details":{"writable":true},
                },
            ])
        );
    }
    let mut report = DoctorReport {
        ok: true,
        state_dir: "/fixture-state".into(),
        checks: Vec::new(),
    };
    for status in [
        CheckStatus::Pass,
        CheckStatus::Warn,
        CheckStatus::Fail,
        CheckStatus::Pass,
        CheckStatus::Warn,
    ] {
        report.add("fixture", status, "", Details::None);
    }
    assert!(
        !report.ok,
        "later warnings/pass cannot erase an earlier failure"
    );
}
