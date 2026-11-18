//! Runtime startup, screen entry points, and cancellation signals.

use std::{
    io::{self, Write},
    path::PathBuf,
    process::ExitCode,
};

use clap::error::ErrorKind;
use serde_json::json;

use super::{FAILURE_EXIT, ParseError, dispatch, parse_args};

pub(super) fn cancellation_listener() -> Result<
    (
        tokio_util::sync::CancellationToken,
        tokio::task::JoinHandle<()>,
    ),
    String,
> {
    let cancellation = tokio_util::sync::CancellationToken::new();
    let stop = artifactize_core::platform::stop_requested().map_err(|e| e.to_string())?;
    let token = cancellation.clone();
    let listener = tokio::spawn(async move {
        stop.await;
        token.cancel();
    });
    Ok((cancellation, listener))
}

/// What a screen runs until the reviewer quits; screens need no `Send`, the command runs on a
/// single thread.
pub type Screen = std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), String>>>>;

/// The terminal screens `artifactize monitor` and `artifactize review` open. They live in
/// `artifactize-tui`; the binary wires these entry points without a dependency from core to CLI.
#[derive(Clone, Copy)]
pub struct Screens {
    /// The Monitor over the state directory, on one repository or on every one.
    pub monitor: fn(PathBuf, Option<PathBuf>, tokio_util::sync::CancellationToken) -> Screen,
    /// The Human review list, or one request when given.
    pub review: fn(
        PathBuf,
        Option<PathBuf>,
        artifactize_core::types::ReviewerId,
        Option<artifactize_core::types::RequestId>,
        tokio_util::sync::CancellationToken,
    ) -> Screen,
}

pub fn run(screens: Screens) -> ExitCode {
    let args: Vec<_> = std::env::args_os().collect();
    let has_args = args.len() > 1;
    let (cli, json) = match parse_args(&args) {
        Ok(parsed) => parsed,
        Err(error) => return print_parse_error(error),
    };
    // Run on a thread whose stack is the same size everywhere: Windows gives the main thread
    // only 1 MiB, which deep review futures outgrow in debug builds.
    let worker = std::thread::Builder::new()
        .name("artifactize".into())
        .stack_size(STACK_BYTES)
        .spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .thread_stack_size(STACK_BYTES)
                .build()
                .map_err(|error| error.to_string())?;
            runtime.block_on(Box::pin(async {
                let result = dispatch::execute(cli, screens).await;
                artifactize_core::changes::drain().await;
                result
            }))
        });
    let result = match worker {
        Ok(worker) => worker
            .join()
            .unwrap_or_else(|panic| std::panic::resume_unwind(panic)),
        Err(error) => Err(error.to_string()),
    };
    match result {
        Ok(code) => match artifactize_core::platform::pause_after_help(has_args) {
            Ok(()) => ExitCode::from(code),
            Err(error) => failure(&error.to_string(), json),
        },
        Err(error) => failure(&error, json),
    }
}

/// The stack of the thread commands run on, and of blocking helper threads: 8 MiB, the
/// usual Linux and macOS main-thread size, so every system runs the same futures.
const STACK_BYTES: usize = 8 * 1024 * 1024;

fn print_parse_error(error: ParseError) -> ExitCode {
    match error {
        ParseError::RepeatedGlobal { name, json } => {
            failure(&format!("{name} cannot be used multiple times."), json)
        }
        ParseError::Clap { error, json: true } if error.use_stderr() => {
            let message = error.to_string();
            let message = if error.kind() == ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand {
                "A subcommand is required; see --help."
            } else {
                message
                    .split("\n\n")
                    .next()
                    .unwrap_or_default()
                    .trim_start_matches("error: ")
            };
            failure(message, true)
        }
        ParseError::Clap { error, .. } => {
            if error.print().is_ok() && !error.use_stderr() {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(FAILURE_EXIT)
            }
        }
    }
}

fn failure(message: &str, json: bool) -> ExitCode {
    if json {
        let _ = writeln!(io::stdout().lock(), "{}", json!({ "error": message }));
    } else {
        let _ = writeln!(io::stderr().lock(), "{message}");
    }
    ExitCode::from(FAILURE_EXIT)
}
