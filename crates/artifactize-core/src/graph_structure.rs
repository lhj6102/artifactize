//! Name-based project structure shared by the graph fingerprint and both graph tools.
//! Paths locate declarations, never identify graph entities. Extraction is flat, so Artifact
//! cycles need no recursive traversal and the graph Artifact includes itself without recursion.
use std::{collections::BTreeMap, fmt::Write, path::Path};

use serde::Serialize;
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

use crate::{
    config::{ArtifactKind, EvalPayload, Fingerprint, Profile, RepoConfig},
    scope::{Argument, RelationKind},
    types::{ArtifactName, EvalId},
};

/// Domain separation: changing the extraction contract invalidates old graph fingerprints.
const FORMAT: &[u8] = b"artifactize-graph-v1\n";

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Structure {
    pub artifacts: BTreeMap<ArtifactName, Artifact>,
    pub evals: BTreeMap<EvalId, Eval>,
    pub relations: Vec<Relation>,
}

#[derive(Debug, Serialize)]
pub struct Artifact {
    pub kind: ArtifactKind,
    pub basis: Option<bool>,
    pub fingerprint: Coverage,
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Coverage {
    Graph,
    Disabled,
    Artifactsum {
        files: Vec<crate::config::LogicalPath>,
        ignore: Vec<String>,
    },
    Script {
        command: String,
        args: Vec<Argument>,
        files: Vec<crate::config::LogicalPath>,
    },
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Eval {
    pub target: ArtifactName,
    pub title: String,
    pub gate: Option<crate::config::Gate>,
    pub criteria: Criteria,
    pub payload: Option<EvalPayload>,
    pub pass_schema: Option<Map<String, Value>>,
    pub fail_schema: Option<Map<String, Value>>,
}

/// One explicit extraction per profile: execution options never enter this type.
#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Criteria {
    Agent,
    Human,
    Dependency {
        depends_on: Vec<ArtifactName>,
    },
    Runtime {
        command: String,
        args: Vec<Argument>,
    },
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct Relation {
    pub source: ArtifactName,
    pub target: ArtifactName,
    #[serde(flatten)]
    pub kind: Edge,
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Edge {
    Child,
    Mount {
        alias: crate::config::MountAlias,
    },
    Dependency {
        eval_id: EvalId,
        name: crate::config::ReferenceName,
    },
    Instruction {
        eval_id: EvalId,
        name: crate::config::ReferenceName,
    },
    Argv {
        eval_id: EvalId,
        index: usize,
        name: crate::config::ReferenceName,
        path: String,
    },
}

pub fn extract(config: &RepoConfig) -> Structure {
    let artifacts = config
        .artifacts
        .iter()
        .map(|(name, artifact)| {
            let fingerprint = match &artifact.fingerprint {
                Some(Fingerprint::Graph) => Coverage::Graph,
                None => Coverage::Disabled,
                Some(Fingerprint::Artifactsum { files, ignore }) => Coverage::Artifactsum {
                    files: files.clone(),
                    ignore: ignore.clone(),
                },
                Some(Fingerprint::Script {
                    command,
                    args,
                    files,
                    ..
                }) => Coverage::Script {
                    command: command.clone(),
                    args: args.clone(),
                    files: files.clone(),
                },
            };
            (
                name.clone(),
                Artifact {
                    kind: artifact.kind,
                    basis: artifact.basis,
                    fingerprint,
                },
            )
        })
        .collect();
    let evals = config
        .evals
        .iter()
        .map(|eval| {
            let declaration = &eval.declaration;
            let criteria = match declaration.profile() {
                Profile::Agent { .. } => Criteria::Agent,
                Profile::Human {} => Criteria::Human,
                Profile::Dependency { depends_on } => Criteria::Dependency {
                    depends_on: depends_on.clone(),
                },
                Profile::Runtime { command, args, .. } => Criteria::Runtime {
                    command: command.clone(),
                    args: args.clone(),
                },
            };
            (
                eval.id.clone(),
                Eval {
                    target: eval.target.clone(),
                    title: declaration.title.clone(),
                    gate: declaration.gate,
                    criteria,
                    payload: declaration.payload().cloned(),
                    pass_schema: declaration.pass_schema().cloned(),
                    fail_schema: declaration.fail_schema().cloned(),
                },
            )
        })
        .collect();
    let mut relations: Vec<_> = config
        .relations
        .iter()
        .map(|relation| Relation {
            source: relation.source.clone(),
            target: relation.target.clone(),
            kind: match &relation.kind {
                RelationKind::Child { .. } => Edge::Child,
                RelationKind::Mount { alias } => Edge::Mount {
                    alias: alias.clone(),
                },
                RelationKind::Dependency { eval_id, name } => Edge::Dependency {
                    eval_id: eval_id.clone(),
                    name: name.clone(),
                },
                RelationKind::Instruction { eval_id, name } => Edge::Instruction {
                    eval_id: eval_id.clone(),
                    name: name.clone(),
                },
                RelationKind::Argument {
                    eval_id,
                    index,
                    name,
                    path,
                } => Edge::Argv {
                    eval_id: eval_id.clone(),
                    index: *index,
                    name: name.clone(),
                    path: path.clone(),
                },
            },
        })
        .collect();
    relations.sort();
    Structure {
        artifacts,
        evals,
        relations,
    }
}

/// Reload declarations, not file contents: tools and end-of-review rechecks see current state.
pub fn read(repo: &Path) -> Result<Structure, String> {
    crate::config::read_workspace_declarations(repo)
        .map(|config| extract(&config))
        .map_err(|error| error.to_string())
}

impl Structure {
    pub fn fingerprint(&self) -> crate::types::Fingerprint {
        let mut value = serde_json::to_value(self).expect("graph structure is JSON");
        value.sort_all_objects();
        let mut digest = Sha256::new();
        digest.update(FORMAT);
        digest.update(serde_json::to_vec(&value).expect("graph structure is JSON"));
        let hex: String = digest
            .finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        format!("graph:{hex}")
            .parse()
            .expect("graph fingerprint is nonblank")
    }

    /// A flat, cycle-safe entity tree; arrows always run from input to consumer.
    /// The criteria JSON exposes exactly the data hashed, not a separate graph projection.
    pub fn tree(&self) -> String {
        let mut text = format!("Artifact graph {}\n", self.fingerprint());
        for (name, artifact) in &self.artifacts {
            writeln!(text, "{name} [{:?}]", artifact.kind).expect("write to String");
            writeln!(
                text,
                "  definition: {}",
                serde_json::to_string(artifact).expect("Artifact is JSON")
            )
            .expect("write to String");
            for (id, eval) in self.evals.iter().filter(|(_, eval)| &eval.target == name) {
                writeln!(text, "  ├─ {id}: {}", eval.title).expect("write to String");
                writeln!(
                    text,
                    "  │  criteria: {}",
                    serde_json::to_string(eval).expect("Eval is JSON")
                )
                .expect("write to String");
            }
        }
        for relation in &self.relations {
            writeln!(
                text,
                "{} → {} {}",
                relation.source,
                relation.target,
                serde_json::to_string(&relation.kind).expect("relation is JSON")
            )
            .expect("write to String");
        }
        text
    }
}

/// The existing desktop application's detached launch path, with a current-project graph
/// screen rather than a historical Run. No shell or second renderer is involved.
pub fn launch(repo: &Path) -> Result<(), String> {
    // Validate the same structure before handing it to the desktop viewer.
    read(repo)?;
    let program =
        crate::platform::program::resolve(std::ffi::OsStr::new("artifactize-desktop"), repo)
            .map_err(|error| format!("Cannot find artifactize-desktop: {error}"))?;
    crate::process::launch_detached(
        program.as_os_str(),
        &[
            "graph".into(),
            "--repo".into(),
            crate::platform::path_text(repo),
        ],
        repo,
    )
    .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests;
