# Note

Markdown note parsing, YAML frontmatter extraction, and task processing.

## Language

### Note

A markdown file on disk. Distinct from a Template — the majority of Notes are authored directly, not produced by template instantiation.
*Avoid*: Output file, document, page

### Template Output Path

The final path on disk where an instantiated note is written. Resolved by precedence: explicit CLI `--output` / `-o` flag > template `file.write_to(path)` declaration > config-derived default output directory (`output_dir` + template name stem). All non-default candidates are confined to the project root. If the resolved path already exists and `--force` is not set, an interactive prompt asks the user for a root-relative alternative path.

#### file.write_to(path)

A method on the `file` namespace object, callable from within a template to
declare the note's output path. Takes effect when the CLI's `-o` flag is
not passed; an explicit `-o` overrides it. Mirrors Obsidian Templater's
`tp.file.move()` pattern. `path` is confined to the project root — an
absolute path or a `..` segment is rejected, never written.
*Avoid*: set_output, move_to, set_destination
