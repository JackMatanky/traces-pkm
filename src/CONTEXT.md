# Core

Shared domain primitives, security boundaries, and cross-cutting types for
Traces.

## Language

### Workspace & Boundaries

#### Project Root

The workspace directory containing notes and `.traces/` configuration, against
which all file operations, template outputs, and queries are strictly confined.
*Avoid*: working directory, vault root, base folder

#### Root-Confined Path

A filesystem path guaranteed to reside within the Project Root, rejecting
absolute paths, root escapes, and parent directory (`..`) traversal.
*Avoid*: relative path, sanitized path, normalized path

### Metadata Primitives

#### Field Key

The canonical, case-insensitive identifier for note and schema metadata fields,
preserving author casing for display.
*Avoid*: property name, attribute key, column

#### Tag

A `#`-prefixed identifier supporting hierarchical sub-tag prefix matching.
*Avoid*: label, category, keyword

#### File Base

The universal filesystem metadata captured for every regular file in a project:
relative path, size, timestamps, and format classification.
*Avoid*: fs entry, file metadata, file record

### Traversal & State

#### Directory Tree

The shared filesystem traversal model for scanning directories, discovering
templates, and loading schemas with classified error handling.
*Avoid*: walker, walk adapter, walkdir

#### File State Store

The user-level persistent store managing tracked configuration paths and
trusted project roots.
*Avoid*: state cache, local database, cache dir

#### User

The human operating Traces, or an automated agent acting on their behalf.
*Avoid*: Client, operator, caller
