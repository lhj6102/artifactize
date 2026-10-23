//! How long a Run with only unclaimed sign-offs left stays open, as `artifactize limits` shows
//! and sets it in the state's limits.json.
use crate::{App, Failure};
use artifactize_core::limits::{Limits, MAX_SIGNOFF_IDLE_GRACE};
use serde::Serialize;
use specta::Type;
use std::time::Duration;

/// The idle grace in force, and the longest one accepted, in whole seconds.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SignoffGrace {
    pub seconds: u32,
    pub max_seconds: u32,
}

impl From<&Limits> for SignoffGrace {
    fn from(limits: &Limits) -> Self {
        let seconds = |grace: Duration| u32::try_from(grace.as_secs()).expect("a day fits u32");
        Self {
            seconds: seconds(limits.signoff_idle_grace()),
            max_seconds: seconds(MAX_SIGNOFF_IDLE_GRACE),
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
    seconds: u32,
) -> Result<SignoffGrace, Failure> {
    Limits::set_signoff_idle_grace(&app.launch.state, Duration::from_secs(seconds.into()))
        .map(|limits| (&limits).into())
        .map_err(Failure::Setting)
}
