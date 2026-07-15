//! Binary entry point: parses CLI arguments and dispatches to
//! [`traces_pkm::cli::run`]. Logic lives in the lib crate (see
//! `src/lib.rs`); this stays minimal (`proj-lib-main-split`).

use std::{error::Error as StdError, process::ExitCode};

fn main() -> ExitCode {
    match traces_pkm::cli::run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            print_error(&error);
            ExitCode::FAILURE
        }
    }
}

/// Prints `error` and its full source chain to stderr, one line per link.
fn print_error(error: &traces_pkm::cli::CliError) {
    eprintln!("error: {error}");
    let mut source = StdError::source(error);
    while let Some(cause) = source {
        eprintln!("  caused by: {cause}");
        source = cause.source();
    }
}
