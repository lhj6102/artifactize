//! Keep help visible when a desktop launch creates a console just for this process.

use std::io::{self, IsTerminal, Write};

/// Arguments and redirected input never turn a command into an interactive prompt.
fn should_pause(owns_console: bool, has_args: bool, stdin_is_console: bool) -> bool {
    owns_console && !has_args && stdin_is_console
}

#[cfg(windows)]
fn owns_console() -> bool {
    use windows_sys::Win32::System::Console::GetConsoleProcessList;

    let mut process = 0;
    // SAFETY: process points to one writable DWORD; the supplied buffer length is one.
    // The API returns the total attached process count even if the buffer is too small.
    unsafe { GetConsoleProcessList(&mut process, 1) == 1 }
}

#[cfg(unix)]
fn owns_console() -> bool {
    false
}

/// Only a no-argument desktop launch with console input AND output may wait for Enter.
pub fn pause_after_help(has_args: bool) -> io::Result<()> {
    if !has_args
        && io::stdout().is_terminal()
        && should_pause(owns_console(), has_args, io::stdin().is_terminal())
    {
        let mut output = io::stdout().lock();
        writeln!(
            output,
            "\nOpen Windows Terminal or PowerShell and run artifactize there. Press Enter to exit."
        )?;
        output.flush()?;
        drop(output);
        io::stdin().read_line(&mut String::new())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::should_pause;

    #[test]
    fn only_owned_console_help_with_console_input_pauses() {
        for (owns_console, has_args, stdin_is_console, expected) in [
            (false, false, false, false),
            (false, false, true, false),
            (false, true, false, false),
            (false, true, true, false),
            (true, false, false, false),
            (true, false, true, true),
            (true, true, false, false),
            (true, true, true, false),
        ] {
            assert_eq!(
                should_pause(owns_console, has_args, stdin_is_console),
                expected,
                "owns_console={owns_console}, has_args={has_args}, stdin_is_console={stdin_is_console}"
            );
        }
    }
}
