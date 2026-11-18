//! Secrets read from stdin, never argv: hidden at a terminal, piped otherwise.

use std::io::{self, BufRead, Read, Write};

use artifactize_core::platform::{self, SecretInput};

/// Bound stdin allocation before validation, with room beyond the 4096-byte token and key
/// limits for surrounding whitespace and line endings; oversized secrets stay invalid.
const SECRET_STDIN_BYTES: u64 = 8192;

/// One line of stdin, trimmed. At a terminal, `what` names the secret in the prompt.
pub(super) fn read(what: &str) -> Result<String, String> {
    let stdin = io::stdin().lock();
    let mode = platform::secret_input().map_err(|e| e.to_string())?;
    if let SecretInput::Visible(error) = &mode {
        writeln!(
            io::stderr().lock(),
            "Warning: this terminal cannot hide input ({error}); the {what} will be visible."
        )
        .map_err(|e| e.to_string())?;
    }
    if !matches!(mode, SecretInput::Redirected) {
        write!(io::stderr().lock(), "Paste the {what}, then press Enter: ")
            .map_err(|e| e.to_string())?;
    }
    let mut secret = String::new();
    let read = stdin.take(SECRET_STDIN_BYTES + 1).read_line(&mut secret);
    if let SecretInput::Hidden(guard) = mode {
        drop(guard);
        // The Enter key was not echoed either.
        let _ = writeln!(io::stderr().lock());
    }
    let read = read.map_err(|_| format!("Cannot read the {what} from stdin."))?;
    if read as u64 > SECRET_STDIN_BYTES {
        return Err(format!(
            "The {what} line exceeds {SECRET_STDIN_BYTES} bytes."
        ));
    }
    Ok(secret.trim().to_owned())
}
