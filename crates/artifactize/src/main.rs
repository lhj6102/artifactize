mod cli;

fn main() -> std::process::ExitCode {
    cli::run()
}

/// Portable OS fixtures shared with the library's tests.
#[cfg(test)]
#[path = "../../artifactize-core/tests/support/os.rs"]
mod test_os;

#[cfg(test)]
use artifactize_core::store;

/// Saved requests shared with the library's tests.
#[cfg(test)]
#[path = "../../artifactize-core/tests/support/request.rs"]
mod test_request;
