//! What a declared Human tool runs, read from the saved Human definition for display.
//!
//! A command tool keeps its shell-quoted declaration. A builtin tool names its action and
//! target in words, the target as a repository-relative logical path with `/`, so a reviewer
//! sees what Enter does without reading the declaration.

use crate::{
    store::definitions::{Artifact, HumanTool},
    types::ArtifactName,
};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use crate::config::Builtin;

/// The repository root shown as a target, when an Artifact sits at the root.
const ROOT: &str = ".";

/// What a tool runs, placeholders resolved to logical paths where the definition allows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Runs {
    /// The declared command and args, shell-quoted, placeholders unresolved.
    Command(String),
    /// Open a file or folder in the desktop's default application.
    Open(PathBuf),
    /// Open an HTTP(S) URL in the browser.
    OpenUrl(String),
    /// Print a UTF-8 file.
    Read(PathBuf),
    /// List a folder.
    List(PathBuf),
    /// Print one section of a Markdown file.
    Section { file: PathBuf, heading: String },
    /// Print `<program> <subcommand...> --help`.
    Help(String),
}

impl Runs {
    /// The declaration of one Human tool of `owner`, within the definition's Artifacts.
    pub(super) fn parse(
        artifacts: &BTreeMap<ArtifactName, Artifact>,
        owner: &ArtifactName,
        tool: &HumanTool,
    ) -> Self {
        let (builtin, command, args) = match tool {
            HumanTool::Command(tool) => (None, tool.command.as_str(), tool.args.as_slice()),
            HumanTool::Builtin(tool) => (Some(tool.builtin), "", tool.args.as_slice()),
        };
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        let place = |id: &str| {
            let artifact = artifacts.get(id)?;
            let path = artifact.path.value()?;
            Some(match artifact.kind.value() {
                Some(crate::config::ArtifactKind::File) => Place {
                    path: path.clone(),
                    file: true,
                },
                // An aspect's path is its declaration's label in its folder.
                Some(crate::config::ArtifactKind::Aspect) => Place {
                    path: path.clone(),
                    file: false,
                },
                _ => Place {
                    path: path.clone(),
                    file: false,
                },
            })
        };
        Self::of(builtin, command, &args, &place, owner)
    }

    /// The same of a Human tool of `owner` in declarations read now, within `artifacts`.
    pub(super) fn declared(
        artifacts: &BTreeMap<ArtifactName, crate::config::Artifact>,
        owner: &ArtifactName,
        tool: &crate::config::HumanTool,
    ) -> Self {
        use crate::config::HumanTool;
        let (builtin, command, args) = match tool {
            HumanTool::Command(tool) => (None, tool.command.as_str(), tool.args.as_slice()),
            HumanTool::Builtin(tool) => (Some(tool.builtin), "", tool.args.as_slice()),
        };
        let args: Vec<String> = args.iter().map(ToString::to_string).collect();
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        let place = |id: &str| {
            let artifact = artifacts.get(id)?;
            Some(Place {
                path: artifact.path.clone(),
                file: artifact.kind() == crate::config::ArtifactKind::File,
            })
        };
        Self::of(builtin, command, &args, &place, owner)
    }

    fn of(
        builtin: Option<Builtin>,
        command: &str,
        args: &[&str],
        place: &dyn Fn(&str) -> Option<Place>,
        owner: &ArtifactName,
    ) -> Self {
        let target = |arg| logical(place, owner, arg);
        match (builtin, args) {
            (Some(Builtin::Open), [url]) if artifactize_tools::builtin::is_url(url) => {
                Self::OpenUrl((*url).to_owned())
            }
            (Some(Builtin::Open), [arg]) => match target(arg) {
                Target::Live(url) if artifactize_tools::builtin::is_url(&url) => Self::OpenUrl(url),
                target => Self::Open(target.into_path()),
            },
            (Some(Builtin::Read), [arg]) => Self::Read(target(arg).into_path()),
            (Some(Builtin::List), [arg]) => Self::List(target(arg).into_path()),
            (Some(Builtin::Section), [arg, heading]) => Self::Section {
                file: target(arg).into_path(),
                heading: (*heading).to_owned(),
            },
            (Some(Builtin::Help), [program, subcommands @ ..]) => {
                let program = crate::platform::program_text(program);
                Self::Help(super::shell(
                    std::iter::once(program.as_str()).chain(subcommands.iter().copied()),
                ))
            }
            _ => {
                let program = builtin
                    .map(|builtin| {
                        match builtin {
                            Builtin::Read => "read",
                            Builtin::List => "list",
                            Builtin::Glob => "glob",
                            Builtin::Grep => "grep",
                            Builtin::ViewImage => "view_image",
                            Builtin::Section => "section",
                            Builtin::Help => "help",
                            Builtin::Open => "open",
                            Builtin::Graph => "graph",
                        }
                        .to_owned()
                    })
                    .unwrap_or_else(|| crate::platform::program_text(command));
                Self::Command(super::shell(
                    std::iter::once(program.as_str()).chain(args.iter().copied()),
                ))
            }
        }
    }

    /// The action in one line: `$ command args`, or a builtin's verb and target.
    pub fn line(&self) -> String {
        match self {
            Self::Command(command) => format!("$ {command}"),
            Self::Open(path) => format!(
                "opens {} in its default app",
                crate::platform::path_text(path)
            ),
            Self::OpenUrl(url) => format!("opens {url} in the browser"),
            Self::Read(path) => format!("prints {}", crate::platform::path_text(path)),
            Self::List(path) => format!("lists {}", folder(&crate::platform::path_text(path))),
            Self::Section { file, heading } => format!(
                "prints section \"{heading}\" of {}",
                crate::platform::path_text(file)
            ),
            Self::Help(program) => format!("prints {program} --help"),
        }
    }
}

/// Where a target's Artifact is, as a definition knows it.
struct Place {
    path: crate::config::Target,
    /// A file Artifact: a path below it is in its folder.
    file: bool,
}

/// A folder target ends in `/`, so it reads as a folder.
fn folder(path: &str) -> String {
    if path.ends_with('/') {
        path.to_owned()
    } else {
        format!("{path}/")
    }
}

enum Target {
    Path(PathBuf),
    Live(String),
}

impl Target {
    fn into_path(self) -> PathBuf {
        match self {
            Self::Path(path) => path,
            Self::Live(address) => PathBuf::from(address),
        }
    }
}

/// A builtin target as a repository-relative path, or an opaque Live address.
fn logical(place: &dyn Fn(&str) -> Option<Place>, owner: &ArtifactName, arg: &str) -> Target {
    let (id, rest) = match arg.strip_prefix('{').and_then(|arg| arg.split_once('}')) {
        Some(("artifactPath", rest)) => (owner.as_str(), rest),
        Some((name, rest)) => (name, rest),
        None => (owner.as_str(), arg),
    };
    let rest = rest.strip_prefix('/').unwrap_or(rest);
    let Some(artifact) = place(id) else {
        return Target::Path(PathBuf::from(arg));
    };
    if let Some(uri) = artifact.path.uri() {
        return Target::Live(if rest.is_empty() {
            uri.to_string()
        } else {
            format!("{uri}/{rest}")
        });
    }
    let path = artifact
        .path
        .local()
        .expect("a non-Live target has a local path");
    if rest.is_empty() {
        return Target::Path(if path.as_os_str().is_empty() {
            PathBuf::from(ROOT)
        } else {
            path.to_owned()
        });
    }
    let base = if artifact.file {
        path.parent().unwrap_or(Path::new(""))
    } else {
        path
    };
    Target::Path(base.join(rest))
}
