//! Binary entry point for the `traces` CLI.
//!
//! Argument parsing and command dispatch live in [`traces_pkm::cli::run`].
//! This file only maps the top-level outcome to a process exit code.

use std::process::ExitCode;

use traces_pkm::cli::{CommandOutcome, UserAbort};

fn main() -> ExitCode {
    exit_code(traces_pkm::cli::run())
}

/// Maps the top-level CLI result to the process exit code.
///
/// Escape exits successfully. Ctrl-C exits with `130`, the POSIX convention
/// for SIGINT. Any other failure is reported to stderr and exits with `1`.
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
