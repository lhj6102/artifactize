#[path = "../tests/support/declaration.rs"]
mod shared;
pub use shared::{read, read_file, read_text, remove_file, source_path, to_artf, to_files, write};

/// One eval as a project reads it, from a builder, for strategy and hashing tests.
pub fn eval(value: serde_json::Value) -> Result<crate::config::EvalDeclaration, String> {
    let root = tempfile::tempdir().map_err(|error| error.to_string())?;
    let files = to_files(serde_json::json!({
        "name": "test",
        "path": "./",
        "type": "files",
        "evals": [value.clone()],
    }))?;
    let definitions = root.path().join(".artifactize");
    std::fs::create_dir_all(&definitions).map_err(|error| error.to_string())?;
    for (file, text) in files {
        std::fs::write(definitions.join(file), text).map_err(|error| error.to_string())?;
    }
    // The Artifacts it names exist, as inputs with nothing to review.
    let mut named =
        crate::config::convert::references(value_text(&value["payload"]["instruction"]).as_str());
    for arg in value["profile"]["args"].as_array().into_iter().flatten() {
        named.extend(crate::config::convert::references(value_text(arg).as_str()));
    }
    named.sort();
    named.dedup();
    for name in named.into_iter().filter(|name| name != "test") {
        let stub = to_artf(
            serde_json::json!({"name": name, "path": "./", "basis": true, "type": "files"}),
        )?;
        std::fs::write(definitions.join(format!("{name}.artf")), stub)
            .map_err(|error| error.to_string())?;
    }
    let mut config =
        crate::config::read_workspace_config(root.path()).map_err(|error| error.to_string())?;
    Ok(config.evals.remove(0).declaration)
}

fn value_text(value: &serde_json::Value) -> String {
    value.as_str().unwrap_or_default().to_owned()
}
