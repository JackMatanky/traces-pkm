//! Error types for the dialog subsystem.
//!
//! The primary type is [`DialogError`], which wraps failures from the
//! underlying dialog backend ([`inquire`]) and from I/O operations.
//! [`From`] impls for [`std::io::Error`] and [`inquire::InquireError`] let
//! the `?` operator convert into [`DialogError`] automatically, preserving
//! the source chain so callers can walk it.

/// Errors returned by [`DialogProvider`](super::DialogProvider) methods.
///
/// Categorised into user-initiated cancellation, input validation, I/O
/// failures, backend errors, and non-TTY conditions.
#[derive(Debug, thiserror::Error)]
pub enum DialogError {
    /// A single-selection prompt received an empty list of options.
    ///
    /// [`select`](super::DialogProvider::select) cannot return an item when
    /// there is nothing to choose from. Contrast with
    /// [`multi_select`](super::DialogProvider::multi_select), which accepts
    /// an empty list and returns an empty [`Vec`].
    #[error("cannot select from an empty list")]
    EmptySelectionInput,

    /// The dialog was cancelled by the user (e.g. pressing Esc).
    #[error("dialog user cancelled the operation")]
    UserCancelled,

    /// The dialog was interrupted by the user (e.g. pressing Ctrl-C).
    #[error("dialog user interrupted the operation")]
    UserInterrupted,

    /// An invalid dialog configuration was provided.
    ///
    /// Contains a description of what was invalid.
    #[error("invalid dialog configuration: {0}")]
    InvalidConfiguration(String),

    /// The I/O medium does not support interactive dialog.
    ///
    /// Returned when the backend reports that stdin is not a terminal and
    /// the caller did not provide fallback defaults.  Should not occur when
    /// using [`TerminalDialogProvider`](super::TerminalDialogProvider) — its
    /// TTY guard catches this condition before invoking the backend.
    #[error("interactive dialog not available, stdin is not a terminal")]
    NotInteractive,

    /// An I/O operation failed during prompting.
    ///
    /// The underlying [`std::io::Error`] is available through the
    /// [`source`](std::error::Error::source) chain.
    #[error("dialog I/O operation failed: {0}")]
    Io(#[source] std::io::Error),

    /// The dialog backend reported an unexpected error.
    ///
    /// The backend error is preserved as the
    /// [`source`](std::error::Error::source) so the chain can be walked,
    /// while its concrete type stays opaque outside of this crate.
    #[error("dialog backend error: {0}")]
    BackendFailure(#[source] Box<dyn std::error::Error + Send + Sync>),
}

/// Converts an [`std::io::Error`] into [`DialogError::Io`].
///
/// Enables `?` on [`std::io::Result`] inside dialog code.
impl From<std::io::Error> for DialogError {
    #[inline]
    fn from(err: std::io::Error) -> Self {
        Self::Io(err)
    }
}

/// Maps each [`inquire::InquireError`] variant to the corresponding
/// [`DialogError`] variant.
///
/// Enables `?` on results from the [`inquire`] backend so errors are
/// transparent to callers.
impl From<inquire::InquireError> for DialogError {
    #[inline]
    fn from(err: inquire::InquireError) -> Self {
        use inquire::InquireError as E;
        match err {
            E::OperationCanceled => Self::UserCancelled,
            E::OperationInterrupted => Self::UserInterrupted,
            E::NotTTY => Self::NotInteractive,
            E::IO(io) => Self::Io(io),
            E::InvalidConfiguration(msg) => Self::InvalidConfiguration(msg),
            E::Custom(err) => Self::BackendFailure(err),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::error::Error as _;

    use super::*;

    mod conversions {
        use super::*;

        #[test]
        fn maps_std_io_error_to_io_variant() {
            let io = std::io::Error::other("std boom");
            let mapped = DialogError::from(io);

            assert!(matches!(mapped, DialogError::Io(_)));
            assert_eq!(
                mapped.to_string(),
                "dialog I/O operation failed: std boom"
            );
        }

        #[test]
        fn maps_inquire_cancel_to_user_cancelled() {
            let mapped =
                DialogError::from(inquire::InquireError::OperationCanceled);

            assert!(matches!(mapped, DialogError::UserCancelled));
            assert_eq!(
                mapped.to_string(),
                "dialog user cancelled the operation"
            );
        }

        #[test]
        fn maps_inquire_interrupt_to_user_interrupted() {
            let mapped =
                DialogError::from(inquire::InquireError::OperationInterrupted);

            assert!(matches!(mapped, DialogError::UserInterrupted));
            assert_eq!(
                mapped.to_string(),
                "dialog user interrupted the operation"
            );
        }

        #[test]
        fn maps_inquire_not_tty_to_not_interactive() {
            let mapped = DialogError::from(inquire::InquireError::NotTTY);

            assert!(matches!(mapped, DialogError::NotInteractive));
            assert_eq!(
                mapped.to_string(),
                "interactive dialog not available, stdin is not a terminal"
            );
        }

        #[test]
        fn maps_inquire_invalid_configuration_to_configuration() {
            let mapped = DialogError::from(
                inquire::InquireError::InvalidConfiguration("bad value".into()),
            );

            assert!(matches!(mapped, DialogError::InvalidConfiguration(_)));
            assert_eq!(
                mapped.to_string(),
                "invalid dialog configuration: bad value"
            );
        }

        #[test]
        fn preserves_source_chain_for_inquire_io() {
            let io = std::io::Error::other("boom");

            let mapped = DialogError::from(inquire::InquireError::IO(io));

            assert!(matches!(mapped, DialogError::Io(_)));
            assert!(mapped.to_string().contains("boom"));
            let source = mapped.source().expect("Io should expose a source");
            assert!(source.to_string().contains("boom"));
        }

        #[test]
        fn preserves_source_chain_for_inquire_custom() {
            let inner: Box<dyn std::error::Error + Send + Sync> =
                "validator failed".into();

            let mapped =
                DialogError::from(inquire::InquireError::Custom(inner));

            assert!(matches!(mapped, DialogError::BackendFailure(_)));
            assert!(mapped.to_string().contains("validator failed"));
            let source =
                mapped.source().expect("BackendFailure should expose a source");
            assert_eq!(source.to_string(), "validator failed");
        }
    }
}
