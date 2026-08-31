# Note

Markdown note parsing, YAML frontmatter extraction, inline fields, links, tags,
and task list processing.

## Language

### Document Model

#### Note Document

A parsed Markdown document capturing page-level frontmatter, lists, outgoing
links, inline fields, and tags in document order.
*Avoid*: Output file, document, page, markdown file

### Metadata & Fields

#### Frontmatter

Structured key-value pairs parsed from YAML frontmatter between `---`
delimiters at the start of a note.
*Avoid*: YAML header, metadata block, document properties

#### Inline Field

A `Key:: Value` pair embedded directly in note text or list items using Dataview
syntax (`Key:: Value`, `[Key:: Value]`, or `(Key:: Value)`).
*Avoid*: metadata tag, embedded field, body attribute

#### Note Field Value

A typed metadata value parsed from frontmatter or inline fields: scalar, ISO
date, link, list, or nested object.
*Avoid*: dynamic value, field payload

### Content Elements

#### Wikilink

An Obsidian-style internal link (`[[target|alias]]`) referencing another note
by name or relative path.
*Avoid*: internal link, page link, wiki ref

#### Link

An outgoing reference extracted from standard Markdown `[text](target)` or
wikilink syntax.
*Avoid*: outlink, reference, hyper-link

#### Task

A checklist item carrying completion status (`- [ ]` or `- [x]`), description
text, and optional date-shorthand emoji markers.
*Avoid*: todo item, checkbox line, action item

#### Tag

A `#`-prefixed identifier extracted from body text and frontmatter supporting
hierarchical sub-tags (e.g. `#projects/active`).
*Avoid*: hashtag, category, label
