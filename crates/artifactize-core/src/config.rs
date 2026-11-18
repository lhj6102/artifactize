//! Inert discovery and strict declarations.

use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

use thiserror::Error;

mod backend;
mod declaration;
mod definition;
mod discovery;
pub mod edit;
mod eval;
mod identities;
mod legacy;
mod location;
pub mod migrate;
mod mutex;
mod plain;
mod profile;
mod schemas;
mod site;
mod stored;
mod tools;
pub(crate) mod validation;
mod workspace;

pub use crate::types::{ArtifactName, EvalId};
pub use artifactize_tools::scope::{ArtifactKind, Target, Uri};
pub use backend::{Backend, Provider};
pub use declaration::{ArtifactDeclaration, Fingerprint, Views, parse_declaration};
pub use discovery::read_workspace_config;
pub(crate) use discovery::read_workspace_declarations;
use discovery::{logical_join, read_regular_text};
pub use eval::{EvalDeclaration, EvalPayload, LocalEvalId};
pub use identities::{
    ChildPrefix, EndpointId, ExecutionPinName, HubEpoch, LogicalPath, ModelId, ProfileVariantName,
    ReferenceName, ToolName, ToolOperationName,
};
pub use mutex::MutexTag;
pub use profile::{Profile, ProfileKind, Reasoning};
pub use site::{ScopeReadError, Site, read_sites};
pub(crate) use site::{read_artifact_scope, read_eval_scope};
pub use stored::{Field, StoredPayload, StoredProfile};
pub use tools::{
    AgentTool, Builtin, BuiltinTool, CommandTool, HumanBuiltinTool, HumanCommandTool, HumanTool,
    HumanToolKind, SessionCommand, ToolProtocol, ToolSession,
};
pub use validation::MAX_TIMEOUT_MS;
pub(crate) use validation::{artifact_name, identifier};
pub use workspace::{Artifact, Eval, RepoConfig, ReviewRequirement};

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
    pub message: String,
    declaration: Option<(Vec<String>, String)>,
}

impl ConfigError {
    pub(crate) fn new(path: impl Into<PathBuf>, message: impl ToString) -> Self {
        Self {
            path: path.into(),
            message: message.to_string(),
            declaration: None,
        }
    }

    /// Position cross-Artifact validation failures at their declaration's typed key.
    pub(crate) fn declaration(
        path: impl Into<PathBuf>,
        keys: &[&str],
        message: impl ToString,
    ) -> Self {
        let path = path.into();
        let message = message.to_string();
        let declaration = Some((
            keys.iter().map(|key| (*key).to_owned()).collect(),
            message.clone(),
        ));
        let message = read_regular_text(&path).map_or(message.clone(), |source| {
            location::error_at(&source, keys, &message)
        });
        Self {
            path,
            message,
            declaration,
        }
    }
}

#[cfg(test)]
mod tests;
