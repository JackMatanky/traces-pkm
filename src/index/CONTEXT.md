# Index

Persistent file indexing, metadata caching, storage, and inbound link graph
construction.

## Language

### Index Model

#### File Index

An immutable in-memory snapshot of indexed files, parsed notes, and derived
inbound links across a project root.
*Avoid*: NoteIndex, database handle, cache, vault

#### Indexer Service

The service driving the index lifecycle: building fresh indexes, persisting to
disk, loading cached data, and performing differential refreshes.
*Avoid*: index manager, scanner, indexer facade

### Indexed Data

#### File Base

The base indexed metadata captured for every regular file: relative path, size,
timestamps, and format classification.
*Avoid*: file metadata, fs entry, raw record

#### Note Metadata

The rich structured data extracted from Markdown files: frontmatter, inline
fields, tags, lists, tasks, and outgoing links.
*Avoid*: page data, document info, note payload

#### Inlink

A derived inbound reference mapping a note's path to all other notes linking to
it via Markdown links or wikilinks.
*Avoid*: backlink, incoming link, reverse ref

#### Incremental Delta

The differential change set computed during refresh that compares timestamps and
patches only modified files, notes, and affected link targets.
*Avoid*: index patch, sync delta
