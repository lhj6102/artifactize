//! The screenshot runtime accepts real checked-in captures and rejects changed bytes or claims.
use serde_json::{Value, json};
use std::{fs, path::Path, process::Command};

#[path = "../../artifactize-core/tests/support/mod.rs"]
mod support;

/// The second IHDR CRC byte, after the PNG signature, chunk header and payload.
const IHDR_CRC_BYTE: usize = 30;

fn check(script: &Path, directory: &Path, code: i32, diagnostic: &str) {
    let output = Command::new(support::os::python_program())
        .arg("-I")
        .arg(script)
        .arg(directory)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(code), "{output:?}");
    if code == 0 {
        let report: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(report["captures"], 4);
    } else {
        assert!(
            String::from_utf8_lossy(&output.stderr).contains(diagnostic),
            "{output:?}"
        );
    }
}

#[test]
fn screenshot_integrity_checks_the_real_bytes_and_honest_unclaimed_capture_evidence() {
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/screenshots");
    let script = source.join("check.py");
    let root = support::os::tempdir();
    for name in [
        "monitor.png",
        "monitor-narrow.png",
        "review.png",
        "review-narrow.png",
        "evidence.json",
    ] {
        fs::copy(source.join(name), root.path().join(name)).unwrap();
    }
    let original = fs::read_to_string(root.path().join("evidence.json")).unwrap();
    check(&script, root.path(), 0, "");

    let path = root.path().join("monitor.png");
    let image = fs::read(&path).unwrap();
    let mut corrupt = image.clone();
    // Corrupt the first chunk's CRC, not merely metadata: even a recomputed outer
    // evidence hash must not make broken PNG bytes a valid screenshot.
    corrupt[IHDR_CRC_BYTE] ^= 1;
    fs::write(&path, &corrupt).unwrap();
    check(&script, root.path(), 1, "capture hash mismatch");
    let mut evidence: Value = serde_json::from_str(&original).unwrap();
    evidence["screenshots"][0]["sha256"] =
        json!(artifactize_core::types::Sha256Digest::of(&corrupt).to_string());
    fs::write(root.path().join("evidence.json"), evidence.to_string()).unwrap();
    check(&script, root.path(), 1, "PNG CRC mismatch");
    fs::write(&path, image).unwrap();

    evidence = serde_json::from_str(&original).unwrap();
    evidence["requests"][1]["claim"] = json!({"reviewer":"alice"});
    fs::write(root.path().join("evidence.json"), evidence.to_string()).unwrap();
    check(&script, root.path(), 1, "captures contain claims");

    evidence = serde_json::from_str(&original).unwrap();
    evidence["screenshots"][0]["file"] = json!("../monitor.png");
    fs::write(root.path().join("evidence.json"), evidence.to_string()).unwrap();
    check(&script, root.path(), 1, "missing or duplicate named");
    fs::write(root.path().join("evidence.json"), original).unwrap();
    check(&script, root.path(), 0, "");
}
