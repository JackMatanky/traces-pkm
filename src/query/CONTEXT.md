# Query

Read-side source selection, record projection, transformations, and output rendering.

## Language

### Query Source

A parsed `--from` expression selecting Notes by tag, folder, or File Class.
A File Class selector names frontmatter values only; Schema is-a expansion is an
optional live matching convenience, never part of parsing the Query Source.
*Avoid*: resolved source, schema query

### Source Expression

A query source parsed by the shared CLI/template DSL. Leaves select tags (`#book`), exact files, folder prefixes (`"books/"`), glob patterns (`"covers/*.jpg"`, `"covers/**/*.jpg"` — `*` excludes `/`, `**` includes it), or File Classes (`@Book` exact, `@Book+` with direct children, `@Book*` with transitive descendants). `and`/`&&`, `or`/`||`, `not`/`!`, and parentheses compose leaves. `class(Book)`, `class(Book).with_children()`, and `class(Book).with_descendants()` are equivalent long forms. An unknown File Class degrades to exact matching with a warning.
*Avoid*: from_class, from_tags, from_folder, DQL source

### QueryRecordSet

The type returned by a page-level (`query`) or task-level (`tasks`) query: an iterable, indexable collection of `QueryRecord`s. Supports `len`, indexing by integer position, and iteration via `{% for %}`. Registered as a minijinja type so pipeline filters compose against it.
*Avoid*: QueryOutcome, QueryResult, result set

### Pipeline Query

A template-side query composed by chaining methods on the `query` namespace Object and the `QueryRecordSet` values it returns. Non-terminal methods (`where`/`filter`, `sort`, `limit`, `group_by`, `flatten`) accept and return a `QueryRecordSet`; terminal methods (`table`, `list`, `task_list`, `count`) accept a `QueryRecordSet` and return a string. Terminal methods are also exposed as pipeline filters for template authors who prefer that syntax; non-terminal transformations are method calls only — there is no pipeline-filter form of `where`/`sort`/`limit`/`group_by`/`flatten`.

`query.from([expr])` is the single page-level entry point; `tasks.from([expr])` applies the same Source Expression before expanding each matched Note's `- [ ]`/`- [x]` items into one row per task. Calling `from()` or `from("")` selects every indexed Note. Task rows expose `task.completed` (bool) and `task.text` (string) alongside their Note's `file.*`, frontmatter, inline-field, and tag metadata.

```jinja
{% for note in query.from("#book and not archive/").where("rating > 7").sort("rating", true) %}
  - {{ note.file.name }} ({{ note.rating }})
{% endfor %}

{{ query.from("@Book*") | table(["Name", "Rating"], ["file.name", "rating"]) }}

{% set books = query.from("@Book+") %}
{% set idx = ui.select("Pick", books | map(attribute="file.name")) %}
{{ books[idx].file.name }}

{% for t in tasks.from("#projects").where("task.completed == false") %}
  - [ ] {{ t.task.text }} ({{ t.file.name }})
{% endfor %}
```

*Avoid*: DQL, dataview query

### CLI Query Commands

#### list

`traces list --from "#book and not archive/" --where "rating > 7"`: page-level bullet list output.

#### table

`traces table "rating, author" --from "#book" --sort "rating" --desc`: page-level tabular output.

#### task

`traces task --from "#projects" --where "task.completed == false"`: task-level output (operates on individual tasks, not pages).
