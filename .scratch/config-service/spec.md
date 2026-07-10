# Config Service

Status: ready-for-agent

## Problem Statement

The CLI tool needs to know where to find templates, where to write notes, and which template directories are safe to execute. These settings must be discoverable across two levels (project-local and user-global), persist across sessions, and be auditable. Currently no configuration infrastructure exists.

## Solution

A `ConfigService` component that discovers, loads, and serves configuration from two TOML config files (local `.traces/config.toml`, global `~/.config/traces/config.toml`) and a trust store (`~/.local/share/traces/trusted/`). Exposes template directory resolution and trust checking to consumers like the `TemplateService`.

Two CLI commands bootstrap and manage the config:
- `traces init` — interactively scaffold `.traces/config.toml` + `.traces/templates/`
- `traces trust` — mark a directory as trusted (or `traces trust list`/`traces trust clean`)

## User Stories

1. As a user, I want to run `traces init` and be guided through setting up my project's `.traces/` directory, so that I can get started without reading documentation.
2. As a user, I want `traces init` to use inquire to ask me about configuration options, so that I can customize my setup interactively.
3. As a user, I want `traces init` to create a `.traces/config.toml` with sensible defaults, so that I can start using templates immediately.
4. As a user, I want `traces init` to create an empty `.traces/templates/` directory, so that I have a place to put my first template.
5. As a user, I want the tool to discover my local `.traces/config.toml` automatically when I run from a project directory, so that I don't have to pass config paths manually.
6. As a user, I want the tool to discover my global `~/.config/traces/config.toml` as a fallback, so that I can set defaults across all projects.
7. As a user, I want local config to override global config when both exist, so that I can override global defaults per project.
8. As a user, I want to configure a custom template directory via `[templates].directory` in my local config, so that I can keep templates outside `.traces/templates/`.
9. As a user, I want to configure a default output directory via `[templates].output_dir` in my local config, so that notes go where I want without passing `-o` every time.
10. As a user, I want templates to resolve in priority order (exact path → local template directory → global template directory), so that I can override global templates locally.
11. As a user, I want an informative error when a template name matches multiple files, so that I know which one the tool found and can disambiguate.
12. As a user, I want to run `traces trust` from within a template directory to mark it safe, so that I can execute templates from that location without further prompts.
13. As a user, I want `traces trust <path>` to trust a specific directory from anywhere, so that I can trust directories without cd-ing into them.
14. As a user, I want `traces trust list` to show all trusted directories, so that I can audit what I've authorized.
15. As a user, I want `traces trust clean` to remove stale trust entries for deleted directories, so that my trust store doesn't accumulate cruft.
16. As a user, I want the tool to refuse to instantiate templates from an untrusted directory, so that I don't accidentally execute code from unknown sources.
17. As a user, I want the error for an untrusted directory to include a suggestion to run `traces trust`, so that I know how to resolve it.
18. As an AI agent (via MCP), I want ConfigService to expose programmatic access to config and trust state, so that I can manage configuration on behalf of the user.

## Implementation Decisions

- **ConfigService** is a struct with methods for loading config, resolving template directories, and checking trust. It owns the parsed config and provides read-only access to consumers.
- **Config loading** walks up from `cwd` looking for `.traces/config.toml`, then falls back to `~/.config/traces/config.toml`. Local overrides global via merge (global loaded first, local overlaid on top).
- **Config schema** is TOML with a single `[templates]` table. Local config has `directory` (String) and `output_dir` (String, defaults to cwd). Global config has only `directory`. Both optional.
- **Trust storage** follows the mise pattern from `src/config/tracking.rs`: a symlink (or plain file on Windows) in `~/.local/share/traces/trusted/` named by SHA-256 hash of the canonical directory path, pointing back to the directory.
- **Template directory resolution**: exact filesystem path → path in local template directory → path in global template directory. First match wins. Multiple matches at the same priority level error with candidates listed.
- **`init` command** uses `PromptProvider` (see PromptService spec) to interactively configure `[templates].directory` and `[templates].output_dir`, then writes the config file and creates `.traces/templates/`.
- **`trust` command** with no args uses `cwd`. With an arg, trusts that path. `trust list` reads symlinks from the trust store. `trust clean` removes dangling symlinks.
- **Registry**: `mise` crate config tracking tracking pattern via `config/tracking.rs`.
- **Error reporting**: miette for rich diagnostics. Untrusted directory errors show the path and suggest `traces trust`. Config parse errors show the TOML parse error with context.

## Testing Decisions

- Tests should only test external behavior: given a filesystem state, does ConfigService return the right config / trust state / resolved path?
- **ConfigService tests**: load config with various file layouts, verify merge priority, verify error on invalid TOML.
- **Template directory resolution tests**: given a set of directories, verify resolution order, verify error on multiple matches.
- **Trust tests**: verify trust is created, listed, and cleaned correctly. Verify untrusted directories are rejected with the right error.
- **CLI command integration tests**: verify `init` creates the expected files, verify `trust` creates the expected symlink.
- No prior art in this repo (new project). Tests use temp directories created per test.

## Out of Scope

- The template rendering pipeline (delegated to TemplateService spec).
- User-defined variables in config (future).
- Folder templates / template-specific settings (future, per Templater settings).
- Config file watching or hot-reload.
- Prompt logic and TTY detection (delegated to PromptService spec — PromptProvider is a dependency).

## Further Notes

- `traces init` and `traces trust` are the only two CLI commands in this spec. The `template` command belongs to the TemplateService spec.
- ConfigService depends on PromptProvider (see PromptService spec) for the `init` command's interactive scaffolding.
- ConfigService is a dependency of TemplateService but can be built and tested independently.
- The `dirs` crate for XDG directory resolution.
- The `sha2` crate for directory path hashing in the trust store (or reuse if mise's approach uses a simpler hash).
