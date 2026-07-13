/// Errors returned by a [`DialogProvider`](super::DialogProvider).
#[derive(Debug, thiserror::Error)]
pub enum DialogError {
    /// The user cancelled the dialog (e.g. Esc).
    #[error("dialog user cancelled the operation")]
    UserCancelled,
    /// The user interrupted the dialog (e.g. Ctrl-C).
    #[error("dialog user interrupted the operation")]
    UserInterrupted,
    /// A selection prompt was given an empty list of options.
    ///
    /// A `select` cannot return a chosen item when there is nothing to choose
    /// from, so this is surfaced as an error rather than panicking. (A
    /// `multi_select` over an empty list is not an error — it yields an empty
    /// selection.)
    #[error("cannot select from an empty list")]
    EmptySelectionInput,
    /// The dialog configuration is invalid.
    #[error("invalid dialog configuration: {0}")]
    InvalidConfiguration(String),
    /// An I/O operation failed while prompting.
    #[error("dialog I/O operation failed: {0}")]
    Io(#[source] std::io::Error),
    /// The dialog backend reported an error.
    ///
    /// The backend error is preserved as the
    /// [`source`](std::error::Error::source) so the chain can be walked,
    /// while its concrete type stays out of this crate's public API.
    #[error("dialog backend error: {0}")]
    BackendFailure(#[source] Box<dyn std::error::Error + Send + Sync>),
    /// The I/O medium does not support interactive dialog.
    ///
    /// Returned when the backend reports that stdin is not a terminal and the
    /// caller has not provided fallback defaults. Should not occur when using
    /// [`TerminalDialogProvider`](super::TerminalDialogProvider) — its TTY
    /// guard catches this before invoking the backend.
    #[error("interactive dialog not available, stdin is not a terminal")]
    NotInteractive,
}

impl From<std::io::Error> for DialogError {
    #[inline]
    fn from(err: std::io::Error) -> Self {
        Self::Io(err)
    }
}

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
    use super::*;
    use std::error::Error as _;

    #[test]
    fn inquire_cancel_maps_to_user_cancelled() {
        let mapped =
            DialogError::from(inquire::InquireError::OperationCanceled);

        assert!(matches!(mapped, DialogError::UserCancelled));
        assert_eq!(mapped.to_string(), "dialog user cancelled the operation");
    }

    #[test]
    fn inquire_interrupt_maps_to_user_interrupted() {
        let mapped =
            DialogError::from(inquire::InquireError::OperationInterrupted);

        assert!(matches!(mapped, DialogError::UserInterrupted));
        assert_eq!(mapped.to_string(), "dialog user interrupted the operation");
    }

    #[test]
    fn inquire_not_tty_maps_to_not_interactive() {
        let mapped = DialogError::from(inquire::InquireError::NotTTY);

        assert!(matches!(mapped, DialogError::NotInteractive));
        assert_eq!(
            mapped.to_string(),
            "interactive dialog not available, stdin is not a terminal"
        );
    }

    #[test]
    fn inquire_invalid_configuration_maps_to_configuration() {
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
    fn inquire_io_preserves_source_chain() {
        let io = std::io::Error::other("boom");

        let mapped = DialogError::from(inquire::InquireError::IO(io));

        assert!(matches!(mapped, DialogError::Io(_)));
        assert!(mapped.to_string().contains("boom"));
        let source = mapped.source().expect("Io should expose a source");
        assert!(source.to_string().contains("boom"));
    }

    #[test]
    fn inquire_custom_preserves_source_chain() {
        let inner: Box<dyn std::error::Error + Send + Sync> =
            "validator failed".into();

        let mapped = DialogError::from(inquire::InquireError::Custom(inner));

        assert!(matches!(mapped, DialogError::BackendFailure(_)));
        assert!(mapped.to_string().contains("validator failed"));
        let source =
            mapped.source().expect("BackendFailure should expose a source");
        assert_eq!(source.to_string(), "validator failed");
    }
}
