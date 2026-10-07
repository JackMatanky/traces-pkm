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

#### File Metadata

The universal filesystem metadata captured for every regular file in a project:
relative path, size, timestamps, and format classification.
*Avoid*: fs entry, file record

#### Date

A parsed civil calendar date with no time-of-day component. `src/date.rs` is the
crate's single owner of date recognition, parsing, and formatting; every
date-shaped string — including serde deserialization — funnels through it.
Enforces the four-digit year invariant (rejecting two-digit ambiguous years) and
hoists precision (`YearMonth`, `Date`, `DateTime`) to the recognition layer,
reducing `YYYY-MM` to day 1.
*Avoid*: NaiveDate, calendar value

#### Date-Time

A parsed date-time point, stored as UTC and rendered as the reader's local wall
clock. Shares `src/date.rs`'s parser with Date, where naive input represents the
local zone, resolved to UTC at parse time. DST ambiguities resolve to the
earliest occurrence (RFC 5545 / Temporal `'compatible'`), spring-forward gaps
advance across the gap, and zone lookup failures surface as explicit errors.
Comparing across Date and Date-Time promotes the Date at local midnight, with a
declared exception falling back to the neutral UTC frame if zone lookup fails.
*Avoid*: Timestamp, instant

#### Duration

An elapsed duration carrying both its total seconds magnitude and written source
components. `src/duration.rs` is the sole owner of duration recognition, parsing,
and the unit registry (`UNIT_MAP`, `DurationUnit`, `fixed_seconds`). Compares and
hashes canonically by total seconds, so `1h 30m` and `90m` are equal. Negative
zeros normalize (`-0m` equals `0m`) at construction; raw float comparisons follow
a unified signed-zero doctrine (ticket 09 extraction home). The unit vocabulary
maintains a strict sync contract between `UNIT_HINT` and `UNIT_MAP` (ticket 04 test
home).
*Avoid*: elapsed time, time span, interval

#### Calendar Owner & Regime

`src/date.rs` is the single calendar owner for shift, diff, and duration
application; query and template engines are callers over it, owning no duplicate
calendar arithmetic. Operates under a dual regime: identity equals total magnitude
in seconds, while application treats day, week, month, and year as calendar units
applied left-to-right on the local wall clock, and sub-day units as exact duration
seconds. Consequently, equal duration values (such as `1d` and `24h`) may shift
dates differently across calendar boundaries and DST transitions.
*Avoid*: calendar engine, custom date math

#### Temporal Strictness & Resolution

Temporal parsing strictness follows a strictness-by-seam rule: user input in notes
and templates is lenient with actionable error diagnostics; serde deserialization
is strict; query filters evaluate strict typed expressions. Week math and
numbering pin strictly to ISO Monday via standard calendar primitives, never
locale-dependent offsets or epoch-day division. Avoid terms listed across this
glossary are strictly prohibited from appearing as concept names or definitions.
*Avoid*: locale week, lenient parser

#### Path Codec
The platform-native (de)serialization of `PathBuf` fields crossing a
serialized format. `src/path.rs` is the crate's single owner; every such
field funnels through `crate::path::codec`.
*Avoid*: path serde, path serialization, filesystem encoding

### Traversal & State

#### Directory Tree

The shared filesystem traversal model for scanning directories, discovering
templates, and loading schemas with classified error handling.
*Avoid*: walker, walk adapter, walkdir

#### File Path Tracker

The user-level persistent tracker managing tracked configuration paths and
trusted project roots.
*Avoid*: state cache, local database, cache dir

#### User

The human operating Traces, or an automated agent acting on their behalf.
*Avoid*: Client, operator, caller
