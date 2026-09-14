---
number: 1
title: Minijinja with Lazy Interactive Custom Functions
status: accepted
date: 2026-07-07
links:
  - target: 6
    kind: relatesto
  - target: 7
    kind: relatesto
  - target: 3
    kind: relatesto
---

# Minijinja with Lazy Interactive Custom Functions

## Context

The project needs a template engine that supports both declarative rendering
(conditionals, loops, filters) and interactive user input (text prompts, confirm
dialogs, select menus, multi-select) during template instantiation. Three
alternatives exist: pre-collecting all input upfront (common in scaffold tools,
but awkward for conditional interaction paths), embedding a scripting language
like Rhai or Rune (adds runtime dependency and a new language for template
authors to learn), or using minijinja with namespace Object helpers that call
interactive functions during render.

## Decision

Use minijinja for template rendering. Register interactive and utility helpers as
namespace globals using add_global with the Object trait, exposing methods via
get_value dispatch. The ui namespace provides four interactive functions:
text_input, confirm, select, and multi_select. All interactive functions are
routed through a DialogProvider trait (object-safe, Send + Sync) that separates
interactive prompting from template rendering. Two implementations:
TerminalDialogProvider (real TTY with is_terminal() fallback to defaults) and
PresetDialogProvider (queue-based replay for tests, MCP, and CI).

--dry-run controls WriteMode (disk writes only). --no-input swaps the provider
to an empty PresetDialogProvider so all ui.\* calls fall through to defaults.
These are independent concerns: dry-run does not suppress prompts. The
is_terminal() check lives in TerminalDialogProvider, not in the template engine.

## Consequences

Good, because:

- Templates stay declarative; the rendering engine remains pure
- No new scripting language for template authors to learn
- DialogProvider abstraction enables test, MCP, and CI without a TTY
- Architecture naturally extends to MCP mode: AI provides all variables upfront
- Object trait dispatch allows namespaced access (ui.\*, file.\*, query.\*, etc.)

Bad, because:

- PresetDialogProvider adds queue-management complexity for test/MCP scenarios
- Render/write phase separation (RenderedTemplate) adds an intermediate type
- Debug mode is always-on to support line:column diagnostic locations on render
  errors
- Template authors must learn the ui.\* namespace rather than bare function
  names

Registration table:

- file -- add_global (Object) -- src/template/engine/file.rs
- query -- add_global (Object) -- src/template/engine/query.rs
- tasks -- add_global (Object) -- src/template/engine/query.rs
- ui -- add_global (Object) -- src/template/engine/ui.rs
- date -- add_global (Object) -- src/template/engine/date.rs
- schema -- add_global (Object) -- src/template/engine/schema.rs
- uuid -- add_function -- src/template/engine.rs
- string filters -- add_filter -- src/template/engine/string.rs
- num filters -- add_filter -- src/template/engine/num.rs
- yaml filters -- add_filter -- src/template/engine/yaml.rs

The DialogProvider trait (src/dialog/mod.rs:35) defines: is_interactive()
(default true), text(label, default), confirm(label, default), select(label,
items), multi_select(label, items). All return DialogResult. Object-safety is
verified by compile-time tests asserting Arc of dyn DialogProvider compiles.

### Confirmation

DialogProvider trait is defined in src/dialog/mod.rs:35 with object-safety
verified by provider_is_send_and_sync and is_usable_as_dyn_dialog_provider
tests. Interactive functions are registered in src/template/engine/ui.rs:34
(METHODS array) and wired into the engine at src/template/engine.rs:144.
TerminalDialogProvider (src/dialog/terminal.rs) checks stdin_is_tty() and
returns defaults for non-TTY. PresetDialogProvider (src/dialog/preset.rs)
replays queued responses. The test
dry_run_still_uses_the_injected_provider_for_ui_calls
(src/template/service.rs:1152) proves dry-run does not suppress prompts.
