---
number: 1
title: Minijinja with Lazy Interactive Custom Functions
status: accepted
date: 2026-07-07
---

# Minijinja with Lazy Interactive Custom Functions

## Context and Problem Statement

The project needs a template engine that supports both declarative rendering (conditionals, loops, filters) and interactive user input (text prompts, select menus, multi-select) during template instantiation. Obsidian Templater achieves this by registering lazy closures that open modal dialogs during the render pass. Three alternatives exist: pre-collecting all input upfront (common in scaffold tools, but awkward for conditional interaction paths), embedding a scripting language like Rhai or Rune (adds runtime dependency and a new language for template authors), or using minijinja with custom functions that block on user input during rendering.

## Considered Options

- **Pre-collect** — CLI gathers all input upfront, then renders. Breaks down when templates have conditional interaction (e.g., "if type X, ask for Y"). Common in scaffold tools but awkward for complex PKM templates.
- **Scripted templates** — Templates are programs in Rhai/Lua/Rune. Adds a runtime dependency, sandboxing concerns, and a new language for template authors to learn.
- **Minijinja + lazy interactive functions** — Templates stay declarative; interactivity is provided by registered Rust closures that minijinja calls during render. No new language, no pre-collect orchestration, works out of the box with minijinja's synchronous render.

## Decision Outcome

Use minijinja for template rendering and register interactive custom functions (text prompt, select menu, multi-select) as minijinja globals that call inquire functions during render. This follows the same lazy-callable pattern as Templater: the renderer doesn't know about interactivity — it just calls registered functions that happen to prompt for user input.

### Consequences

Good, because:
- Templates stay declarative; the rendering engine remains pure
- No new scripting language for template authors to learn
- Architecture naturally extends to MCP mode: AI provides all variables upfront

Bad, because:
- Dry-run mode must detect `!is_terminal()` and return defaults from interactive functions
- Each interactive function needs a synchronous fallback path for non-interactive use

### Confirmation

The implementation uses minijinja's `Environment::add_function` to register interactive functions. Code review should verify that every interactive function has a non-interactive fallback path.
