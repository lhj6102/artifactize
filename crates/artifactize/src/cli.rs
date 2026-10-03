//! Command-line parsing, projections, and exit codes.

use std::path::PathBuf;

use clap::{CommandFactory, Parser};

#[derive(Debug, Parser)]
#[command(
    name = "artifactize",
    version,
    about = "Pull validation and explicit review execution"
)]
pub struct Cli {
    /// Repository input path.
    #[arg(long, global = true, value_name = "PATH")]
    pub repo: Option<PathBuf>,

    /// Run receipts and history directory; global services stay in the state home.
    #[arg(long, global = true, value_name = "PATH")]
    pub state_dir: Option<PathBuf>,

    /// Use JSON for command results.
    #[arg(long, global = true)]
    pub json: bool,
}

pub fn run() -> std::io::Result<()> {
    let _cli = Cli::parse();
    Cli::command().print_help()
}
