#[path = "../tests/support/declaration.rs"]
mod shared;
pub use shared::{read, to_toml, write};

/// Parse the real input shape without semantic validation, for strategy/schema tests.
pub fn eval(value: serde_json::Value) -> Result<crate::config::EvalDeclaration, String> {
    let source = to_toml(serde_json::json!({"name":"test","evals":[value]}))?;
    let mut declaration: crate::config::ArtifactDeclaration =
        toml::from_str(&source).map_err(|error| error.to_string())?;
    Ok(declaration.evals.remove(0))
}
