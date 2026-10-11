use artifactize_tools::cli::{Cli, run};
use clap::Parser;

#[tokio::main(flavor = "current_thread")]
async fn main() -> std::process::ExitCode {
    std::process::ExitCode::from(run(Cli::parse().command).await)
}
