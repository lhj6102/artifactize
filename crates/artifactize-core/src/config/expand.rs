//! What the code declares, for people and agents to inspect: each Artifact as authored and as
//! expanded, and every global binding with its macro signature.

use std::path::{Path, PathBuf};

use serde::Serialize;

use super::{ArtifactName, ConfigError, RepoConfig, Symbol, discovery, lang};

/// One Artifact's code: the statement that binds it and the declaration it expands to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Expansion {
    pub artifact: ArtifactName,
    #[serde(serialize_with = "crate::platform::path_serde::serialize")]
    pub file: PathBuf,
    pub line: usize,
    #[serde(rename = "macro", skip_serializing_if = "Option::is_none")]
    pub macro_name: Option<String>,
    /// The whole lines of the `artf` statement as written; they start at `line`.
    pub authored: String,
    pub expanded: String,
    pub evals: Vec<EvalSite>,
}

/// Where an eval of the expansion is written, which may be inside a macro.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EvalSite {
    pub id: String,
    #[serde(serialize_with = "crate::platform::path_serde::serialize")]
    pub file: PathBuf,
    pub line: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
}

/// The expansion of `artifact` in a configuration read from code.
pub fn expansion(config: &RepoConfig, artifact: &ArtifactName) -> Option<Expansion> {
    let program = config.program.as_ref()?;
    let lowered = program
        .artifacts
        .iter()
        .find(|lowered| lowered.name == artifact.as_str())?;
    Some(of(program, lowered))
}

fn of(program: &lang::Program, lowered: &lang::LoweredArtifact) -> Expansion {
    let (line, authored) = program.sources.lines(lowered.binding);
    Expansion {
        artifact: lowered.name.parse().expect("a validated Artifact name"),
        file: program.sources.file(lowered.binding.file).path.clone(),
        line,
        macro_name: lowered.macro_name.as_ref().map(|name| format!("{name}!")),
        authored: authored.to_owned(),
        expanded: lang::render::artifact(lowered),
        evals: lowered
            .evals
            .iter()
            .map(|eval| EvalSite {
                id: eval.id.clone(),
                file: program.sources.file(eval.id_span.file).path.clone(),
                line: program.sources.line_column(eval.id_span).0,
                from: eval.source.clone(),
            })
            .collect(),
    }
}

/// Every Artifact's expansion, or one Artifact's.
pub fn expand(repo: &Path, artifact: Option<&ArtifactName>) -> Result<Vec<Expansion>, ConfigError> {
    let config = discovery::read_workspace_declarations(repo)?;
    let program = config.program.as_ref().expect("a workspace read has its program");
    let expansions: Vec<_> = program
        .artifacts
        .iter()
        .filter(|lowered| artifact.is_none_or(|name| lowered.name == name.as_str()))
        .map(|lowered| of(program, lowered))
        .collect();
    if let Some(name) = artifact
        && expansions.is_empty()
    {
        return Err(ConfigError::new(repo, format!("Unknown Artifact {name}.")));
    }
    Ok(expansions)
}

/// Every global binding of the project and its plugins: kind, name, `!` and parameters.
pub fn symbols(repo: &Path) -> Result<Vec<Symbol>, ConfigError> {
    let config = discovery::read_workspace_declarations(repo)?;
    let program = config.program.as_ref().expect("a workspace read has its program");
    Ok(program.symbols.clone())
}
