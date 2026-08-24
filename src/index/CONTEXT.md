# Index

Persistent file index, note parsing, and link graph construction.

## Language

### FileIndex

A persisted cache of metadata extracted from every file in the project root. Built by `traces index` and transparently kept fresh on every query. Two tiers: every file gets a **File Base**; markdown files additionally get **Note Metadata** (frontmatter, inline fields, tags, tasks, lists, links). Stored in a redb database.
*Avoid*: NoteIndex, database, cache, vault

### File Base

The indexed metadata for every file regardless of type: `file.path`, `file.name`, `file.folder`, `file.created_at`, `file.modified_at`, `file.size`, and `kind` (whether the file is a markdown Note or a plain file — per ADR-0005's `file_records` schema). Exposes `ctime`/`cdate`/`mtime`/`mdate` accessors for Dataview-style queries.
*Avoid*: file metadata, fs entry

### Note Metadata

The rich indexed data for markdown files only, layered on top of the File Base: frontmatter fields, inline fields (`Key:: Value`), tags, tasks, lists, and links.
*Avoid*: page data, document info

### Inlink

A Note's inbound links, derived by resolving every indexed Note's outgoing Markdown links and wikilinks against every other indexed Note's path, in a post-processing pass over Note Metadata. Persisted alongside FileBase/Note data and recomputed in full only when the index changes on refresh (never patched per-Note, since one Note's resolved target can depend on every other indexed Note); reused unchanged from the last persisted computation otherwise, so it never goes stale relative to the FileIndex. Exposed to Templates and CLI as the `inlinks` field, alongside `tags`.
*Avoid*: backlink, incoming link

### Inline Field

A `Key:: Value` pair embedded in a note's body using Dataview-compatible syntax: `Key:: Value` (start of line), `[Key:: Value]` (inline with visible key), or `(Key:: Value)` (inline with hidden key). Parsed from body text and list items, not from code blocks or inline code.
*Avoid*: metadata tag, embedded field

### IndexRecord

A single item inside a `QueryOutcome`: a Note with its implicit `file.*` fields and all indexed frontmatter/inline field metadata. A task-level row (from `tasks.*`) is the same type with `task.completed`/`task.text` also set, retaining its parent Note's metadata.
*Avoid*: QueryRow, page, record
