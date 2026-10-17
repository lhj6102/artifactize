//! The test build's engine text-input device. It changes only the WebView's native
//! composition, never the app's review state or keyboard/OS event stream.
mod platform;

use serde::Deserialize;

/// Native composition operations; keystrokes deliberately do not belong to this device.
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
enum Input {
    Set { text: String },
    Commit { text: String },
    Cancel,
}

/// Bound text passed through the test IPC and native input-method allocation.
const MAX_TEXT_BYTES: usize = 64 * 1024;

#[tauri::command]
async fn e2e_input<R: tauri::Runtime>(
    window: tauri::WebviewWindow<R>,
    input: Input,
) -> Result<(), String> {
    if let Input::Set { text } | Input::Commit { text } = &input
        && (text.len() > MAX_TEXT_BYTES || text.contains('\0'))
    {
        return Err("Native composition text must be at most 64 KiB and contain no NUL.".into());
    }
    platform::apply(&window, input).await
}

/// The application router delegates only this test-build command, outside production bindings.
pub(super) fn handle<R: tauri::Runtime>(invoke: tauri::ipc::Invoke<R>) -> bool {
    let handler: fn(tauri::ipc::Invoke<R>) -> bool = tauri::generate_handler![e2e_input];
    handler(invoke)
}
