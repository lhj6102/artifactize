use std::{path::Path, process::Command};

#[path = "../../artifactize-core/tests/support/mod.rs"]
mod support;

#[test]
fn hey_arti_harness_checks_source_cases_and_fake_builder_declarations_with_the_candidate() {
    let scratch = support::os::tempdir();
    let repository = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let output = Command::new(support::os::python_program())
        .arg("-I")
        .arg(repository.join("harness/hey-arti/driver.py"))
        .arg("check")
        .arg("--artifactize")
        .arg(support::os::cli())
        .env("TMPDIR", scratch.path())
        .env("TMP", scratch.path())
        .env("TEMP", scratch.path())
        .env("ARTIFACTIZE_REMOTE", "off")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
