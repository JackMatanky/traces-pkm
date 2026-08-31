# Query

Read-side source selection, record projection, transformations, and output
rendering.

## Language

### Sources & Grammars

#### Source Expression

A query DSL expression selecting notes by tag (`#tag`), folder prefix
(`"folder/"`), glob pattern (`"*.md"`), or File Class (`@Book*`).
*Avoid*: from_class, from_tags, from_folder, DQL source

#### Filter Expression

A boolean predicate expression evaluated against note fields to filter query
results (e.g. `rating > 7 and file.folder == "books"`).
*Avoid*: where clause, filter string

#### Query Mode

The row evaluation granularity of a query: `Pages` (one row per Note) or
`Tasks` (one row per task checklist item).
*Avoid*: query type, evaluation level

### Execution & Records

#### Query Record

A single query result row pairing file metadata with parsed note metadata, task
state, and resolved field paths.
*Avoid*: IndexRecord, QueryOutcome, QueryRow, page, record

#### Query Record Set

An ordered, indexable, and iterable collection of Query Records returned by
query execution, supporting chained transformations and formatting.
*Avoid*: QueryOutcome, QueryResult, result set

#### Pipeline Query

A template-side query composed by chaining transformation methods (`where`,
`sort`, `limit`, `group_by`, `flatten`) and terminal formatters (`table`,
`list`, `task_list`, `count`).
*Avoid*: DQL, dataview query
