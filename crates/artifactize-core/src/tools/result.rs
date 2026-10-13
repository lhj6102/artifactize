use crate::process::Output;

pub(super) use artifactize_tools::result::parse;

pub(super) fn plain(output: &Output) -> super::ToolResult {
    artifactize_tools::result::plain(&output.stdout, output.status.success(), output.truncated)
}
