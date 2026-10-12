//! The external editor the standalone review hands owner-fields JSON to.
use std::io::Read;

/// Edit text in `$VISUAL`, then `$EDITOR` (default `vi`, Notepad on Windows); bounded
/// like `--fields-file`.
pub(super) async fn edit(text: String) -> Result<String, String> {
    let file =
        crate::platform::private_temp_file("artifactize-fields-").map_err(|e| e.to_string())?;
    std::fs::write(file.path(), text).map_err(|e| e.to_string())?;
    crate::platform::run_editor(&crate::platform::selected_editor(), file.path())
        .await
        .map_err(|e| format!("cannot run the editor: {e}"))?;
    let mut bytes = Vec::new();
    crate::platform::open_regular(file.path())
        .and_then(|file| {
            file.take(crate::human::FIELDS_READ_BYTES)
                .read_to_end(&mut bytes)
        })
        .map_err(|e| e.to_string())?;
    if bytes.len() > crate::human::MAX_RESULT_BYTES {
        return Err(format!(
            "Human fields exceed {} bytes.",
            crate::human::MAX_RESULT_BYTES
        ));
    }
    String::from_utf8(bytes).map_err(|e| e.to_string())
}
