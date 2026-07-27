//! Binary entry point: parses CLI arguments and dispatches to
//! [`traces_pkm::cli::run`]. Logic lives in the lib crate (see
//! `src/lib.rs`); this stays minimal (`proj-lib-main-split`).

use std::process::ExitCode;

use traces_pkm::cli::{CommandOutcome, UserAbort};

fn main() -> ExitCode {
    match traces_pkm::cli::run() {
        Ok(
            CommandOutcome::Completed
            | CommandOutcome::Aborted(UserAbort::Cancelled),
        ) => ExitCode::SUCCESS,
        Ok(CommandOutcome::Aborted(UserAbort::Interrupted)) => {
            ExitCode::from(130)
        }
        Err(error) => {
            eprintln!("{:?}", miette::Report::new(error));
            ExitCode::FAILURE
        }
    }
}
