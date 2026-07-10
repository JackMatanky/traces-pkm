# Prompt Service

Status: ready-for-agent

## Problem Statement

Both config scaffolding and template rendering need interactive user input — text prompts, select menus, confirmations, multi-select. These must work interactively in a terminal but return sensible defaults in non-TTY contexts (dry-run, scripts, future MCP mode). The prompt logic should be testable without a physical TTY and consistent across all consumers.

## Solution

A `PromptProvider` trait (and a `TerminalPromptProvider` implementation wrapping `inquire`) that abstracts interactive input behind a seam. All interactive functions in both ConfigService and TemplateService go through this trait.

## User Stories

1. As a user, I want to be prompted for text input with an optional default value, so that templates and config init can collect freeform data.
2. As a user, I want to choose from a list of options via a select menu, so that I don't have to type exact values.
3. As a user, I want to confirm actions with y/n prompts, so that I can approve or reject operations.
4. As a user, I want to select multiple items from a list, so that I can pick several options at once.
5. As a user, I want prompts to work seamlessly in my terminal, so that the experience feels native.
6. As a developer (of templates), I want prompt functions to return sensible defaults when the tool is in non-interactive mode, so that templates render without hanging.
7. As an AI agent (via MCP), I want all prompts to be skippable by providing values upfront, so that I can instantiate templates without a terminal.
8. As a developer (of traces), I want PromptProvider to be testable with a fake implementation, so that I can write deterministic tests for prompt-dependent features.

## Implementation Decisions

- **PromptProvider trait** with methods matching the desired prompt types:
  - `text(&self, label: &str, default: Option<&str>) -> Result<String>`
  - `select(&self, label: &str, items: &[String]) -> Result<String>` _(renamed from `suggester`)_
  - `confirm(&self, label: &str, default: Option<bool>) -> Result<bool>`
  - `multi_select(&self, label: &str, items: &[String]) -> Result<Vec<String>>`
- **TerminalPromptProvider** implements the trait via `inquire`. Checks `is_terminal()` on stdin before prompting. In non-TTY mode, returns defaults without calling inquire.
- **PresetPromptProvider** (test fake) returns configured responses without any I/O. Used in tests and MCP mode.
- The trait is defined in its own small crate or module with no dependencies beyond `inquire` and `is-terminal`.
- ConfigService's `init` command receives a `&dyn PromptProvider`. TemplateService's custom functions also hold a reference to one.
- Crate: `inquire` for terminal prompts, `is-terminal` for TTY detection.

## Testing Decisions

- **PromptProvider trait** is designed for testability — tests never need a real TTY.
- **PresetPromptProvider tests**: verify it returns exactly the configured responses.
- **TerminalPromptProvider tests**: only test non-TTY fallback paths (no way to automate a real TTY in CI). Verify that when stdin is not a terminal, defaults are returned without calling inquire.
- Prior art: standard Rust trait + mock testing pattern.

## Out of Scope

- Complex multi-step wizards or chained prompts (keep it stateless — one call, one response).
- Rich terminal UI beyond inquire's capabilities (fuzzy search, async validation).
- Configuration of prompt appearance (colors, symbols) — use inquire's built-in theming.

## Further Notes

- This is the smallest and most foundational spec — build first.
- ConfigService's `init` command will use this for interactive config scaffolding.
- TemplateService's interactive functions (`prompt_text`, `select`, `confirm`, `multi_select`) will delegate to this.
