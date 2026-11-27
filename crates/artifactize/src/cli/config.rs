//! Apply declaration changes and present the owner's migration report.

use clap::Subcommand;

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
    let renamed = artifactize_core::config::rename::rename(&repo, &old, &new)
        .map_err(|error| error.to_string())?;
    if context.json {
        print_json(&renamed)?;
    } else {
        let mut output = io::stdout().lock();
        for (file, names) in &renamed.files {
            writeln!(
                output,
                "{}: {names} {}",
                artifactize_tools::files::path_text(file),
                if *names == 1 { "name" } else { "names" }
            )
            .map_err(|error| error.to_string())?;
        }
        if let Some((from, to)) = &renamed.moved {
            writeln!(
                output,
                "{} -> {}",
                artifactize_tools::files::path_text(from),
                artifactize_tools::files::path_text(to)
            )
            .map_err(|error| error.to_string())?;
        }
        writeln!(output, "Renamed {old} to {new}.").map_err(|error| error.to_string())?;
    }
    Ok(0)
}

pub(super) fn migrate(context: Context) -> Result<u8, String> {
    let repo = context.repo.unwrap_or_else(|| PathBuf::from("."));
    let report =
        artifactize_core::config::migrate::migrate(&repo).map_err(|error| error.to_string())?;
    if context.json {
        print_json(&report)?;
        return Ok(0);
    }
    let mut output = io::stdout().lock();
    if report.written.is_empty() && report.removed.is_empty() {
        writeln!(output, "The definitions are already in the .artf language.")
            .map_err(|error| error.to_string())?;
        return Ok(0);
    }
    for path in &report.written {
        writeln!(
            output,
            "wrote {}",
            artifactize_tools::files::path_text(path)
        )
        .map_err(|error| error.to_string())?;
    }
    for path in &report.removed {
        writeln!(
            output,
            "removed {}",
            artifactize_tools::files::path_text(path)
        )
        .map_err(|error| error.to_string())?;
    }
    for warning in &report.warnings {
        writeln!(output, "Note: {warning}").map_err(|error| error.to_string())?;
    }
    Ok(0)
}

/// Print each Artifact's binding as written and the declaration it expands to.
pub(super) fn expand(context: Context, artifact: Option<ArtifactName>) -> Result<u8, String> {
    let repo = context.repo.unwrap_or_else(|| PathBuf::from("."));
    let expansions = artifactize_core::config::expand::expand(&repo, artifact.as_ref())
        .map_err(|error| error.to_string())?;
    if context.json {
        print_json(&expansions)?;
        return Ok(0);
    }
    let mut output = io::stdout().lock();
    for (index, expansion) in expansions.iter().enumerate() {
        if index > 0 {
            writeln!(output).map_err(|error| error.to_string())?;
        }
        writeln!(
            output,
            "// {}:{}{}",
            artifactize_tools::files::path_text(&expansion.file),
            expansion.line,
            expansion
                .macro_name
                .as_ref()
                .map_or(String::new(), |name| format!(" (by {name})"))
        )
        .map_err(|error| error.to_string())?;
        write!(output, "{}", expansion.expanded).map_err(|error| error.to_string())?;
    }
    Ok(0)
}

/// Print the global bindings: profiles, tools, evals and Artifacts, and their macros.
pub(super) fn symbols(context: Context) -> Result<u8, String> {
    let repo = context.repo.unwrap_or_else(|| PathBuf::from("."));
    let symbols =
        artifactize_core::config::expand::symbols(&repo).map_err(|error| error.to_string())?;
    if context.json {
        print_json(&symbols)?;
        return Ok(0);
    }
    let mut output = io::stdout().lock();
    for symbol in &symbols {
        let params = symbol.params.as_ref().map_or(String::new(), |params| {
            let params: Vec<_> = params
                .iter()
                .map(|param| match &param.default {
                    Some(default) => format!("{} = {default}", param.name),
                    None => param.name.clone(),
                })
                .collect();
            format!("({})", params.join(", "))
        });
        let place = match &symbol.plugin {
            Some(plugin) => format!("plugin {plugin}"),
            None => format!(
                "{}:{}",
                artifactize_tools::files::path_text(&symbol.file),
                symbol.line
            ),
        };
        writeln!(
            output,
            "{}{} {}{params}  {place}",
            symbol.kind,
            if symbol.is_macro { "!" } else { "" },
            symbol.name
        )
        .map_err(|error| error.to_string())?;
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
        super::graph::graph(&view).map_err(|error| error.to_string())?;
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

#[derive(Debug, Subcommand)]
pub enum ConfigCommand {
    /// Convert a project from before 0.27 into the .artf language, once.
    Migrate,
    /// Rename an Artifact where it is bound and everywhere a definition names it.
    Rename {
        old: artifactize_core::types::ArtifactName,
        new: artifactize_core::types::ArtifactName,
    },
    /// Print what each Artifact expands to, macros and toolsets filled in.
    Expand {
        /// One Artifact; defaults to all.
        artifact: Option<artifactize_core::types::ArtifactName>,
    },
    /// List the global bindings: Artifacts, tools, profiles, evals and their macros.
    Symbols,
    /// Validate declarations and unique Artifact/Eval ids.
    Check,
    /// Inspect full static definitions, relations, and cycles.
    Graph {
        /// Select one Artifact and its required closure; defaults to all.
        artifact: Option<artifactize_core::types::ArtifactName>,
        /// Include downstream consumers and their required inputs (conservative impact).
        #[arg(long)]
        impact: bool,
    },
}
