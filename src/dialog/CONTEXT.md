# Dialog

I/O seam separating template rendering from user interaction. Defines the object-safe contract for prompting users and collecting responses.

## Language

### DialogProvider

An object-safe trait for prompting users and collecting responses. Two implementations cover the supported runtime modes: `PresetDialogProvider` replays queued responses for tests and non-interactive MCP execution; `TerminalDialogProvider` delegates to `inquire` for real terminal interaction and returns fallback values in non-TTY contexts. Both are `Send + Sync` so shared providers can be captured by thread-safe template-rendering closures.
*Avoid*: prompt backend, input provider

### Selection by Position

`DialogProvider::select` and `DialogProvider::multi_select` return indices into the `items` slice, not copied labels. Index-based selection lets callers recover non-string values from a parallel list and keeps duplicate labels distinguishable.
*Avoid*: index-based selection, positional select
