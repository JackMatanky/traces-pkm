//! Terminal-backed [`DialogProvider`] — interactive prompts via `inquire`.

use super::{DialogError, DialogProvider};

/// [`DialogProvider`] that prompts the user through the terminal.
///
/// Backed by [`inquire`](https://docs.rs/inquire). Falls back to defaults in
/// non-TTY contexts (CI, piping, scripts, dry-run):
///
/// | method                                         | with default        | without default   |
/// | ---------------------------------------------- | ------------------- | ----------------- |
/// | [`text`](DialogProvider::text)                 | returns the default | returns `""`      |
/// | [`confirm`](DialogProvider::confirm)           | returns the default | returns `false`   |
/// | [`select`](DialogProvider::select)             | returns index `0`   | returns index `0` |
/// | [`multi_select`](DialogProvider::multi_select) | —                   | returns `[]`      |
///
/// ## Empty-item edge case
///
/// [`select`](DialogProvider::select) short-circuits **before** the TTY check
/// when `items` is empty — it returns
/// [`EmptySelectionInput`](DialogError::EmptySelectionInput) regardless of TTY
/// status, because zero options can never yield a valid choice.
///
/// # Examples
///
/// ```
/// use traces_pkm::dialog::{DialogProvider, TerminalDialogProvider};
///
/// let p = TerminalDialogProvider::new();
/// // In non-TTY contexts all methods return their fallback defaults:
/// let name = p.text("name", Some("carol")).unwrap();
/// assert_eq!(name, "carol");
/// ```
#[derive(Copy, Clone, Debug, Default)]
pub struct TerminalDialogProvider;

impl TerminalDialogProvider {
    /// Create a [`TerminalDialogProvider`] with default configuration.
    #[inline]
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl DialogProvider for TerminalDialogProvider {
    #[inline]
    fn text(
        &self,
        label: &str,
        default: Option<&str>,
    ) -> Result<String, DialogError> {
        if !stdin_is_tty() {
            return Ok(default.unwrap_or_default().to_owned());
        }
        let mut prompt = inquire::Text::new(label);
        if let Some(d) = default {
            prompt = prompt.with_default(d);
        }
        Ok(prompt.prompt()?)
    }

    #[inline]
    fn confirm(
        &self,
        label: &str,
        default: Option<bool>,
    ) -> Result<bool, DialogError> {
        if !stdin_is_tty() {
            return Ok(default.unwrap_or(false));
        }
        let mut prompt = inquire::Confirm::new(label);
        if let Some(d) = default {
            prompt = prompt.with_default(d);
        }
        Ok(prompt.prompt()?)
    }

    #[inline]
    fn select(
        &self,
        label: &str,
        items: &[String],
    ) -> Result<usize, DialogError> {
        // A select over zero options can never yield a choice, in either mode.
        if items.is_empty() {
            return Err(DialogError::EmptySelectionInput);
        }
        if !stdin_is_tty() {
            return Ok(0);
        }
        Ok(inquire::Select::new(label, items.to_vec()).raw_prompt()?.index)
    }

    #[inline]
    fn multi_select(
        &self,
        label: &str,
        items: &[String],
    ) -> Result<Vec<usize>, DialogError> {
        if items.is_empty() {
            return Ok(Vec::new());
        }
        if !stdin_is_tty() {
            return Ok(Vec::new());
        }
        Ok(inquire::MultiSelect::new(label, items.to_vec())
            .raw_prompt()?
            .into_iter()
            .map(|op| op.index)
            .collect())
    }
}

/// Returns `true` when stdin is connected to an interactive terminal.
#[inline]
fn stdin_is_tty() -> bool {
    use is_terminal::IsTerminal as _;
    std::io::stdin().is_terminal()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn skip_if_tty(test: &str) -> bool {
        let is_tty = stdin_is_tty();
        if is_tty {
            eprintln!(
                "skipping {test}: stdin is a real TTY, cannot assert the \
                 non-TTY fallback"
            );
        }
        is_tty
    }

    use super::*;

    mod text {
        use super::*;

        #[test]
        fn returns_default_when_not_a_tty() {
            if skip_if_tty("returns_default_when_not_a_tty") {
                return;
            }
            let p = TerminalDialogProvider::new();
            assert_eq!(p.text("name", Some("carol")).unwrap(), "carol");
        }

        #[test]
        fn returns_empty_when_not_a_tty_and_no_default() {
            if skip_if_tty("returns_empty_when_not_a_tty_and_no_default") {
                return;
            }
            let p = TerminalDialogProvider::new();
            assert_eq!(p.text("name", None).unwrap(), "");
        }
    }

    mod confirm {
        use super::*;

        #[test]
        fn returns_default_when_not_a_tty() {
            if skip_if_tty("returns_default_when_not_a_tty") {
                return;
            }
            let p = TerminalDialogProvider::new();
            assert!(p.confirm("ok?", Some(true)).unwrap());
            assert!(!p.confirm("ok?", Some(false)).unwrap());
        }

        #[test]
        fn returns_false_when_not_a_tty_and_no_default() {
            if skip_if_tty("returns_false_when_not_a_tty_and_no_default") {
                return;
            }
            let p = TerminalDialogProvider::new();
            assert!(!p.confirm("ok?", None).unwrap());
        }
    }

    mod object_safety {
        use super::*;

        #[test]
        fn is_usable_as_dyn_when_not_a_tty() {
            if skip_if_tty("is_usable_as_dyn_when_not_a_tty") {
                return;
            }
            let concrete = TerminalDialogProvider::new();
            let p: &dyn DialogProvider = &concrete;
            assert_eq!(p.text("l", Some("d")).unwrap(), "d");
            assert!(p.confirm("l", Some(true)).unwrap());
        }
    }

    mod select {
        use super::*;

        #[test]
        fn returns_index_zero_when_not_a_tty() {
            if skip_if_tty("returns_index_zero_when_not_a_tty") {
                return;
            }
            let items = vec!["first".to_owned(), "second".to_owned()];
            let p = TerminalDialogProvider::new();

            assert_eq!(p.select("pick", &items).unwrap(), 0);
        }

        #[test]
        fn returns_error_when_items_are_empty() {
            let p = TerminalDialogProvider::new();

            assert!(matches!(
                p.select("pick", &[]),
                Err(DialogError::EmptySelectionInput)
            ));
        }
    }

    mod multi_select {
        use super::*;

        #[test]
        fn returns_empty_when_items_are_empty() {
            let p = TerminalDialogProvider::new();
            assert!(p.multi_select("pick", &[]).unwrap().is_empty());
        }

        #[test]
        fn returns_empty_when_not_a_tty() {
            if skip_if_tty("returns_empty_when_not_a_tty") {
                return;
            }
            let items = vec!["a".to_owned(), "b".to_owned()];
            let p = TerminalDialogProvider::new();

            assert!(p.multi_select("pick", &items).unwrap().is_empty());
        }
    }
}
