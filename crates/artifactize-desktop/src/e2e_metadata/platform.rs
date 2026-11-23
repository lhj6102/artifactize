//! Native desktop observation for an E2E-owned window.

/// Keep a recent image of the fixture's own main window, even before WebDriver can connect.
#[cfg(target_os = "macos")]
pub fn arm_window_image<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    use std::{ffi::CString, os::unix::ffi::OsStrExt, sync::Arc, time::Duration};
    use tauri::Manager;

    unsafe extern "C" {
        fn arti_e2e_window_image(webview: *mut std::ffi::c_void, path: *const std::ffi::c_char);
    }

    let Some(path) = artifactize_core::platform::environment::var("ARTIFACTIZE_E2E_WINDOW_IMAGE")
    else {
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

#[cfg(not(target_os = "macos"))]
pub fn arm_window_image<R: tauri::Runtime>(_: &tauri::AppHandle<R>) {}
