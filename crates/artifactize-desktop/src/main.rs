// The release app has no console window on Windows; the terminal UIs stay in the CLI.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() -> std::process::ExitCode {
    let launch = match artifactize_desktop::Launch::parse(std::env::args_os()) {
        Ok(launch) => launch,
        Err(error) => {
            eprintln!("{error}");
            return std::process::ExitCode::from(2);
        }
    };
    let result =
        artifactize_desktop::app(tauri::Builder::default(), launch).run(tauri::generate_context!());
    match result {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("cannot start the desktop app: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}
