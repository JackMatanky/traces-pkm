# Index

Persistent file indexing, metadata caching, storage, and inbound link graph
construction.

## Language

### Index Model

#### Workspace Index

An immutable in-memory snapshot of indexed files, parsed notes, and derived
inbound links across a project root.
*Avoid*: File Index, NoteIndex, database handle, cache, vault

#### Index Store

The durable on-disk cache of file entries, parsed notes, and derived
relationships for one project root, distinct from the in-memory Workspace Index.
*Avoid*: database, persistence layer, cache handle

#### Indexer Service

The service driving the index lifecycle: building fresh indexes, persisting to
disk, loading cached data, and performing differential refreshes.
*Avoid*: index manager, scanner, indexer facade

#### Index Update

The computed change set and recomputed state from one incremental
synchronization, not yet applied.
*Avoid*: sync outcome, reconciliation result, sync delta

#### Refresh Pass

One comparison-and-reconciliation cycle between current project files and the
persisted index, producing either an unchanged result or an Index Update.
*Avoid*: sync pass, scan pass, refresh transaction

#### Refresh Report

The changed-file and changed-link counts observed during a Refresh Pass.
*Avoid*: sync report, update result, refresh outcome

### Indexed Data

#### File Base

The filesystem metadata captured for every regular file regardless of document
type: relative path, size, timestamps, and format classification.
*Avoid*: file metadata, fs entry, raw record

#### File Entry

One indexed file's File Base together with its optional parsed Note and derived
inbound links in a Workspace Index.
*Avoid*: file record, indexed note

#### Note

The indexed form of a Markdown note: the parsed frontmatter, inline fields,
tags, lists, tasks, and outgoing links persisted for querying, distinct from
the File Base carrying its file metadata.
*Avoid*: note metadata, page data, document info, note payload

#### Inlink

A derived inbound reference mapping a note's path to all other notes linking to
it via Markdown links or wikilinks.
*Avoid*: backlink, incoming link, reverse ref

#### Incremental Delta

The differential change set computed during refresh that compares timestamps and
patches only modified files, notes, and affected link targets.
*Avoid*: index patch, sync delta
