//! How long a Run with only unclaimed sign-offs left stays open, as `artifactize limits` shows
//! and sets it in the state's limits.json.
use crate::{App, Failure, ids::WholeSeconds};
use artifactize_core::limits::{Limits, MAX_SIGNOFF_IDLE_GRACE};
use serde::Serialize;
use specta::Type;
use std::time::Duration;

/// The idle grace in force, and the longest one accepted, in whole seconds.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SignoffGrace {
    pub seconds: WholeSeconds,
    pub max_seconds: WholeSeconds,
}

impl From<&Limits> for SignoffGrace {
    fn from(limits: &Limits) -> Self {
        Self {
            seconds: limits.signoff_idle_grace().into(),
            max_seconds: MAX_SIGNOFF_IDLE_GRACE.into(),
        }
    }
}

/// `limits`: the idle grace Runs started now take.
#[tauri::command]
#[specta::specta]
pub fn signoff_grace(app: tauri::State<'_, App>) -> Result<SignoffGrace, Failure> {
    Limits::read(&app.launch.state)
        .map(|limits| (&limits).into())
        .map_err(Failure::Setting)
}

/// `limits --signoff-idle-grace SECONDS`: the Runs started afterwards take it.
#[tauri::command]
#[specta::specta]
pub fn signoff_grace_set(
    app: tauri::State<'_, App>,
    seconds: WholeSeconds,
) -> Result<SignoffGrace, Failure> {
    Limits::set_signoff_idle_grace(&app.launch.state, Duration::from(seconds))
        .map(|limits| (&limits).into())
        .map_err(Failure::Setting)
}
