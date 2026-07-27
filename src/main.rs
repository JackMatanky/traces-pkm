//! Binary entry point: parses CLI arguments and dispatches to
//! [`traces_pkm::cli::run`]. Logic lives in the lib crate (see
//! `src/lib.rs`); this stays minimal (`proj-lib-main-split`).

use std::process::ExitCode;

use traces_pkm::cli::{CommandOutcome, UserAbort};

fn main() -> ExitCode {
    exit_code(traces_pkm::cli::run())
}

/// Maps a top-level [`cli::run`](traces_pkm::cli::run) result to the
/// process exit code.
///
/// Escape completes the command (exit `0`); Ctrl-C interrupts it (exit
/// `130`, the POSIX convention for SIGINT); any other failure is a
/// diagnostic reported to stderr (exit `1`).
fn exit_code(
    result: Result<CommandOutcome, traces_pkm::cli::CliError>,
) -> ExitCode {
    match result {
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

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use traces_pkm::cli::CliError;

    use super::*;

    #[test]
    fn completed_exits_success() {
        assert_eq!(exit_code(Ok(CommandOutcome::Completed)), ExitCode::SUCCESS);
    }

    #[test]
    fn escape_abort_exits_success() {
        assert_eq!(
            exit_code(Ok(CommandOutcome::Aborted(UserAbort::Cancelled))),
            ExitCode::SUCCESS
        );
    }

    #[test]
    fn ctrl_c_abort_exits_130() {
        assert_eq!(
            exit_code(Ok(CommandOutcome::Aborted(UserAbort::Interrupted))),
            ExitCode::from(130)
        );
    }

    #[test]
    fn diagnostic_failure_exits_failure() {
        assert_eq!(exit_code(Err(CliError::NoCommand)), ExitCode::FAILURE);
    }
}
