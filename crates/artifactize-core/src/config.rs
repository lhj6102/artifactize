//! Inert discovery and strict declarations.

use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

use thiserror::Error;

mod authored_target;
mod backend;
#[doc(hidden)]
pub mod convert;
mod declaration;
mod discovery;
mod eval;
pub mod expand;
mod identities;
pub(crate) mod lang;
// mod legacy;
// pub mod migrate;
mod mutex;
mod plain;
pub mod plugins;
mod profile;
// pub mod rename;
mod schemas;
mod site;
mod stored;
mod tools;
pub(crate) mod validation;
mod workspace;

pub use crate::types::{ArtifactName, EvalId};
pub use artifactize_tools::scope::{ArtifactKind, Target, Uri};
pub use backend::{Backend, CustomBackendName, DirectBackend, HttpProvider, Provider};
pub use declaration::{Fingerprint, Views};
pub use discovery::read_workspace_config;
pub(crate) use discovery::read_workspace_declarations;
use discovery::logical_join;
pub use eval::{
    Audience, EvalDeclaration, EvalPayload, InstructionSlot, LocalEvalId, Priority, Slot,
};
pub use identities::{
    ChildPrefix, EndpointId, ExecutionPinName, HubEpoch, LogicalPath, ModelId, PluginName,
    ProfileName, ReferenceName, ToolFqn, ToolName,
};
pub use lang::Symbol;
pub use mutex::MutexTag;
pub use profile::{Profile, ProfileKind, Reasoning};
pub use site::{ScopeReadError, Site, read_sites};
pub(crate) use site::{read_artifact_scope, read_eval_scope};
pub use stored::{Field, StoredPayload, StoredProfile};
pub use tools::{
    AgentTool, Builtin, BuiltinTool, CommandTool, HumanBuiltinTool, HumanCommandTool, HumanTool,
    HumanToolKind, HumanViewerTool, SessionCommand, ToolProtocol, ToolSession,
};
pub use validation::MAX_TIMEOUT_MS;
pub(crate) use validation::{artifact_name, identifier};
pub use workspace::{Artifact, Binding, Eval, RepoConfig, ReviewRequirement};

/// Format version for artifactize configuration documents.
pub const CONFIG_FORMAT_VERSION: u32 = 1;
pub const CONFIG_FILE: &str = "index.artf";
/// Gitignore-style folder patterns, at the workspace root, that discovery skips.
pub const IGNORE_FILE: &str = ".artfignore";
/// Bound named variants and declared input lists to keep discovery and preparation finite.
const MAX_DECLARED_ITEMS: usize = 64;

#[derive(Debug, Error)]
#[error("{}: {message}", crate::platform::path_text(.path))]
pub struct ConfigError {
    pub path: PathBuf,
    /// `line L, column C: …`, then a `note:` line per related place (a macro's call site,
    /// the other side of a duplicate).
    pub message: String,
}

impl ConfigError {
    pub(crate) fn new(path: impl Into<PathBuf>, message: impl ToString) -> Self {
        Self {
            path: path.into(),
            message: message.to_string(),
        }
    }
}

#[cfg(test)]
mod tests;

/// One definition text's Artifact and evals, read as a project with only that file: for tests
/// of declaration rules that need no files.
#[cfg(test)]
pub(crate) struct ParsedDeclaration {
    pub evals: Vec<EvalDeclaration>,
    pub views: Views,
    pub tags: Vec<String>,
    pub basis: Option<bool>,
    pub fingerprint: Option<Fingerprint>,
}

#[cfg(test)]
pub(crate) fn parse_declaration(text: &str) -> Result<ParsedDeclaration, String> {
    let root = tempfile::tempdir().map_err(|error| error.to_string())?;
    let definitions = root.path().join(discovery::DEFINITION_DIR);
    std::fs::create_dir_all(&definitions).map_err(|error| error.to_string())?;
    std::fs::write(definitions.join("test.artf"), text).map_err(|error| error.to_string())?;
    let config = read_workspace_declarations(root.path()).map_err(|error| error.to_string())?;
    let artifact = config
        .artifacts
        .into_values()
        .next()
        .ok_or("The text declares no Artifact.")?;
    Ok(ParsedDeclaration {
        evals: config.evals.into_iter().map(|eval| eval.declaration).collect(),
        views: artifact.views,
        tags: artifact.tags,
        basis: artifact.basis,
        fingerprint: artifact.fingerprint,
    })
}
