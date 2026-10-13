//! What the app was started for: the CLI's `monitor` and `review` options.
use crate::{App, Failure};
use serde::Serialize;
use specta::Type;
use std::{ffi::OsString, path::PathBuf};

/// The screen the app opens on; both stay reachable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum Mode {
    Monitor,
    Review,
}

#[derive(Debug, Clone)]
pub struct Launch {
    pub mode: Mode,
    pub state: PathBuf,
    /// `--repo` when given; `None` (the default, or `--all`) is every repository.
    pub repo: Option<PathBuf>,
    pub reviewer: artifactize_core::types::ReviewerId,
    /// `review REQUEST_ID`: the request to open first.
    pub request: Option<artifactize_core::types::RequestId>,
    /// The review list's first search: the waiting Human reviews, of `--repo` when given.
    pub search: String,
}

/// The options as `artifactize monitor` and `artifactize review` take them, with their typed
/// values; the reviewer may follow either screen.
#[derive(clap::Parser)]
#[command(name = "artifactize-desktop", about = "The artifactize desktop app.")]
struct Args {
    #[command(subcommand)]
    screen: Option<Screen>,
    /// Repository input path.
    #[arg(long, global = true, value_name = "PATH", conflicts_with = "all")]
    repo: Option<PathBuf>,
    /// Review every repository.
    #[arg(long, global = true)]
    all: bool,
    /// State database and Run output directory.
    #[arg(long, global = true, value_name = "PATH")]
    state_dir: Option<PathBuf>,
    /// Reviewer name (defaults to USER).
    #[arg(long, global = true, value_name = "NAME")]
    reviewer: Option<artifactize_core::types::ReviewerId>,
}

#[derive(clap::Subcommand)]
enum Screen {
    /// Open on the Monitor.
    Monitor,
    /// Open on Review.
    Review {
        /// Open this request instead of the list.
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
        };
        let reviewer = match reviewer {
            Some(reviewer) => reviewer,
            None => artifactize_core::human::default_reviewer()?,
        };
        let state = artifactize_core::store::state_dir(state.as_deref())?;
        // Only an explicit --repo narrows the monitor's scope and the review search.
        let search = artifactize_core::review::search::Query::text_for(
            artifactize_core::monitor::initial(repo.clone()).as_deref(),
        );
        Ok(Self {
            mode,
            state,
            search,
            repo,
            reviewer,
            request,
        })
    }

    /// The path that picks the first scope, canonical like the terminal monitor's.
    pub fn initial(&self) -> Option<PathBuf> {
        artifactize_core::monitor::initial(self.repo.clone())
    }
}

/// What the frontend needs to know about this launch.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct LaunchView {
    pub mode: Mode,
    pub state: String,
    pub reviewer: String,
    /// The review list's first search.
    pub search: String,
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
        search: launch.search.clone(),
        request: launch.request.clone().map(Into::into),
    })
}
