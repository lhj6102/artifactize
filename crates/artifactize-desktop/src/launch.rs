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
    /// `--repo` or the current directory; `None` with `--all`.
    pub repo: Option<PathBuf>,
    pub reviewer: artifactize::types::ReviewerId,
    /// `review REQUEST_ID`: the request to open first.
    pub request: Option<artifactize::types::RequestId>,
    /// The review list's first search: the waiting Human reviews, of `--repo` when given.
    pub search: String,
}

const USAGE: &str = "usage: artifactize-desktop [monitor | review [REQUEST_ID]] [--repo PATH | --all] [--state-dir PATH] [--reviewer NAME]";

impl Launch {
    /// Parse `args` (with the program name first) the way `artifactize monitor` and
    /// `artifactize review` take their options.
    pub fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Self, String> {
        let mut args = args.into_iter().skip(1);
        let mut mode = Mode::Monitor;
        let (mut repo, mut all, mut state, mut reviewer, mut request) =
            (None, false, None, None, None);
        let text = |value: Option<OsString>, name: &str| {
            value
                .and_then(|value| value.into_string().ok())
                .ok_or_else(|| format!("{name} needs a value.\n{USAGE}"))
        };
        while let Some(arg) = args.next() {
            match arg.to_str() {
                Some("monitor") => mode = Mode::Monitor,
                Some("review") => mode = Mode::Review,
                Some("--repo") => {
                    repo = Some(PathBuf::from(
                        args.next()
                            .ok_or(format!("--repo needs a value.\n{USAGE}"))?,
                    ))
                }
                Some("--all") => all = true,
                Some("--state-dir") => {
                    state = Some(PathBuf::from(
                        args.next()
                            .ok_or(format!("--state-dir needs a value.\n{USAGE}"))?,
                    ))
                }
                Some("--reviewer") => reviewer = Some(text(args.next(), "--reviewer")?.parse()?),
                Some(id) if mode == Mode::Review && request.is_none() && !id.starts_with('-') => {
                    request = Some(id.parse()?)
                }
                _ => {
                    return Err(format!(
                        "unexpected argument {}\n{USAGE}",
                        arg.to_string_lossy()
                    ));
                }
            }
        }
        if all && repo.is_some() {
            return Err("accepts --repo or --all, not both.".into());
        }
        let reviewer = match reviewer {
            Some(reviewer) => reviewer,
            None => artifactize::human::default_reviewer()?,
        };
        let state = artifactize::store::state_dir(state.as_deref())?;
        // Only an explicit --repo narrows the review search; the monitor still starts on cwd.
        let search = artifactize::review::search::Query::text_for(
            artifactize::monitor::initial(repo.clone()).as_deref(),
        );
        Ok(Self {
            mode,
            state,
            search,
            repo: (!all).then(|| repo.unwrap_or_else(|| PathBuf::from("."))),
            reviewer,
            request,
        })
    }

    /// The path that picks the first scope, canonical like the terminal monitor's.
    pub fn initial(&self) -> Option<PathBuf> {
        artifactize::monitor::initial(self.repo.clone())
    }

    /// The window title: the app and the repository it was started for.
    pub fn title(&self) -> String {
        match self.initial() {
            Some(repo) => format!(
                "Artifactize — {}",
                artifactize_tools::files::path_text(&repo)
            ),
            None => "Artifactize — all repositories".into(),
        }
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
    pub request: Option<String>,
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
        request: launch.request.as_ref().map(ToString::to_string),
    })
}
