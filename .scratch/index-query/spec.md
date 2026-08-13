Status: ready-for-agent

# Template Query Spec

## Problem Statement

Traces can instantiate Templates, but it cannot yet treat the project root as a queryable personal knowledge base. A User who has many markdown Notes needs Dataview-like access to File Records, Note Metadata, Inline Fields, tasks, lists, tags, and links from both Templates and CLI commands.

Without a FileIndex, Templates cannot ask questions such as "which books match this tag?", "which unfinished tasks belong to active projects?", or "which Note should the User select from indexed metadata?" The User also cannot run quick terminal queries over the same indexed data.

## Solution

Add a FileIndex backed by redb. The FileIndex scans every file under the trusted project root, stores general File Records for all files, stores richer Note Metadata for markdown Notes, and refreshes stale entries lazily by comparing `created_at`, `modified_at`, and `size`.

Expose the FileIndex through a QueryOps minijinja namespace Object. QueryOps supports method chaining for query construction and returns QueryOutcome objects that are iterable, indexable, and usable as input to `ui.select` and `ui.multi_select`. QueryOutcome terminal methods and pipeline filters render markdown tables, lists, task lists, and counts for simple Template output. Template authors use `{% for %}` loops when they need custom value manipulation.

Expose secondary CLI query commands as top-level commands: `traces list`, `traces table`, and `traces task`. These commands use flags, not a Dataview Query Language parser.

## User Stories

1. As a User, I want Traces to index every file in my project root, so that I can query the same project I use for Templates and Notes.
2. As a User, I want every indexed file to have a File Record, so that non-markdown files can still be discovered by path, name, folder, timestamps, and size.
3. As a User, I want markdown Notes to have richer Note Metadata, so that queries can use frontmatter, Inline Fields, tags, tasks, lists, and links.
4. As a User, I want the FileIndex to persist to disk, so that query commands do not need to parse the entire project from scratch every time.
5. As a User, I want the FileIndex to refresh stale entries automatically, so that I do not need to remember to run `traces index` before querying.
6. As a User, I want a `traces index` command, so that I can explicitly build or rebuild the FileIndex when I want to.
7. As a User, I want `created_at`, `modified_at`, and `size` stored for each File Record, so that Traces can detect changed files cheaply.
8. As a User, I want `ctime`, `cdate`, `mtime`, and `mdate` accessors, so that Dataview-style metadata expressions feel familiar.
9. As a User, I want frontmatter metadata indexed from markdown Notes, so that existing YAML note metadata becomes queryable.
10. As a User, I want Dataview-compatible Inline Fields indexed, so that `Key:: Value`, `[Key:: Value]`, and `(Key:: Value)` work in Traces Notes.
11. As a User, I want Inline Fields preserved in Note contents, so that source markdown remains compatible with other tools.
12. As a User, I want Inline Fields ignored inside fenced code blocks, so that code examples do not accidentally become Note Metadata.
13. As a User, I want Inline Fields ignored inside indented code blocks, so that markdown code blocks remain literal text.
14. As a User, I want Inline Fields ignored inside inline code spans, so that prose examples do not accidentally become Note Metadata.
15. As a User, I want Inline Fields parsed from normal body text, so that body metadata is queryable.
16. As a User, I want Inline Fields parsed from list items, so that task and list context can carry metadata.
17. As a User, I want markdown tasks indexed, so that I can query unfinished and completed work across Notes.
18. As a User, I want basic task completion status indexed from CommonMark task markers, so that `- [ ]` and `- [x]` tasks can be queried.
19. As a User, I want markdown lists indexed, so that list content can be queried and later flattened.
20. As a User, I want markdown links and wikilinks indexed as outlinks, so that relationships between Notes become queryable.
21. As a User, I want inlinks derived from outlinks, so that I can find Notes that link to a target Note.
22. As a User, I want markdown tags indexed, so that I can query sources such as `#book` or `#projects/active`.
23. As a Template author, I want a `query` namespace, so that query features live beside existing `file`, `ui`, and `date` namespaces.
24. As a Template author, I want QueryOps method chaining, so that I can write readable query construction inside Templates.
25. As a Template author or CLI User, I want a single `from([expr])` source selection seam (`query.from` / `tasks.from` / CLI `--from`), so that I can filter source pages using tags (`#book`), paths (`books/`), classes (`@Book`), and boolean logic (`and`, `or`, `not`).
26. As a User, I want class queries to support Incremental Depth (`@Book` for exact, `@Book+` for direct children, `@Book*` for all descendants), so that I have precise control over subclass matching.
27. As a Template author, I want QueryOutcome to support `.where(...)` and `.filter(...)`, so that I can narrow query results.
28. As a Template author, I want QueryOutcome to support `.sort(...)`, so that I can order query results.
29. As a Template author, I want QueryOutcome to support `.limit(...)`, so that I can cap query output.
30. As a Template author, I want QueryOutcome to support `.group_by(...)`, so that I can group Notes by metadata.
31. As a Template author, I want QueryOutcome to support `.flatten(...)`, so that nested list-like metadata can become rows.
32. As a Template author, I want QueryOutcome to be iterable in `{% for %}` loops, so that I can fully control rendered markdown.
33. As a Template author, I want QueryOutcome to be indexable, so that I can retrieve the item selected by a User.
34. As a Template author, I want QueryOutcome to expose `len`, so that I can branch on the number of query results.
35. As a Template author, I want IndexRecord values to expose `file.*` fields, so that implicit metadata is available in Templates.
36. As a Template author, I want IndexRecord values to expose frontmatter fields, so that Note-specific metadata is available in Templates.
37. As a Template author, I want IndexRecord values to expose Inline Fields, so that body metadata is available in Templates.
38. As a Template author, I want QueryOutcome to work with `ui.select`, so that a query can provide the input choices for an Interactive Function.
39. As a Template author, I want QueryOutcome to work with `ui.multi_select`, so that a query can provide multiple selectable Note or task choices.
40. As a Template author, I want `ui.select` to use attribute labels from QueryOutcome items, so that the User sees meaningful choices.
41. As a Template author, I want terminal query helpers to output markdown tables, so that simple tabular query output can be embedded in rendered Notes.
42. As a Template author, I want terminal query helpers to output markdown lists, so that simple list query output can be embedded in rendered Notes.
43. As a Template author, I want terminal query helpers to output markdown task lists, so that task queries can be embedded in rendered Notes.
44. As a Template author, I want terminal query helpers to output counts, so that Templates can include summary values.
45. As a Template author, I want pipeline terminal filters such as `| table(...)`, so that simple rendering reads naturally in minijinja.
46. As a Template author, I want terminal methods such as `.table(...)`, so that method-chained query output also works.
47. As a Template author, I want `{% for %}` loops to remain the transformation escape hatch, so that I can apply minijinja filters like `upper` to individual values.
48. As a Template author, I want clear template diagnostics when a query method or filter fails, so that I can fix the exact Template line and column.
49. As a CLI User, I want `traces list`, so that I can print page-level bullet list query output in the terminal.
50. As a CLI User, I want `traces table`, so that I can print page-level tabular query output in the terminal.
51. As a CLI User, I want `traces task`, so that I can print task-level query output in the terminal.
52. As a CLI User, I want query commands to use flags, so that command usage follows the project CLI guidelines and stays scriptable.
53. As a CLI User, I want query command primary output on stdout, so that I can pipe it to other tools.
54. As a CLI User, I want diagnostics on stderr, so that machine-readable output is not polluted.
55. As a CLI User, I want table output to be readable in a terminal, so that common queries are useful without additional tools.
56. As a CLI User, I want JSON output available later if needed, so that automation can consume structured query results.
57. As a maintainer, I want the FileIndex behind a small interface, so that persistence, parsing, and freshness can change internally without spreading across callers.
58. As a maintainer, I want parser behavior tested at the markdown event seam, so that edge cases do not require redb or CLI setup.
59. As a maintainer, I want QueryOps tested through TemplateEngine rendering, so that tests cover the same seam Template authors use.
60. As a maintainer, I want CLI behavior tested through command dispatch, so that tests cover config loading, trust, index refresh, and output together.

## Implementation Decisions

- The canonical index module is FileIndex, not NoteIndex, because it indexes all files and only adds Note Metadata for markdown Notes.
- FileIndex persists to redb.
- FileIndex stores File Records for every file under the project root.
- FileIndex stores Note Metadata for markdown Notes only.
- FileIndex uses two redb tables: one keyed by path for File Records, one keyed by path for Note Metadata.
- File Records include `file.path`, `file.name`, `file.folder`, `file.created_at`, `file.modified_at`, and `file.size`.
- File Records expose Dataview-style accessors for `ctime`, `cdate`, `mtime`, and `mdate`.
- FileIndex freshness uses `(created_at, modified_at, size)` comparison.
- Query execution lazily refreshes stale FileIndex entries before returning data.
- `traces index` explicitly builds or rebuilds the FileIndex.
- A future file-watching mode is allowed but out of scope for this spec.
- Note Metadata includes frontmatter, Inline Fields, tags, tasks, lists, and links.
- Inline Fields use Dataview-compatible syntax: `Key:: Value`, `[Key:: Value]`, and `(Key:: Value)`.
- Inline Fields are parsed from body text and list items.
- Inline Fields are not parsed inside fenced code blocks, indented code blocks, or inline code spans.
- `pulldown-cmark` is the markdown event parser for CommonMark/GFM structure.
- `pulldown-cmark` options should include task lists, YAML metadata blocks, and wikilinks when extracting those features.
- Parser extraction should use markdown events and source ranges where useful, not ad hoc whole-file regexes.
- A custom lexer may still be needed for Dataview Inline Field syntax and custom task metadata beyond `pulldown-cmark` events.
- Inlinks are derived in a post-processing pass from indexed outlinks.
- Query execution lives in a top-level `src/query/` domain module, establishing Command-Query Separation (CQS) between index persistence (`src/index/`) and query execution (`src/query/`).
- Template queries and CLI queries share a single unified `from([expr])` entry point (`QueryOps::from`), replacing separate `.all()`, `.from_tags()`, `.from_folder()`, and `.from_class()` methods.
- `query.from()` or `query.from("")` evaluates to `QuerySource::All`.
- QueryOps follows the existing namespace Object pattern used by `file`, `ui`, and `date`.
- QueryOps methods return QueryOutcome Objects.
- QueryOutcome supports methods including `where`, `filter`, `sort`, `limit`, `group_by`, and `flatten`.
- QueryOutcome supports terminal methods including `table`, `list`, `task_list`, and `count`.
- Terminal operations are also exposed as pipeline filters when that reads better in a Template.
- Non-terminal operations accept and return QueryOutcome.
- Terminal operations accept QueryOutcome and return rendered markdown or scalar values.
- Terminal table/list helpers accept field path strings, not arbitrary minijinja expression strings.
- Template authors use `{% for %}` loops for value transformations such as `file.name | upper`.
- QueryOutcome is iterable in minijinja loops.
- QueryOutcome is indexable by integer position.
- QueryOutcome can be passed directly to `ui.select` and `ui.multi_select`.
- `ui.select` and `ui.multi_select` continue to use `attribute=` to derive display labels.
- Task queries operate at the task level, not the page level.
- CLI query commands are top-level commands: `list`, `table`, and `task`.
- CLI query commands use flags, not a DQL parser.
- Calendar queries are out of scope.
- ADR 5 records the FileIndex, QueryOps namespace, and pipeline terminal filter decisions.

## Testing Decisions

- Tests should cover external behavior at module interfaces, not internal implementation details.
- The preferred highest seam is CLI command dispatch, using isolated trusted project roots.
- CLI tests should verify `traces index`, `traces list`, `traces table`, `traces task`, and `traces template` behavior from parsed command arguments through output.
- FileIndex tests should use the FileIndex interface: build, refresh, load, and query from a project root.
- FileIndex tests should verify redb persistence through observable reload behavior, not by inspecting redb internals.
- FileIndex tests should verify freshness by changing file metadata/content and observing that stale entries refresh.
- Template tests should use TemplateEngine rendering as the seam.
- Template tests should verify QueryOps namespace registration, method chaining, QueryOutcome iteration, terminal filters, and `ui.select` integration through rendered output.
- Template diagnostics tests should assert that query failures surface as useful render errors with template context.
- Parser tests should use a Markdown metadata extraction module seam.
- Parser tests should verify behavior over `pulldown-cmark` events: metadata blocks, lists, items, task markers, wikilinks, and code-span/code-block exclusion.
- Parser tests should verify Inline Field extraction from body text and list items.
- Parser tests should verify Inline Fields are ignored inside `Event::Code` and `Tag::CodeBlock` regions.
- Existing CLI dispatch tests are prior art for command-level testing.
- Existing TemplateService and TemplateEngine tests are prior art for rendering-level testing.
- Existing `ui.select` tests are prior art for preserving original values while deriving display labels with `attribute=`.

## Out of Scope

- A Dataview Query Language parser.
- DataviewJS or arbitrary scripting.
- Calendar query output.
- A background file watcher or daemon.
- Obsidian-specific metadata such as starred files.
- Editing Notes through query output.
- Stripping Inline Fields from rendered or authored markdown.
- Full Obsidian Tasks plugin compatibility in the first slice.
- JSON output unless a later ticket explicitly adds it.
- Rich terminal table rendering beyond the minimum useful table output for the first CLI slice.

## Further Notes

- The first implementation tickets should preserve a deep FileIndex interface and avoid leaking redb table details to CLI or Template modules.
- The parsing stage should be split carefully: `pulldown-cmark` provides markdown structure, but Dataview Inline Field syntax likely remains custom logic layered on top of event ranges.
- Terminal helpers are deliberately convenience features. Complex formatting belongs in Template loops.
- The term FileIndex replaces NoteIndex everywhere in new design work.
- ADR 5 is proposed and should be reviewed before acceptance.
- `QuerySource` uses a composable `QuerySourceExpr` AST (`Tag`, `Path`, `Class`, `And`, `Or`, `Not`), parsed via Logos tokenizer and recursive-descent parser.
- Class querying uses an Incremental Depth Mental Model: `ClassExpansionMode::Exact` (`@Book` / `class(Book)` — self only), `ClassExpansionMode::Children` (`@Book+` / `class(Book).with_children()` — self + direct children), and `ClassExpansionMode::Descendants` (`@Book*` / `class(Book).with_descendants()` — self + transitive descendants).
- Class expansion is evaluated in a caller-side AST pre-pass (`resolve_sources`) against `SchemaRegistry`, populating `ClassExpansionMode`'s `BTreeSet<String>` match set and keeping `src/query/` independent of `src/schema/`.
