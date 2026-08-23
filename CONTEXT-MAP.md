# Context Map

Traces is a CLI tool for template-driven personal knowledge management. Each module under `src/` has its own `CONTEXT.md` defining the glossary and domain terms for that area.

## Contexts

| Context | Directory | Scope |
| ------- | --------- | ----- |
| Core | `src/` | Shared types, entry points, glue code |
| CLI | `src/cli/` | Commands, flags, template browser |
| Config | `src/config/` | Config files, template directories, resolution |
| Dialog | `src/dialog/` | Dialog provider trait and prompting |
| Index | `src/index/` | FileIndex, file records, note metadata, inlinks |
| Note | `src/note/` | Note parsing, output paths, template variables |
| Query | `src/query/` | Source expressions, pipeline queries, CLI query commands |
| Schema | `src/schema/` | Schemas, field definitions, inheritance, file classes |
| Template | `src/template/` | Template rendering, custom functions, instantiation |

## Reading order

When exploring a topic, read the context that owns the concept first, then cross-reference related contexts as needed. The root `docs/adr/` holds system-wide architecture decisions; individual contexts may also have `docs/adr/` for context-specific decisions.
