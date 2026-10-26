use std::{path::Path, process::Command};

#[path = "../../artifactize-core/tests/support/mod.rs"]
mod support;

#[test]
fn hey_arti_harness_checks_source_cases_and_fake_builder_declarations_with_the_candidate() {
    let scratch = support::os::tempdir();
    let repository = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut command = Command::new(support::os::python_program());
    support::os::temporary_environment(&mut command, scratch.path());
    let output = support::os::run(
        command
            .arg("-I")
            .arg(repository.join("harness/hey-arti/driver.py"))
            .arg("check")
            .arg("--artifactize")
            .arg(support::os::cli())
            .env("ARTIFACTIZE_REMOTE", "off"),
    );
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
