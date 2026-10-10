//! What a declared Human tool runs, read from the saved Human definition for display.
//!
//! A command tool keeps its shell-quoted declaration. A builtin tool names its action and
//! target in words, the target as a repository-relative logical path with `/`, so a reviewer
//! sees what Enter does without reading the declaration.

use serde_json::{Map, Value};

use crate::config::Builtin;

/// The repository root shown as a target, when an Artifact sits at the root.
const ROOT: &str = ".";

/// What a tool runs, placeholders resolved to logical paths where the definition allows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Runs {
    /// The declared command and args, shell-quoted, placeholders unresolved.
    Command(String),
    /// Open a file or folder in the desktop's default application.
    Open(String),
    /// Open an HTTP(S) URL in the browser.
    OpenUrl(String),
    /// Print a UTF-8 file.
    Read(String),
    /// List a folder.
    List(String),
    /// Print one section of a Markdown file.
    Section { file: String, heading: String },
    /// Print `<program> <subcommand...> --help`.
    Help(String),
}

impl Runs {
    /// The declaration of one Human tool of `owner`, within the definition's Artifacts.
    pub(super) fn parse(artifacts: &Map<String, Value>, owner: &str, tool: &Value) -> Self {
        let args: Vec<&str> = tool["args"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect();
        let builtin = serde_json::from_value::<Builtin>(tool["builtin"].clone()).ok();
        let target = || {
            args.first()
                .map(|arg| logical(artifacts, owner, arg))
                .unwrap_or_default()
        };
        match (builtin, args.as_slice()) {
            (Some(Builtin::Open), [url]) if artifactize_tools::builtin::is_url(url) => {
                Self::OpenUrl((*url).to_owned())
            }
            (Some(Builtin::Open), [_]) => Self::Open(target()),
            (Some(Builtin::Read), [_]) => Self::Read(target()),
            (Some(Builtin::List), [_]) => Self::List(target()),
            (Some(Builtin::Section), [_, heading]) => Self::Section {
                file: target(),
                heading: (*heading).to_owned(),
            },
            (Some(Builtin::Help), [_, ..]) => Self::Help(super::shell(args.iter().copied())),
            // Any other declaration is shown as recorded.
            _ => Self::Command(super::shell(
                std::iter::once(tool.get("builtin").unwrap_or(&tool["command"]))
                    .filter_map(Value::as_str)
                    .chain(args.iter().copied()),
            )),
        }
    }

    /// The action in one line: `$ command args`, or a builtin's verb and target.
    pub fn line(&self) -> String {
        match self {
            Self::Command(command) => format!("$ {command}"),
            Self::Open(path) => format!("opens {path} in its default app"),
            Self::OpenUrl(url) => format!("opens {url} in the browser"),
            Self::Read(path) => format!("prints {path}"),
            Self::List(path) => format!("lists {}", folder(path)),
            Self::Section { file, heading } => format!("prints section \"{heading}\" of {file}"),
            Self::Help(program) => format!("prints {program} --help"),
        }
    }
}

/// A folder target ends in `/`, so it reads as a folder.
fn folder(path: &str) -> String {
    if path.ends_with('/') {
        path.to_owned()
    } else {
        format!("{path}/")
    }
}

/// A builtin target as a repository-relative logical path: `{artifactPath}[/path]`,
/// `{name}[/path]` through the owner's mounts, or a path in the owner that may enter a mount.
/// A target the definition cannot place is shown as declared.
fn logical(artifacts: &Map<String, Value>, owner: &str, arg: &str) -> String {
    let mount = |id: &str, name: &str| {
        artifacts
            .get(id)
            .and_then(|artifact| artifact["mounts"][name].as_str())
            .map(str::to_owned)
    };
    let (mut id, rest) = match arg.strip_prefix('{').and_then(|arg| arg.split_once('}')) {
        Some(("artifactPath", rest)) => (owner.to_owned(), rest),
        Some((name, rest)) => (mount(owner, name).unwrap_or_else(|| name.to_owned()), rest),
        None => (owner.to_owned(), arg),
    };
    let mut rest = rest.strip_prefix('/').unwrap_or(rest);
    // A first segment that names a mount continues in the mounted Artifact, as in the scope.
    loop {
        let (first, after) = rest.split_once('/').unwrap_or((rest, ""));
        match mount(&id, first) {
            Some(mounted) if !first.is_empty() => (id, rest) = (mounted, after),
            _ => break,
        }
    }
    let Some(artifact) = artifacts.get(&id) else {
        return arg.to_owned();
    };
    let Some(path) = artifact["path"].as_str() else {
        return arg.to_owned();
    };
    let base = if rest.is_empty() || artifact["kind"] != "file" {
        path
    } else {
        path.rsplit_once('/').map_or("", |(parent, _)| parent)
    };
    match (base, rest) {
        ("", "") => ROOT.to_owned(),
        ("", rest) => rest.to_owned(),
        (base, "") => base.to_owned(),
        (base, rest) => format!("{base}/{rest}"),
    }
}
