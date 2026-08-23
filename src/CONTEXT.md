# Core

Shared types and entry points for Traces. Covers `lib.rs` (module declarations, public re-exports) and `main.rs` (process exit code mapping), plus the loose root-level files.

## Language

### User

The human operating the CLI tool. In MCP mode, the AI agent acts on the user's behalf.
*Avoid*: Client, operator
