//! Apply declaration changes and present the owner's migration report.

use super::{Context, print_json};
use artifactize_core::{
    config::read_workspace_config, project::selection::Selection, types::ArtifactName,
};
use std::{
    io::{self, Write},
    path::PathBuf,
};

pub(super) fn rename(context: Context, old: ArtifactName, new: ArtifactName) -> Result<u8, String> {
    let repo = context.repo.unwrap_or_else(|| PathBuf::from("."));
    let config = artifactize_core::config::read_workspace_config(&repo)
        .map_err(|error| error.to_string())?;
    let artifact = config
        .artifacts
        .get(&old)
        .ok_or_else(|| format!("Unknown Artifact {old}."))?;
    let destination = artifact
        .declaration_path()
        .with_file_name(format!("{new}.artf"));
    let mut based =
        artifactize_core::config::edit::read(&repo).map_err(|error| error.to_string())?;
    based.entry(destination).or_insert(None);
    let result = artifactize_core::config::edit::apply(
        &repo,
        &based,
        artifactize_core::config::edit::Operation::Rename { old, new },
    )
    .map_err(|error| error.to_string())?;
    if context.json {
        print_json(&result)?;
    }
    Ok(0)
}

pub(super) fn migrate(context: Context) -> Result<u8, String> {
    let repo = context.repo.unwrap_or_else(|| PathBuf::from("."));
    let report =
        artifactize_core::config::migrate::migrate(&repo).map_err(|error| error.to_string())?;
    if context.json {
        print_json(&report)?;
    } else {
        let mut output = io::stdout().lock();
        for (old, new) in report.files {
            writeln!(
                output,
                "{} -> {}",
                artifactize_tools::files::path_text(&old),
                artifactize_tools::files::path_text(&new)
            )
            .map_err(|error| error.to_string())?;
        }
        for (whole, parts) in report.explicit_parts {
            writeln!(
                output,
                "{whole}: parts = {}",
                serde_json::to_string(&parts).map_err(|error| error.to_string())?
            )
            .map_err(|error| error.to_string())?;
        }
        for name in report.ignored {
            writeln!(output, "{name}: ignore added: .artifactize/")
                .map_err(|error| error.to_string())?;
        }
        for path in report.separate_projects {
            writeln!(
                output,
                "Separate project (run config migrate --repo there): {}",
                artifactize_tools::files::path_text(&path)
            )
            .map_err(|error| error.to_string())?;
        }
    }
    Ok(0)
}

pub(super) fn graph(
    context: Context,
    artifact: Option<artifactize_core::types::ArtifactName>,
    impact: bool,
) -> Result<u8, String> {
    let config = read_workspace_config(&context.repo.unwrap_or_else(|| PathBuf::from(".")))
        .map_err(|error| error.to_string())?;
    let selection = artifact.map_or(Selection::All, |artifact_id| Selection::Artifact {
        artifact_id,
    });
    let selection = if impact {
        artifactize_core::query::impact(&config, &selection)?
    } else {
        selection
    };
    let view = artifactize_core::query::graph(&config, &selection)?;
    if context.json {
        print_json(&view)?;
    } else {
        super::render::graph(&view).map_err(|error| error.to_string())?;
    }
    Ok(0)
}

pub(super) fn check(context: Context) -> Result<u8, String> {
    let repo = context.repo.unwrap_or_else(|| PathBuf::from("."));
    let report = artifactize_core::inspect::config_check(&repo)?;
    if context.json {
        print_json(&report)?;
    } else {
        for note in &report.notes {
            writeln!(io::stdout().lock(), "Note: {note}").map_err(|error| error.to_string())?;
        }
        writeln!(io::stdout().lock(), "Folder configuration is valid.")
            .map_err(|error| error.to_string())?;
    }
    Ok(0)
}
