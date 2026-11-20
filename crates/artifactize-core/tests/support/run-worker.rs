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
    let executable =
        artifactize_core::platform::current_executable().map_err(|error| error.to_string())?;
    let context = runs::Context::new(&args.repo, Some(&args.state_dir), &executable)?;
    match args.command {
        Entry::Supervise { run_id } => {
            let gate = args.state_dir.join("fixture-admission");
            if gate.is_dir() {
                std::fs::write(gate.join("held"), "").map_err(|e| e.to_string())?;
                // Poll the explicit fixture signal. The timeout only bounds a broken fixture.
                const WATCHDOG: std::time::Duration = std::time::Duration::from_secs(60);
                const POLL: std::time::Duration = std::time::Duration::from_millis(10);
                tokio::time::timeout(WATCHDOG, async {
                    while !gate.join("release").is_file() {
                        tokio::time::sleep(POLL).await;
                    }
                })
                .await
                .map_err(|_| "The fixture did not release worker admission.".to_owned())?;
            }
            let directory = args.state_dir.join("runs").join(run_id.as_str());
            runs::supervise_observing_launch(&context, &run_id, || {
                let lock = std::fs::OpenOptions::new()
                    .read(true)
                    .write(true)
                    .create(true)
                    .truncate(false)
                    .open(directory.join("fixture-launch.lock"))
                    .map_err(|error| error.to_string())?;
                lock.lock().map_err(|error| error.to_string())?;
                Ok(move |owner| {
                    let _held = lock;
                    let bytes = serde_json::to_vec(&owner).map_err(|error| error.to_string())?;
                    std::fs::write(directory.join("fixture-executor.json"), bytes)
                        .map_err(|error| error.to_string())
                })
            })
            .await
        }
        Entry::Execute { run_id } => runs::execute(&context, &run_id).await,
    }
}
