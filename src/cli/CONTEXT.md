# CLI

Command-line interface definitions, argument parsing, command dispatch, and
diagnostic formatting.

## Language

### Operations

#### Template Instantiation

The primary operation rendering a template into a note on disk or stdout,
invoked via `template` (or `tmpl`).
*Avoid*: run template, apply template, new note

#### Config Scaffolding

The interactive setup process initializing a `.traces/` directory and default
configuration, invoked via `init`.
*Avoid*: project setup, bootstrap

#### Trust Management

The administrative operations granting (`trust`), inspecting, or revoking
(`untrust`) execution permissions for project roots.
*Avoid*: authorization, permissions

#### Project Reindexing

The operation scanning all files under a project root and updating the
persisted metadata index, invoked via `index`.
*Avoid*: scan, reindex, refresh

#### Query Commands

Commands executing read queries against indexed notes: bullet lists (`list`),
tabular views (`table`), and task checklists (`task`).
*Avoid*: search commands, DQL commands

### Interaction & Outcomes

#### Template Browser

An interactive selector listing all available templates when no template name
is provided.
*Avoid*: template picker, template selector, fuzzy finder

#### Command Outcome

The final result of command execution, distinguishing normal completion from a
deliberate user abort.
*Avoid*: exit status, success, return code

#### User Abort

An intentional user cancellation (Escape) or interruption (Ctrl-C) ending an
interactive prompt cleanly.
*Avoid*: prompt error, failure, prompt cancellation
