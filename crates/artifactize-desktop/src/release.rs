//! The header's newer-release notice: the library's [`release::check`], typed for the frontend.
//! The app never updates itself; it shows how to update the way it was installed.
use crate::{App, Failure};
use artifactize_core::release;
use serde::Serialize;
use specta::Type;

/// A newer release and how to update to it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseNotice {
    pub running: String,
    pub latest: String,
    pub update: ReleaseUpdate,
}

/// How to update this install, as [`release::Update`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ReleaseUpdate {
    /// Run this command.
    Command { command: String },
    /// Download this file and install it by hand, with the command when there is one.
    Download {
        url: String,
        command: Option<String>,
    },
    /// See the release page: how the app was installed is unknown.
    Page { url: String },
}

impl From<release::Update> for ReleaseUpdate {
    fn from(update: release::Update) -> Self {
        match update {
            release::Update::Command { command } => Self::Command { command },
            release::Update::Download { url, command } => Self::Download { url, command },
            release::Update::Page { url } => Self::Page { url },
        }
    }
}

/// The page an update links to, if any.
fn link(update: &release::Update) -> Option<&str> {
    match update {
        release::Update::Command { .. } => None,
        release::Update::Download { url, .. } | release::Update::Page { url } => Some(url),
    }
}

/// The newer release, if one is out; `None` when this copy is the latest, the lookup is off
/// or failed, or the Microsoft Store updates it. A lookup made within the day is reused.
#[tauri::command]
#[specta::specta]
pub async fn release_notice(app: tauri::State<'_, App>) -> Result<Option<ReleaseNotice>, Failure> {
    let checked = release::check(&app.launch.state).await;
    Ok(match checked.outcome {
        release::Outcome::Newer { latest, update } => Some(ReleaseNotice {
            running: checked.running.to_string(),
            latest: latest.to_string(),
            update: update.into(),
        }),
        _ => None,
    })
}

/// Open the newer release's download or page in the browser.
#[tauri::command]
#[specta::specta]
pub async fn release_open(app: tauri::State<'_, App>) -> Result<(), Failure> {
    let checked = release::check(&app.launch.state).await;
    let release::Outcome::Newer { update, .. } = checked.outcome else {
        return Err(Failure::Missing("No newer release is out.".into()));
    };
    let url = link(&update).ok_or_else(|| Failure::Missing("The update has no page.".into()))?;
    artifactize_tools::opener::open(url.as_ref())
        .await
        .map_err(|error| Failure::Action(format!("Cannot open {url}: {error}")))
}
