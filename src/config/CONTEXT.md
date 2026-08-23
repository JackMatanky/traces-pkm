# Config

Project configuration loading, discovery, TOML parsing, and trust verification.

## Language

### Config File

TOML files at two levels. Local (`.traces/config.toml`) and global (`~/.config/traces/config.toml`). The `[templates]`, `[schemas]`, and `[frontmatter]` tables are defined:

```toml
[templates]
# Either level: replaces the default templates directory for that level
# directory = ""

# Local only: overrides default output directory (defaults to cwd)
# output_dir = ""

[schemas]
# Frontmatter key naming a note's File Class (default: class)
# class_field = "class"

# Directory containing Schema files (default: .traces/schemas/)
# directory = ""

[frontmatter]
# Frontmatter keys for canonical metadata roles
# title        = "title"
# aliases      = "aliases"   # read for file-field display labels
# date_created = { name = "date_created", format = "%Y-%m-%dT%H:%M:%S" }
# date_modified = { name = "date_modified", format = "%Y-%m-%dT%H:%M:%S" }
```

### Template Directory

A user-configurable directory containing template files. Local (project-level, `.traces/templates/`) is checked first, then global (user-level, OS-appropriate default). Configured via the `[templates]` table in `.traces/config.toml` or `~/.config/traces/config.toml`.
*Avoid*: Templates folder, template location

### Template Resolution

A template name resolves first as an exact path, then as a filename in the local template directory, then in the global template directory. Multiple matches produce an error listing the candidates.
*Avoid*: Template lookup, search
