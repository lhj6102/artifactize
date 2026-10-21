//! What the app was started for: the CLI's `monitor` and `review` options, and `graph`.
use crate::{App, Failure};
use serde::Serialize;
use specta::Type;
use std::{ffi::OsString, path::PathBuf};

/// What the Main area opens on; everything else stays reachable from the Pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum Mode {
    /// The branch of `--repo` with its last chip, or nothing chosen.
    Monitor,
    /// A waiting Human review: the one named, else the newest.
    Review,
    /// The Current of the branch of `--repo`.
    Graph,
}

#[derive(Debug, Clone)]
pub struct Launch {
    pub mode: Mode,
    pub state: PathBuf,
    /// `--repo` when given, canonical: its branch is selected and listed even before its first
    /// Run.
    pub repo: Option<PathBuf>,
    pub reviewer: artifactize_core::types::ReviewerId,
    /// `review REQUEST_ID`: the request to open first.
    pub request: Option<artifactize_core::types::RequestId>,
}

/// The options as `artifactize monitor` and `artifactize review` take them, with their typed
/// values; the reviewer may follow either.
#[derive(clap::Parser)]
#[command(name = "artifactize-desktop", about = "The artifactize desktop app.")]
struct Args {
    #[command(subcommand)]
    screen: Option<Screen>,
    /// Select the branch of this folder.
    #[arg(long, global = true, value_name = "PATH", conflicts_with = "all")]
    repo: Option<PathBuf>,
    /// Select no branch: every branch is listed either way.
    #[arg(long, global = true)]
    all: bool,
    /// State database and Run output directory.
    #[arg(long, global = true, value_name = "PATH")]
    state_dir: Option<PathBuf>,
    /// Reviewer name (defaults to the signed-in user).
    #[arg(long, global = true, value_name = "NAME")]
    reviewer: Option<artifactize_core::types::ReviewerId>,
}

#[derive(clap::Subcommand)]
enum Screen {
    /// Open on the branches.
    Monitor,
    /// Open on the current Artifact graph of the branch of --repo (not a saved Run).
    Graph,
    /// Open a waiting Human review.
    Review {
        /// Open this request instead of the newest waiting one.
        #[arg(value_name = "REQUEST_ID")]
        request: Option<artifactize_core::types::RequestId>,
    },
}

impl Launch {
    /// Parse `args` (with the program name first).
    pub fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Self, String> {
        use clap::Parser;
        let Args {
            screen,
            repo,
            all: _,
            state_dir: state,
            reviewer,
        } = Args::try_parse_from(args).map_err(|error| error.to_string())?;
        let (mode, request) = match screen {
            None | Some(Screen::Monitor) => (Mode::Monitor, None),
            Some(Screen::Review { request }) => (Mode::Review, request),
            Some(Screen::Graph) if repo.is_some() => (Mode::Graph, None),
            Some(Screen::Graph) => return Err("graph requires --repo PATH.".into()),
        };
        let reviewer = match reviewer {
            Some(reviewer) => reviewer,
            None => artifactize_core::human::default_reviewer()?,
        };
        let state = artifactize_core::store::state_dir(state.as_deref())?;
        Ok(Self {
            mode,
            state,
            repo: artifactize_core::monitor::initial(repo),
            reviewer,
            request,
        })
    }
}

/// What the frontend needs to know about this launch.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct LaunchView {
    pub mode: Mode,
    pub state: String,
    pub reviewer: String,
    pub request: Option<crate::ids::RequestKey>,
}

#[tauri::command]
#[specta::specta]
pub fn launch(app: tauri::State<'_, App>) -> Result<LaunchView, Failure> {
    let launch = &app.launch;
    Ok(LaunchView {
        mode: launch.mode,
        state: artifactize_tools::files::path_text(&launch.state),
        reviewer: launch.reviewer.to_string(),
        request: launch.request.clone().map(Into::into),
    })
}
