# Query

Read-side source selection, row projection, transformations, and output
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

### Execution & Rows

#### Query Builder

The declarative specification of a query: Query Mode, Source Expression, and
the pending Query Plan, built before execution.
*Avoid*: query request, request

#### Query Plan

The ordered transform sequence a query applies to rows, fused where possible
(adjacent filters, adjacent sorts, and Sort followed by Limit) and executed
lazily on first read.
*Avoid*: plan steps, ops list

#### Sort Order

The ordered sequence of field paths and directions defining a query's sorting
criteria, defaulting to descending order. Adjacent Sort operations in a Query
Plan fuse into a single composite Sort Order.
*Avoid*: sort spec, sort criteria, sort clause, order by string

#### Query Row

A single query result row pairing a Note with its File Base, task state, and
resolved field paths.
*Avoid*: Query Record, record, IndexRecord, QueryOutcome, page

#### Query Set

A query result set of evaluated Query Rows, usable directly or like a CTE as
an intermediate result set: cloning shares the memoized rows, chained
transforms append in `O(1)`, and reads flush the pending plan once. Transform
methods (`where`/`filter`, `sort`, `limit`, `group_by`, `flatten`,
`with_children`, `with_descendants`) chain; terminal methods (`table`, `list`,
`task_list`, `count`) render output.
*Avoid*: Query Record Set, QueryOutcome, pipeline query, DQL, dataview query

#### Task Path Style

Whether task list output appends each row's file path in parentheses (`Suffix`)
or omits it (`None`).
*Avoid*: path display, path suffix toggle
