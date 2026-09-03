# Template

Template loading, engine helper namespaces, custom functions, output path
resolution, and note rendering.

## Language

### Template Model & Resolution

#### Template Document

A Markdown document containing template syntax and custom helper calls that
produces a rendered note when instantiated.
*Avoid*: template file, template script

#### Instantiate

The process of resolving, evaluating, and writing a template to produce a note
on disk.
*Avoid*: apply, insert, compile, process

#### Template Directory

A configured directory containing templates, checked locally
(`.traces/templates/`) before falling back to user-global locations.
*Avoid*: templates folder, template location

#### Template Resolution

The lookup precedence resolving template names first as exact paths, then in
local template directories, then in global directories.
*Avoid*: template lookup, search

#### Template Variable

A static value provided in the render environment before execution (e.g.
`date`, `title`), distinct from dynamic helper functions.
*Avoid*: context parameter, render argument

#### No-Declaration Format

The design invariant that templates declare no upfront parameter manifests,
invoking interactive helpers at point of need during evaluation.
*Avoid*: declared template, template schema, manifest

### Helpers & Namespaces

#### Template Helper

A function, filter, or generator available in templates, grouped under domain
namespaces (`ui`, `file`, `date`, `query`, `tasks`, `schema`).
*Avoid*: custom function, internal function, tp helper

### Output Resolution & Writing

#### Template Output Path

The final destination path for an instantiated note, resolved by precedence:
CLI `--output` > template `file.write_to()` > configured default output
directory, confined to the Project Root.
*Avoid*: target path, destination file

#### Write Mode

The execution mode choosing whether rendered content is returned unwritten
(`DryRun`) or committed to disk (`Commit`).
*Avoid*: dry-run mode, preview mode, execution mode

#### Commit Policy

The rule governing how committed writes handle existing files: fail on
collision (`CreateNew`) or overwrite (`Overwrite`).
*Avoid*: collision policy, overwrite flag
