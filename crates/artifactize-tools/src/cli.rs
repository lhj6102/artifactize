//! Shared command-line definition and current-directory tool runner.

use std::{
    collections::BTreeMap,
    ffi::OsString,
    io::{self, Write},
    path::{Path, PathBuf},
};

use crate::{
    Content, ToolResult,
    builtin::{self, fixed::FixedInput},
    scope::{Artifact, ArtifactId, Scope},
};
use clap::{Parser, Subcommand};
use tokio_util::sync::CancellationToken;

#[derive(Parser)]
#[command(
    disable_help_subcommand = true,
    version,
    about = "Portable scoped tools for artifactize",
    long_about = "Portable scoped tools for artifactize. Filesystem targets must be relative to the current directory and stay below it. Parent components, absolute paths, and symlinks are refused. Use '.' for the current directory. Only open accepts HTTP(S) URLs. help resolves a literal program through the system's program search path."
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Tool,
}

#[derive(Debug, Subcommand)]
pub enum Tool {
    /// Print a UTF-8 file (at most 64 KiB).
    Read { path: String },
    /// List a directory, as JSON.
    List {
        #[arg(default_value = ".")]
        path: String,
    },
    /// Find logical file paths, as JSON.
    Glob {
        pattern: String,
        #[arg(default_value = ".")]
        path: String,
    },
    /// Search UTF-8 files, as JSON.
    Grep {
        pattern: String,
        #[arg(default_value = ".")]
        path: String,
        #[arg(long)]
        glob: Option<String>,
        #[arg(long)]
        case_insensitive: bool,
    },
    /// Print a Markdown section, selected by its exact heading.
    Section { path: String, heading: String },
    /// Run PROGRAM SUBCOMMAND... --help (10 seconds, 64 KiB).
    Help {
        #[arg(required = true, num_args = 1.., trailing_var_arg = true)]
        args: Vec<String>,
    },
    /// Open one scoped path or HTTP(S) URL in its default application.
    Open { target: String },
}

/// Execute a parsed standalone tool with the current directory as its scope.
/// The facade and standalone binary share output and exit codes, not just parsing.
pub async fn run(tool: Tool) -> u8 {
    match execute(tool).await {
        Ok(()) => 0,
        Err(error) => {
            let _ = writeln!(std::io::stderr(), "{error}");
            1
        }
    }
}

fn logical(path: String) -> String {
    if path == "." { String::new() } else { path }
}

async fn execute(tool: Tool) -> Result<(), String> {
    let root = crate::files::current_directory().map_err(|error| error.to_string())?;
    execute_in(tool, &root, async |target| {
        crate::opener::open(&target).await
    })
    .await
}

async fn execute_in(
    tool: Tool,
    root: &Path,
    open: impl AsyncFnOnce(OsString) -> io::Result<()>,
) -> Result<(), String> {
    let owner = ArtifactId::new("cwd").expect("static identifier");
    let scope = Scope {
        artifacts: BTreeMap::from([(
            owner.clone(),
            Artifact {
                path: PathBuf::new()
                    .try_into()
                    .expect("workspace root is relative"),
                name: "cwd".into(),
                covering_part: None,
                nested: BTreeMap::new(),
            },
        )]),
    };
    let cancellation = CancellationToken::new();
    enum Invocation {
        Fixed(Result<FixedInput, String>),
        Dynamic(builtin::Input),
    }
    let invocation = match tool {
        Tool::Read { path } => Invocation::Fixed(FixedInput::read(logical(path))),
        Tool::List { path } => Invocation::Fixed(FixedInput::list(logical(path))),
        Tool::Section { path, heading } => {
            Invocation::Fixed(FixedInput::section(logical(path), heading))
        }
        Tool::Help { args } => Invocation::Fixed(FixedInput::help(&args)),
        Tool::Glob { pattern, path } => {
            Invocation::Dynamic(builtin::Input::glob(&pattern, logical(path))?)
        }
        Tool::Grep {
            pattern,
            path,
            glob,
            case_insensitive,
        } => Invocation::Dynamic(builtin::Input::grep(
            &pattern,
            logical(path),
            glob.as_deref(),
            case_insensitive,
        )?),
        Tool::Open { target } => {
            let target = if builtin::is_url(&target) {
                target.into()
            } else {
                scope
                    .resolve_input(root, &owner, &logical(target))
                    .map_err(|error| error.to_string())?
                    .into_os_string()
            };
            return open(target).await.map_err(|error| error.to_string());
        }
    };
    let result = match invocation {
        Invocation::Fixed(Err(error)) => ToolResult::error(error),
        Invocation::Fixed(Ok(input)) => {
            builtin::fixed::execute(
                input,
                builtin::Context {
                    root,
                    scope: &scope,
                    owner: &owner,
                    launcher: &crate::launch::Standalone,
                },
                &cancellation,
            )
            .await
        }
        Invocation::Dynamic(input) => builtin::call(input, root, &scope, &owner, &cancellation),
    };
    let (mut stdout, mut stderr) = (std::io::stdout().lock(), std::io::stderr().lock());
    for content in result.content() {
        match content {
            Content::Text { text } if result.is_error() => writeln!(stderr, "{text}"),
            Content::Text { text } => write!(stdout, "{text}"),
            Content::Json { data } => writeln!(
                stdout,
                "{}",
                serde_json::to_string_pretty(&data).expect("builtin JSON")
            ),
            Content::Image { .. } => unreachable!("CLI tools produce no images"),
        }
        .map_err(|error| error.to_string())?;
    }
    if result.is_error() {
        Err("Tool failed.".into())
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn scoped_cli_open_records_exactly_one_absolute_target() {
        let root = crate::test_os::tempdir();
        let root = crate::files::canonicalize(root.path()).unwrap();
        let file = crate::test_os::opener_input(&root);
        let cli = Cli::try_parse_from([
            "artifactize-tools",
            "open",
            file.file_name().unwrap().to_str().unwrap(),
        ])
        .unwrap();
        let mut targets = Vec::new();
        // Both platforms execute the real CLI resolution; the native handoff alone is replaced.
        execute_in(cli.command, &root, async |target| {
            targets.push(PathBuf::from(target));
            Ok(())
        })
        .await
        .unwrap();
        assert!(file.is_absolute());
        assert_eq!(targets, [file]);
    }
}
