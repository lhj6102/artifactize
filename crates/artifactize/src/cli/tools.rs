//! CLI argument parsing for the shared tool diagnostic operation.

use artifactize_core::diagnostics::ToolArguments;
use clap::Subcommand;

#[derive(Debug, Default, clap::Args)]
pub struct ToolCheckArgs {
    /// Select one Agent or Human eval's admitted scope (same as --eval).
    #[arg(conflicts_with = "eval")]
    pub selector: Option<artifactize_core::types::EvalId>,
    /// Select one Agent or Human eval's admitted scope.
    #[arg(long, value_name = "ID")]
    pub eval: Option<artifactize_core::types::EvalId>,
    /// Check tools declared for one Artifact.
    #[arg(long, value_name = "ID")]
    pub artifact: Option<artifactize_core::types::ArtifactName>,
    /// Restrict --artifact to Agent or Human tools.
    #[arg(long, value_enum)]
    pub audience: Option<artifactize_core::diagnostics::Audience>,
    /// Check one tool by short or published name.
    #[arg(long, value_name = "NAME")]
    pub tool: Option<artifactize_core::config::ToolName>,
    /// Invoke exactly one explicitly selected tool; never create review evidence.
    #[arg(long)]
    pub execute: bool,
    /// JSON arguments for an executed Agent tool.
    #[arg(long, value_name = "JSON")]
    pub args: Option<ToolArguments>,
}

impl TryFrom<ToolCheckArgs> for artifactize_core::diagnostics::ToolCheckOptions {
    type Error = String;

    fn try_from(args: ToolCheckArgs) -> Result<Self, Self::Error> {
        Self::from_flags(
            args.eval.or(args.selector),
            args.artifact,
            args.audience,
            args.tool,
            args.execute,
            args.args,
        )
    }
}

#[derive(Debug, Subcommand)]
pub enum ToolsCommand {
    /// Static preflight by default; --execute opts in to one tool call.
    Check(ToolCheckArgs),
    #[command(flatten)]
    Builtin(artifactize_tools::cli::Tool),
}
