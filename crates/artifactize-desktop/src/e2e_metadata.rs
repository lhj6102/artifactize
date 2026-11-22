//! Immutable self-identity for the quit tests' read-only native exit observation.
//! This module and its webview metadata do not exist without the E2E feature.

#[expect(
    dead_code,
    reason = "the app only reads its own birth identity; the observer binary checks exit"
)]
#[path = "../../artifactize-core/tests/support/os/observation.rs"]
mod observation;

/// Check the settings and frontend of this actual binary, without opening a window.
pub fn check_build(context: &mut tauri::Context<tauri::Wry>) -> std::process::ExitCode {
    let plugin_path = b"/@wdio/tauri-plugin/";
    let test_frontend = context
        .assets()
        .get(&"/.vite/manifest.json".into())
        .is_some_and(|contents| {
            contents
                .windows(plugin_path.len())
                .any(|part| part == plugin_path)
        });
    // The runner's frame actions (callWindow): focus, orderly close and native title reads.
    // Query the resolved permissions for each window that the suite addresses, including a
    // representative label for the Artifact Viewer's generated viewer-* labels.
    let authority = context.runtime_authority_mut();
    let missing_window_access: Vec<_> = [
        ("main", "set_focus"),
        ("main", "close"),
        ("main", "title"),
        ("viewer-1", "title"),
        ("pet", "title"),
    ]
    .into_iter()
    .filter_map(|(window, name)| {
        let command = format!("plugin:window|{name}");
        authority
            .resolve_access(&command, window, window, &tauri::ipc::Origin::Local)
            .is_none()
            .then(|| format!("{window}: {name}"))
    })
    .collect();
    println!(
        "{}",
        serde_json::json!({
            "embeddedDriver": true,
            "globalTauri": context.config().app.with_global_tauri,
            "testFrontend": test_frontend,
            "missingWindowAccess": missing_window_access,
        })
    );
    std::process::ExitCode::SUCCESS
}

/// Recover an app started by the service when its ordinary shutdown failed. Its owner file
/// is created before the WebDriver starts; never send a signal based on a PID alone.
pub fn stop_owner(path: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    #[derive(serde::Deserialize)]
    struct Owner {
        pid: artifactize_core::process::ProcessId,
        start_time: String,
    }
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        // A service startup can fail before it creates the app. Without an owner receipt
        // it did not reach WebDriver initialization and there is no fixture session to stop.
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
    };
    let owner: Owner = serde_json::from_reader(file)?;
    // Leave time to finish native process exit, but do not block fixture teardown forever.
    const EXIT_BOUND: std::time::Duration = std::time::Duration::from_secs(10);
    observation::stop_owned(
        owner.pid.get(),
        owner.start_time.parse()?,
        false,
        EXIT_BOUND,
    )?;
    Ok(())
}

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

/// Only the installed fixture executable launched by the real graph tool selects
/// the companion's observation endpoint. The graph tool still owns the launch.
fn companion_port() -> Result<Option<u16>, String> {
    use artifactize_core::platform::environment::var;
    let Some(executable) = var("ARTIFACTIZE_E2E_COMPANION_EXE") else {
        return Ok(None);
    };
    let actual = std::env::current_exe().and_then(std::fs::canonicalize);
    let expected = std::fs::canonicalize(executable);
    if actual.map_err(|error| error.to_string())? != expected.map_err(|error| error.to_string())? {
        return Ok(None);
    }
    let port = var("ARTIFACTIZE_E2E_COMPANION_PORT")
        .and_then(|value| value.to_str()?.parse::<u16>().ok())
        .filter(|port| *port != 0)
        .ok_or_else(|| "invalid E2E graph companion port".to_owned())?;
    Ok(Some(port))
}

/// Fail-safe for the E2E companion only: a missing driver must not leave a
/// product-launched window running after its observer fails. The normal close
/// still comes from the observation session; the fail-safe cannot pass the case.
pub fn arm_companion_close<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    abandoned: std::sync::Arc<std::sync::atomic::AtomicBool>,
) {
    use tauri::Manager;
    // Leave time for driver attachment and for the fixture to inspect the window.
    const ABANDONED_AFTER: std::time::Duration = std::time::Duration::from_secs(60);
    if let Some(window) = app.get_webview_window("main") {
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(ABANDONED_AFTER).await;
            abandoned.store(true, std::sync::atomic::Ordering::Release);
            eprintln!("E2E graph companion reached its shutdown watchdog without closing");
            if let Err(error) = window.close() {
                eprintln!("E2E graph companion could not close: {error}");
            }
        });
    }
}

/// Capture once before the service-owned app opens any window. The plugin has no
/// commands or lifecycle callbacks: it only supplies immutable observation data.
pub fn init<R: tauri::Runtime>()
-> Result<(tauri::plugin::TauriPlugin<R>, ExitReceipt, Option<u16>), String> {
    let pid = std::process::id();
    let start_time = observation::observed_start_time(pid)
        .ok_or_else(|| "cannot observe the E2E app's native birth identity".to_owned())?;
    let identity = serde_json::json!({ "pid": pid, "start_time": start_time.to_string() });
    let companion = companion_port()?;
    let owner_var = if companion.is_some() {
        "ARTIFACTIZE_E2E_COMPANION_OWNER"
    } else {
        "ARTIFACTIZE_E2E_OWNER_FILE"
    };
    let owner =
        artifactize_core::platform::environment::var(owner_var).map(std::path::PathBuf::from);
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
    Ok((metadata, exit, companion))
}

/// Keep a recent image of the fixture's own main window, even before WebDriver can connect.
#[cfg(target_os = "macos")]
pub fn arm_window_image<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    use std::{ffi::CString, os::unix::ffi::OsStrExt, sync::Arc, time::Duration};
    use tauri::Manager;

    unsafe extern "C" {
        fn arti_e2e_window_image(webview: *mut std::ffi::c_void, path: *const std::ffi::c_char);
    }

    let Some(path) = std::env::var_os("ARTIFACTIZE_E2E_WINDOW_IMAGE") else {
        return;
    };
    let Ok(path) = CString::new(path.as_bytes()) else {
        return;
    };
    let Some(main) = app.get_webview_window("main") else {
        return;
    };
    let path = Arc::new(path);
    tauri::async_runtime::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(1));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            let path = path.clone();
            if main
                .with_webview(move |webview| {
                    // SAFETY: Tauri invokes this closure on the WebKit main thread with its live
                    // WKWebView pointer. The adapter copies the path before returning.
                    unsafe {
                        arti_e2e_window_image(webview.inner(), path.as_ptr());
                    }
                })
                .is_err()
            {
                break;
            }
        }
    });
}
