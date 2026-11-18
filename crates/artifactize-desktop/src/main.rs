// The terminal UIs stay in the CLI; this is the window app (its Windows link settings are in
// build.rs).
/// What the CLI exits with for arguments it cannot read, so scripts treat both the same.
const USAGE_ERROR: u8 = 2;

#[cfg(feature = "e2e")]
mod e2e_metadata;

fn main() -> std::process::ExitCode {
    let launch = match artifactize_desktop::Launch::parse(std::env::args_os()) {
        Ok(launch) => launch,
        Err(error) => {
            eprintln!("{error}");
            return std::process::ExitCode::from(USAGE_ERROR);
        }
    };
    let builder = tauri::Builder::default();
    #[cfg(feature = "e2e")]
    let (builder, exit) = match e2e_metadata::init() {
        Ok((metadata, exit)) => (
            builder
                .plugin(metadata)
                .plugin(tauri_plugin_wdio::init())
                .plugin(tauri_plugin_wdio_webdriver::init()),
            exit,
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
    let result = app.build(tauri::generate_context!()).map(|app| {
        let code = app.run_return(|_, _| {});
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
