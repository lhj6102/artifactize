use artifactize_tools::{
    cli::{Cli, run},
    exit_code,
};
use clap::Parser;

#[tokio::main(flavor = "current_thread")]
async fn main() -> std::process::ExitCode {
    exit_code(run(Cli::parse().command).await)
}
