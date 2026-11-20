//! Text projection of the configuration graph.

use std::io::{self, Write};

use super::render::tags;

pub(super) fn graph(view: &artifactize_core::query::GraphView<'_>) -> io::Result<()> {
    let mut out = io::stdout().lock();
    writeln!(
        out,
        "Graph: {} Artifacts, {} Evals",
        view.artifacts.len(),
        view.evals.len()
    )?;
    for component in &view.components {
        writeln!(
            out,
            "  Component {}{}: {}",
            component.id,
            if component.cyclic { " [cycle]" } else { "" },
            component
                .artifacts
                .iter()
                .map(|id| id.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )?;
        for id in &component.artifacts {
            let artifact = view.artifacts[id];
            writeln!(
                out,
                "    Artifact {id}{}{}{} ({})",
                tags(&artifact.tags),
                if artifact.kind() == artifactize_core::config::ArtifactKind::File {
                    " [file]"
                } else if artifact.kind() == artifactize_core::config::ArtifactKind::Live {
                    " [live]"
                } else {
                    ""
                },
                if artifact.basis == Some(true) {
                    " [basis]"
                } else {
                    ""
                },
                if artifact
                    .path
                    .local()
                    .is_some_and(|path| path.as_os_str().is_empty())
                {
                    ".".into()
                } else {
                    artifact.path.to_string()
                }
            )?;
            for eval in view.evals.iter().filter(|eval| eval.target == **id) {
                writeln!(
                    out,
                    "      {}: {} -> {}",
                    eval.id,
                    if eval.deps.is_empty() {
                        "(no deps)".into()
                    } else {
                        eval.deps.join(", ")
                    },
                    eval.target
                )?;
            }
        }
    }
    for edge in &view.relations {
        use artifactize_core::scope::RelationKind;
        let detail = match &edge.relation.kind {
            RelationKind::Part => "part".into(),
            RelationKind::Instruction { eval_id, name } => {
                format!("instruction eval={eval_id} name={name}")
            }
            RelationKind::Argument {
                eval_id,
                index,
                name,
                path,
            } => format!("argv eval={eval_id} index={index} name={name} path={path}"),
        };
        writeln!(
            out,
            "  {} -> {} [{detail}]{}",
            edge.relation.source,
            edge.relation.target,
            if edge.cyclic { " [cycle]" } else { "" }
        )?;
    }
    Ok(())
}
