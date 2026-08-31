# Schema

Schema registry, field resolution, inheritance DAG, and class hierarchy
queries.

## Language

### Schema Model

#### Schema Definition

A declarative specification in `.traces/schemas/<name>.toml` defining the field
definitions and constraints that govern notes of a File Class.
*Avoid*: field preset, schema definition file, template schema

#### Global Schema

The reserved schema `global.toml` providing a shared pool of field definitions
referenceable via `$ref`, which cannot itself be assigned to notes.
*Avoid*: preset fields, shared fields, base schema

#### File Class

The classification string in a note's frontmatter that binds the note to one or
more Schemas.
*Avoid*: note type, document kind, tag

### Inheritance & Resolution

#### Extends

The mechanism by which a Schema inherits field definitions from parent Schemas,
forming an acyclic inheritance DAG.
*Avoid*: inherits, subclasses, parents

#### Excludes

A Schema-level list of inherited field names dropped during inheritance
resolution.
*Avoid*: skip, ignore, omit

#### Field Resolution

The process of merging own fields, inherited parent fields, exclusions, and
`$ref` overrides into an effective field definition list.
*Avoid*: inheritance, field merging

#### $ref

A reference in a field definition pointing to a base definition in the Global
Schema or an ancestor Schema for local override.
*Avoid*: reference, field alias, preset ref

### Field Definitions

#### Field Definition

A named metadata specification describing field type (`input`, `select`,
`boolean`, `number`, `date`, `file`), requirements, and type-specific
constraints.
*Avoid*: property, field setting, column

#### Select Option

An allowed discrete choice for a select field, optionally defining display
labels and sorting order.
*Avoid*: choice, enum value, dropdown item

#### File Field Filter

Declarative constraints restricting valid targets for a file-valued field by
folder, file extension, or File Class.
*Avoid*: file constraint, target rule
