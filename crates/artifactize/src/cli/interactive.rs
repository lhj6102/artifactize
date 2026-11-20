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

pub(super) enum Screen {
    Monitor,
    Review {
        request: Option<RequestId>,
        reviewer: Option<ReviewerId>,
    },
}

pub(super) struct Launch {
    pub scope: Scope,
    pub screen: Screen,
}

impl Launch {
    pub(super) async fn run(self, state_dir: Option<PathBuf>) -> Result<u8, String> {
        if let Screen::Review {
            request: Some(id), ..
        } = &self.screen
        {
            let state = artifactize_core::store::state_dir(state_dir.as_deref())?;
            artifactize_core::store::read_request(&state, id).await?;
        }
        artifactize_core::platform::launch_desktop(&self.args(state_dir))?;
        Ok(0)
    }

    pub(super) fn args(self, state_dir: Option<PathBuf>) -> Vec<OsString> {
        let mut args = match self.screen {
            Screen::Monitor => vec!["monitor".into()],
            Screen::Review { request, reviewer } => {
                let mut args = vec!["review".into()];
                args.extend(request.map(|id| OsString::from(id.as_str())));
                if let Some(id) = reviewer {
                    args.push(option("--reviewer", id.as_str()));
                }
                args
            }
        };
        if let Some(repo) = self.scope.repository() {
            args.push(option("--repo", repo));
        }
        if let Some(state) = state_dir {
            args.push(option("--state-dir", state));
        }
        args
    }
}

fn option(flag: &str, value: impl AsRef<std::ffi::OsStr>) -> OsString {
    let mut argument = OsString::from(flag);
    argument.push("=");
    argument.push(value);
    argument
}
