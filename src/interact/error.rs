/// Errors returned by a [`PromptProvider`](super::PromptProvider).
#[derive(Debug, thiserror::Error)]
pub enum PromptError {
    /// The user cancelled or interrupted the prompt (e.g. Esc or Ctrl-C).
    #[error("prompt was interrupted")]
    Interrupted,
    /// A selection prompt was given an empty list of options.
    ///
    /// A `select` cannot return a chosen item when there is nothing to choose
    /// from, so this is surfaced as an error rather than panicking. (A
    /// `multi_select` over an empty list is not an error — it yields an empty
    /// selection.)
    #[error("no options to select from")]
    EmptyOptions,
    /// An I/O error occurred while prompting.
    #[error("prompt I/O error")]
    Io(#[source] std::io::Error),
    /// The prompt failed for another reason reported by the backend.
    ///
    /// The backend error is preserved as the
    /// [`source`](std::error::Error::source) so the chain can be walked,
    /// while its concrete type stays out of this crate's public API.
    #[error("prompt failed")]
    Backend(#[source] Box<dyn std::error::Error + Send + Sync>),
}

impl From<std::io::Error> for PromptError {
    #[inline]
    fn from(err: std::io::Error) -> Self {
        Self::Io(err)
    }
}

impl From<inquire::InquireError> for PromptError {
    #[inline]
    fn from(err: inquire::InquireError) -> Self {
        use inquire::InquireError as E;
        match err {
            // The TTY guard in `TerminalPromptProvider` means `inquire` is only
            // invoked with a terminal present, so `NotTTY` is not expected
            // here; treat it like any other non-completion as an
            // interruption.
            E::OperationCanceled | E::OperationInterrupted | E::NotTTY => {
                Self::Interrupted
            }
            E::IO(io) => Self::Io(io),
            E::InvalidConfiguration(msg) => Self::Backend(msg.into()),
            E::Custom(err) => Self::Backend(err),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::error::Error as _;

    #[test]
    fn inquire_cancel_maps_to_interrupted() {
        let mapped =
            PromptError::from(inquire::InquireError::OperationCanceled);

        assert!(matches!(mapped, PromptError::Interrupted));
    }

    #[test]
    fn inquire_io_preserves_source_chain() {
        let io = std::io::Error::other("boom");

        let mapped = PromptError::from(inquire::InquireError::IO(io));

        assert!(matches!(mapped, PromptError::Io(_)));
        let source = mapped.source().expect("Io should expose a source");
        assert!(source.to_string().contains("boom"));
    }

    #[test]
    fn inquire_custom_preserves_source_chain() {
        let inner: Box<dyn std::error::Error + Send + Sync> =
            "validator failed".into();

        let mapped = PromptError::from(inquire::InquireError::Custom(inner));

        let source = mapped.source().expect("Backend should expose a source");
        assert_eq!(source.to_string(), "validator failed");
    }
}
