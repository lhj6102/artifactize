//! Immutable self-identity for the quit tests' read-only native exit observation.
//! This module and its webview metadata do not exist without the E2E feature.

#[expect(
    dead_code,
    reason = "the app only reads its own birth identity; the observer binary checks exit"
)]
#[path = "../../artifactize-core/tests/support/os/observation.rs"]
mod observation;

/// Evidence of a zero-status return from the normal Tauri event loop.
pub struct ExitReceipt {
    file: Option<std::path::PathBuf>,
    identity: serde_json::Value,
}

impl ExitReceipt {
    pub fn record(&self) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(path) = &self.file {
            let file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(path)?;
            serde_json::to_writer(file, &self.identity)?;
        }
        Ok(())
    }
}

/// Capture once before the service-owned app opens any window. The plugin has no
/// commands or lifecycle callbacks: it only supplies immutable observation data.
pub fn init<R: tauri::Runtime>() -> Result<(tauri::plugin::TauriPlugin<R>, ExitReceipt), String> {
    let pid = std::process::id();
    let start_time = observation::observed_start_time(pid)
        .ok_or_else(|| "cannot observe the E2E app's native birth identity".to_owned())?;
    let identity = serde_json::json!({ "pid": pid, "start_time": start_time.to_string() });
    let owner = artifactize_core::platform::environment::var("ARTIFACTIZE_E2E_OWNER_FILE")
        .map(std::path::PathBuf::from);
    if let Some(path) = &owner {
        let file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .map_err(|error| format!("cannot record native app ownership: {error}"))?;
        serde_json::to_writer(file, &identity)
            .map_err(|error| format!("cannot record native app ownership: {error}"))?;
    }
    let exit = ExitReceipt {
        file: owner.map(|path| path.with_extension("exit.json")),
        identity: identity.clone(),
    };
    let receipt = serde_json::json!(exit.file);
    let fixed_date = artifactize_core::platform::environment::var("ARTIFACTIZE_E2E_DATE")
        .map(|value| {
            value
                .to_str()
                .and_then(|text| text.parse::<i64>().ok())
                .and_then(|seconds| seconds.checked_mul(1000))
                .ok_or_else(|| "invalid E2E date".to_owned())
        })
        .transpose()?;
    let fixed_date = serde_json::json!(fixed_date);
    let metadata = tauri::plugin::Builder::new("e2e-observation")
        .js_init_script(format!(
            "Object.defineProperty(window, '__ARTIFACTIZE_E2E_PROCESS__', {{ value: Object.freeze({{...{identity}, exitReceipt: {receipt}, fixedDate: {fixed_date}}}), writable: false, configurable: false }});"
        ))
        .build();
    Ok((metadata, exit))
}
