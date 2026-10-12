// The terminal UIs stay in the CLI; this is the window app (its Windows link settings are in
// build.rs).
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
