//! Immutable self-identity for the quit tests' read-only native exit observation.
//! This module and its webview metadata do not exist without the E2E feature.

#[expect(
    dead_code,
    reason = "the app only reads its own birth identity; the observer binary checks exit"
)]
#[path = "../../artifactize-core/tests/support/os/observation.rs"]
mod observation;

/// Capture once before the service-owned app opens any window. The plugin has no
/// commands or lifecycle callbacks: it only supplies immutable observation data.
pub fn init<R: tauri::Runtime>() -> Result<tauri::plugin::TauriPlugin<R>, String> {
    let pid = std::process::id();
    let start_time = observation::observed_start_time(pid)
        .ok_or_else(|| "cannot observe the E2E app's native birth identity".to_owned())?;
    let identity = serde_json::json!({ "pid": pid, "start_time": start_time.to_string() });
    Ok(tauri::plugin::Builder::new("e2e-observation")
        .js_init_script(format!(
            "Object.defineProperty(window, '__ARTIFACTIZE_E2E_PROCESS__', {{ value: Object.freeze({identity}), writable: false, configurable: false }});"
        ))
        .build())
}
