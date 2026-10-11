//! The echo-only guard's restoration contract across return, error and future cancellation.
//! Crate choices are documented in the parent platform module. These tests cover guard
//! ownership, not a claim that remote login catches external signals.

#[cfg(test)]
mod tests {
    use std::io::{self, BufRead, Write};

    use super::super::HiddenInput;
    use crate::test_os::{self, PseudoTerminal};

    const READY: &str = "HIDDEN_INPUT_READY";
    const RESTORED: &str = "HIDDEN_INPUT_RESTORED";
    const VISIBLE: &str = "visible-after-restoration";

    /// Each acknowledgment is emitted only after the mode change it describes. No sleeps
    /// or elapsed-time assertions decide whether input is hidden or restored.
    #[test]
    fn terminal_modes_restore_after_success_error_and_cancellation() {
        let (mut terminal, receiver) = PseudoTerminal::start(
            &std::env::current_exe().unwrap(),
            &[
                "--exact",
                "platform::hidden_input::tests::restoration_probe",
                "--ignored",
                "--nocapture",
            ],
            &[],
            100,
            30,
        );
        let wait = test_os::patience(std::time::Duration::from_secs(10));
        let mut output = Vec::new();
        for case in 0..3 {
            let ready = format!("{READY}_{case}");
            while !String::from_utf8_lossy(&output).contains(&ready) {
                output.extend(
                    receiver
                        .recv_timeout(wait)
                        .expect("no hidden-input acknowledgment"),
                );
            }
            terminal.type_text(format!("secret-input-{case}\r").as_bytes());
            let restored = format!("{RESTORED}_{case}");
            while !String::from_utf8_lossy(&output).contains(&restored) {
                output.extend(
                    receiver
                        .recv_timeout(wait)
                        .expect("no restoration acknowledgment"),
                );
            }
            terminal.type_text(format!("{VISIBLE}-{case}\r").as_bytes());
        }
        assert!(terminal.finish());
        while let Ok(chunk) = receiver.recv_timeout(wait) {
            output.extend(chunk);
        }
        let output = String::from_utf8_lossy(&output);
        for case in 0..3 {
            assert!(
                !output.contains(&format!("secret-input-{case}")),
                "{output}"
            );
            assert!(output.contains(&format!("{VISIBLE}-{case}")), "{output}");
        }
    }

    /// Runs only in the parent test's private terminal, never in the test runner's console.
    #[tokio::test]
    #[ignore = "launched by the restoration test inside a private pseudo-terminal"]
    async fn restoration_probe() {
        for case in 0..3 {
            let original = test_os::input_terminal_mode();
            if case == 2 {
                let (send, receive) = tokio::sync::oneshot::channel::<()>();
                let mut reading = Box::pin(async {
                    let _hidden = HiddenInput::new().unwrap();
                    acknowledge(READY, case);
                    receive.await.unwrap();
                });
                assert!(futures_util::poll!(&mut reading).is_pending());
                assert_ne!(test_os::input_terminal_mode(), original);
                read_line(); // The parent acknowledges the pending future with hidden input.
                drop(reading); // Cancellation drops the guard without resolving the read.
                drop(send);
            } else {
                assert_eq!(read_hidden(case).is_err(), case == 1);
            }
            assert_eq!(test_os::input_terminal_mode(), original);
            acknowledge(RESTORED, case);
            assert_eq!(read_line().trim(), format!("{VISIBLE}-{case}"));
        }
    }

    fn read_hidden(case: usize) -> io::Result<()> {
        let original = test_os::input_terminal_mode();
        let _hidden = HiddenInput::new()?;
        assert_ne!(test_os::input_terminal_mode(), original);
        acknowledge(READY, case);
        read_line();
        if case == 1 {
            // An I/O failure propagates out of the scope that owns the guard.
            Err(io::ErrorKind::UnexpectedEof.into())
        } else {
            Ok(())
        }
    }

    fn acknowledge(message: &str, case: usize) {
        let mut stdout = io::stdout().lock();
        writeln!(stdout, "{message}_{case}").unwrap();
        stdout.flush().unwrap();
    }

    fn read_line() -> String {
        let mut line = String::new();
        io::stdin().lock().read_line(&mut line).unwrap();
        line
    }
}
