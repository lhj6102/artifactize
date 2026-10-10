use std::{collections::BTreeMap, ffi::OsStr, io::Write, path::PathBuf};

use artifactize_tools::{
    Builtin, Content, builtin,
    scope::{Artifact, ArtifactId, ArtifactKind, Scope},
};
use clap::{Parser, Subcommand};
use serde_json::json;
use tokio_util::sync::CancellationToken;

#[derive(Parser)]
#[command(
    disable_help_subcommand = true,
    version,
    about = "Portable scoped tools for artifactize",
    long_about = "Portable scoped tools for artifactize. Filesystem targets must be relative to the current directory and stay below it. Parent components, absolute paths, and symlinks are refused. Use '.' for the current directory. Only open accepts HTTP(S) URLs. help resolves a literal program through the system's program search path."
)]
struct Cli {
    #[command(subcommand)]
    command: Tool,
}

#[derive(Subcommand)]
enum Tool {
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

#[tokio::main(flavor = "current_thread")]
async fn main() -> std::process::ExitCode {
    match run(Cli::parse()).await {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            let _ = writeln!(std::io::stderr(), "{error}");
            std::process::ExitCode::FAILURE
        }
    }
}

fn logical(path: String) -> String {
    if path == "." { String::new() } else { path }
}

async fn run(cli: Cli) -> Result<(), String> {
    let root = artifactize_tools::files::canonicalize(
        &std::env::current_dir().map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    let owner = ArtifactId::new("cwd").expect("static identifier");
    let scope = Scope {
        artifacts: BTreeMap::from([(
            owner.clone(),
            Artifact {
                path: PathBuf::new(),
                kind: ArtifactKind::Folder,
                name: "cwd".into(),
                children: BTreeMap::new(),
                mounts: BTreeMap::new(),
            },
        )]),
    };
    let cancellation = CancellationToken::new();
    let (tool, args, input) = match cli.command {
        Tool::Read { path } => (Builtin::Read, Some(vec![logical(path)]), json!({})),
        Tool::List { path } => (Builtin::List, Some(vec![logical(path)]), json!({})),
        Tool::Section { path, heading } => (
            Builtin::Section,
            Some(vec![logical(path), heading]),
            json!({}),
        ),
        Tool::Help { args } => (Builtin::Help, Some(args), json!({})),
        Tool::Glob { pattern, path } => (
            Builtin::Glob,
            None,
            json!({"pattern":pattern,"path":logical(path)}),
        ),
        Tool::Grep {
            pattern,
            path,
            glob,
            case_insensitive,
        } => {
            let mut input =
                json!({"pattern":pattern,"path":logical(path),"caseInsensitive":case_insensitive});
            if let Some(glob) = glob {
                input["glob"] = json!(glob);
            }
            (Builtin::Grep, None, input)
        }
        Tool::Open { target } => {
            let target = if builtin::is_url(&target) {
                target
            } else {
                scope
                    .resolve_input(&root, &owner, &logical(target))
                    .map_err(|error| error.to_string())?
                    .to_str()
                    .ok_or("Paths must be UTF-8.")?
                    .to_owned()
            };
            return artifactize_tools::opener::open(OsStr::new(&target))
                .await
                .map_err(|error| error.to_string());
        }
    };
    let result = if let Some(args) = args {
        let context = builtin::Context {
            root: &root,
            scope: &scope,
            owner: &owner,
            launcher: &artifactize_tools::launch::Standalone,
        };
        builtin::call_fixed(tool, &args, input, context, &cancellation).await
    } else {
        let validator = artifactize_tools::schema::compile(&builtin::input_schema(tool))?;
        artifactize_tools::schema::validate(&validator, &input)?;
        builtin::call(
            builtin::Input::parse(tool, input)?,
            &root,
            &scope,
            &owner,
            &cancellation,
        )
    };
    let (mut stdout, mut stderr) = (std::io::stdout().lock(), std::io::stderr().lock());
    for content in result.content {
        match content {
            Content::Text { text } if result.is_error => writeln!(stderr, "{text}"),
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
    if result.is_error {
        Err("Tool failed.".into())
    } else {
        Ok(())
    }
}
