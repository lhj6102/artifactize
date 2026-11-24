use std::{
    io::{self, Write},
    path::PathBuf,
};

use artifactize_core::config::{ArtifactType, plugins};
use clap::Subcommand;

use super::{Context, print_json};

#[derive(Debug, Subcommand)]
pub enum PluginCommand {
    /// Install the plugin folder at PATH into this project.
    Add { path: PathBuf },
    /// Remove a project-installed plugin by its type name.
    Remove { r#type: ArtifactType },
    /// List built-in and project-installed types.
    List,
}

pub(super) fn execute(context: Context, command: PluginCommand) -> Result<u8, String> {
    let repo = context.repo.unwrap_or_else(|| PathBuf::from("."));
    let describe = |info: &plugins::PluginInfo| {
        let source = if info.builtin { "built-in" } else { "project" };
        writeln!(
            io::stdout().lock(),
            "{}\t{}\t{source}",
            info.name,
            info.title
        )
        .map_err(|error| error.to_string())
    };
    match command {
        PluginCommand::Add { path } => {
            let info = plugins::add(&repo, &path).map_err(|error| error.to_string())?;
            if context.json {
                print_json(&info)?;
            } else {
                describe(&info)?;
            }
        }
        PluginCommand::Remove { r#type: name } => {
            plugins::remove(&repo, &name).map_err(|error| error.to_string())?;
            if context.json {
                print_json(&name)?;
            } else {
                writeln!(io::stdout().lock(), "Removed {name}.")
                    .map_err(|error| error.to_string())?;
            }
        }
        PluginCommand::List => {
            let list = plugins::list(&repo).map_err(|error| error.to_string())?;
            if context.json {
                print_json(&list)?;
            } else {
                for info in list {
                    describe(&info)?;
                }
            }
        }
    }
    Ok(0)
}
