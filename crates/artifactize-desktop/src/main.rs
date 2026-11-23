// The terminal UIs stay in the CLI; this is the window app (its Windows link settings are in
// build.rs).
/// What the CLI exits with for arguments it cannot read, so scripts treat both the same.
const USAGE_ERROR: u8 = 2;

#[cfg(feature = "e2e")]
mod e2e_metadata;

fn main() -> std::process::ExitCode {
    #[cfg(feature = "e2e")]
    let mut context: tauri::Context<tauri::Wry> = tauri::generate_context!();
    #[cfg(feature = "e2e")]
    if std::env::args_os()
        .nth(1)
        .is_some_and(|arg| arg == "--check-e2e-build")
    {
        return e2e_metadata::check_build(&mut context);
    }
    #[cfg(feature = "e2e")]
    if std::env::args_os()
        .nth(1)
        .is_some_and(|arg| arg == "--stop-e2e-owner")
    {
        let Some(path) = std::env::args_os().nth(2) else {
            return std::process::ExitCode::from(USAGE_ERROR);
        };
        return match e2e_metadata::stop_owner(std::path::Path::new(&path)) {
            Ok(()) => std::process::ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("cannot stop the E2E fixture app: {error}");
                std::process::ExitCode::FAILURE
            }
        };
    }
    let launch = match artifactize_desktop::Launch::parse(std::env::args_os()) {
        Ok(launch) => launch,
        Err(error) => {
            eprintln!("{error}");
            return std::process::ExitCode::from(USAGE_ERROR);
        }
    };
    let builder = tauri::Builder::default();
    #[cfg(feature = "e2e")]
    let (builder, exit, companion) = match e2e_metadata::init() {
        Ok((metadata, exit, companion)) => (
            builder
                .plugin(metadata)
                .plugin(tauri_plugin_wdio::init())
                .plugin(companion.map_or_else(
                    tauri_plugin_wdio_webdriver::init,
                    tauri_plugin_wdio_webdriver::init_with_port,
                )),
            exit,
            companion,
        ),
        Err(error) => {
            eprintln!("cannot initialize E2E observation: {error}");
            return std::process::ExitCode::FAILURE;
        }
    };
    let app = artifactize_desktop::app(builder, launch);
    #[cfg(not(feature = "e2e"))]
    let result = app.run(tauri::generate_context!());
    #[cfg(feature = "e2e")]
    let result = app.build(context).map(|app| {
        use std::sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        };
        let abandoned = Arc::new(AtomicBool::new(false));
        let watchdog = abandoned.clone();
        let code = app.run_return(move |app, event| {
            if matches!(event, tauri::RunEvent::Ready) {
                e2e_metadata::arm_window_image(app);
                if companion.is_some() {
                    e2e_metadata::arm_companion_close(app, watchdog.clone());
                }
            }
        });
        if abandoned.load(Ordering::Acquire) {
            // If emergency cleanup fired, it is not a successful observed close.
            std::process::exit(1);
        }
        if code == 0 {
            if let Err(error) = exit.record() {
                eprintln!("cannot record orderly E2E app exit: {error}");
                std::process::exit(1);
            }
        } else {
            std::process::exit(code);
        }
    });
    match result {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("cannot start the desktop app: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}
