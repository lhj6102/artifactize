//! Secrets read from stdin, never argv: hidden at a terminal, piped otherwise.

use std::io::{self, BufRead, IsTerminal, Read, Write};

use artifactize_core::platform::HiddenInput;

/// Bound stdin allocation before validation, with room beyond the 4096-byte token and key
/// limits for surrounding whitespace and line endings; oversized secrets stay invalid.
const SECRET_STDIN_BYTES: u64 = 8192;

/// One line of stdin, trimmed. At a terminal, `what` names the secret in the prompt.
pub(super) fn read(what: &str) -> Result<String, String> {
    let stdin = io::stdin().lock();
    let hidden = if stdin.is_terminal() {
        // Echo goes off before the prompt invites a paste.
        let hidden = match HiddenInput::new() {
            Ok(hidden) => Some(hidden),
            // Where the platform allows it, read the secret visibly rather than not at all.
            Err(error) if artifactize_core::platform::VISIBLE_INPUT_FALLBACK => {
                writeln!(
                    io::stderr().lock(),
                    "Warning: this terminal cannot hide input ({error}); the {what} will be visible."
                )
                .map_err(|e| e.to_string())?;
                None
            }
            Err(error) => return Err(error.to_string()),
        };
        write!(io::stderr().lock(), "Paste the {what}, then press Enter: ")
            .map_err(|e| e.to_string())?;
        hidden
    } else {
        None
    };
    let mut secret = String::new();
    let read = stdin.take(SECRET_STDIN_BYTES).read_line(&mut secret);
    if hidden.is_some() {
        drop(hidden);
        // The Enter key was not echoed either.
        let _ = writeln!(io::stderr().lock());
    }
    read.map_err(|_| format!("Cannot read the {what} from stdin."))?;
    Ok(secret.trim().to_owned())
}
