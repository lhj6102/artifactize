//! The real library worker entry points, without a dependency on any user interface.
use artifactize_core::{runs, types::RunId};
use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
struct Arguments {
    #[arg(long)]
    repo: PathBuf,
    #[arg(long)]
    state_dir: PathBuf,
    #[command(subcommand)]
    command: Entry,
}
#[derive(Subcommand)]
enum Entry {
    #[command(name = "__verify")]
    Supervise { run_id: RunId },
    #[command(name = "__execute")]
    Execute { run_id: RunId },
}
#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), String> {
    let args = Arguments::parse();
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    let context = runs::Context::new(&args.repo, Some(&args.state_dir), &executable)?;
    match args.command {
        Entry::Supervise { run_id } => runs::supervise(&context, &run_id).await,
        Entry::Execute { run_id } => runs::execute(&context, &run_id).await,
    }
}
