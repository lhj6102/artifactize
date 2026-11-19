//! Desktop launch arguments and validation for the CLI shortcuts.

use artifactize_core::types::{RequestId, ReviewerId};
use std::{ffi::OsString, path::PathBuf};

pub(super) enum Scope {
    All,
    Repository(PathBuf),
}

impl Scope {
    pub(super) fn from_args(
        all: bool,
        path: Option<PathBuf>,
        repo: Option<PathBuf>,
    ) -> Result<Self, String> {
        match (all, path, repo) {
            (_, Some(_), Some(_)) => Err("monitor accepts PATH or --repo, not both.".into()),
            (true, Some(_), _) | (true, _, Some(_)) => {
                Err("monitor accepts --repo or --all, not both.".into())
            }
            (false, Some(path), None) | (false, None, Some(path)) => Ok(Self::Repository(path)),
            (_, None, None) => Ok(Self::All),
        }
    }

    pub(super) fn for_review(all: bool, repo: Option<PathBuf>) -> Result<Self, String> {
        match (all, repo) {
            (true, Some(_)) => Err("review accepts --repo or --all, not both.".into()),
            (false, Some(path)) => Ok(Self::Repository(path)),
            (_, None) => Ok(Self::All),
        }
    }

    fn repository(self) -> Option<PathBuf> {
        match self {
            Self::All => None,
            Self::Repository(path) => Some(path),
        }
    }
}

pub(super) fn monitor(state_dir: Option<PathBuf>, scope: Scope, json: bool) -> Result<u8, String> {
    launch(args("monitor", state_dir, scope, None, None), json)
}

pub(super) async fn review(
    state_dir: Option<PathBuf>,
    scope: Scope,
    request: Option<RequestId>,
    reviewer: Option<ReviewerId>,
    json: bool,
) -> Result<u8, String> {
    if !json && let Some(id) = &request {
        let state = artifactize_core::store::state_dir(state_dir.as_deref())?;
        artifactize_core::store::read_request(&state, id).await?;
    }
    launch(args("review", state_dir, scope, request, reviewer), json)
}

fn launch(args: Vec<OsString>, json: bool) -> Result<u8, String> {
    if json {
        return Err("Desktop launch shortcuts do not support --json.".into());
    }
    artifactize_core::platform::launch_desktop(&args)?;
    Ok(0)
}

pub(super) fn args(
    command: &str,
    state_dir: Option<PathBuf>,
    scope: Scope,
    request: Option<RequestId>,
    reviewer: Option<ReviewerId>,
) -> Vec<OsString> {
    let mut args = vec![OsString::from(command)];
    args.extend(request.map(|id| OsString::from(id.as_str())));
    for (flag, value) in [
        ("--repo", scope.repository().map(PathBuf::into_os_string)),
        ("--state-dir", state_dir.map(PathBuf::into_os_string)),
        ("--reviewer", reviewer.map(|id| OsString::from(id.as_str()))),
    ] {
        if let Some(value) = value {
            let mut option = OsString::from(flag);
            option.push("=");
            option.push(value);
            args.push(option);
        }
    }
    args
}
