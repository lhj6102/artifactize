use clap::Parser;

#[derive(Parser)]
#[command(version, about = "Portable scoped tools for artifactize")]
struct Cli {}

fn main() {
    Cli::parse();
}
