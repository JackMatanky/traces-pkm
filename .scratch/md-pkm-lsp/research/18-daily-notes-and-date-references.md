# Research 18: Daily Notes & Date-Based References

Resolves ticket [18-daily-notes-and-date-references](../issues/18-daily-notes-and-date-references.md).

**Date:** 2026-09-18
**Status:** Complete

Consolidated from: rust-docs-mcp crate analysis, LSP ecosystem survey, performant-LSP best practices, three adversarial stress tests (codebase, map, performant-LSP conventions), and natural language date feasibility research.

---

## Executive Summary

The ecosystem splits into two layers: **editor plugins** (Obsidian ecosystem) handle daily note creation/navigation at the UI level, while **LSP servers** (Markdown Oxide, Marksman) handle link resolution and completion. Only Markdown Oxide has native LSP-level date intelligence. The rest rely on convention (filename pattern) or external plugins.

For Traces, daily notes are a *configuration-driven* concern — the LSP needs the user's date format, folder structure, and granularity preferences. The canonical pattern across performant LSPs: **extract date metadata cheaply at index time, compute all LSP features lazily at request time**.

Key decisions from this research:
1. **LSP-aware** — daily notes are a distinct note kind at the LSP level
2. **Configurable matching** — `file`/`folder`/`both`/`path` per granularity
3. **All periodic granularities** — `day`/`week`/`month`/`quarter`/`year` in config; LSP features start with daily
4. **Hybrid completion** — algorithmic (±7 days with relative labels) + on-disk matching, capped
5. **Natural language dates in v1** — custom ~150-line parser (zero deps) for today/tomorrow/weekday/±N
6. **Diagnostic-driven creation** — code action on diagnostic only; definition returns null; hover informational
7. **Past-date-only diagnostics** — future date-shaped links are intentional forward references

---

## 1. Ecosystem Landscape

### 1.1 Per-Tool Comparison

#### Markdown Oxide (LSP — Rust)

The **only LSP** with native daily note intelligence.

| Aspect | Detail |
|--------|--------|
| Daily notes as distinct kind? | Yes — dedicated `daily.rs` module (~259 lines). |
| Date-shorthand wikilink resolution? | Yes — `[[tomorrow]]`, `[[next monday]]`, `[[+7]]` all resolve via `fuzzydate::parse`. |
| Completion/validation? | Yes — generates 15 candidates (±7 days) with relative labels. No NLP — purely deterministic date arithmetic. |
| Date extraction? | **Filenames only.** Regex `r"(\d{4})-(\d{2})-(\d{2})"` on file stem. |
| Next/prev navigation? | Yes — `prev`/`next` directives, `+N`/`-N` offsets. |

**Health caveat:** Markdown Oxide's creator publicly sought a maintainer handoff (Sept 2025). Useful precedent but not sole authority.

#### Obsidian Core Daily Notes (Plugin — TypeScript)

Format uses Moment.js tokens (`YYYY-MM-DD`). Folder structure can embed date components. Template uses `{{date:YYYY-MM-DD}}` syntax.

#### Periodic Notes (Plugin — TypeScript, by Liam Cain)

Each granularity has independent format/folder/template. Flat, independent-per-granularity config — no inheritance or cascading.

#### Marksman, zk, rumdl

No LSP-level daily note intelligence. Marksman: generic wiki-link completion only. zk: CLI-level daily notes, LSP handles regular links. rumdl: linting only.

### 1.2 Cross-Cutting Patterns

1. **Configuration-Driven Date Format.** No tool hardcodes `YYYY-MM-DD`.
2. **Filename as Primary Date Source.** Frontmatter `date` is secondary.
3. **Granularity = Independent Config.** Each granularity has separate format/folder/template.
4. **Natural Language as Power Feature.** High-value; included in v1 via custom parser.
5. **No LSP Query Language for Dates.** Dataview does this at plugin level.

---

## 2. Rust Crate Ecosystem

### 2.1 Already in Traces

| Crate | Version | Relevant Use |
|-------|---------|-------------|
| `chrono` | 0.4.45 | `NaiveDate` parsing, strftime formatting, date comparison |
| `regex` | 1.13.1 | Filename pattern matching, date component extraction |
| `walkdir` | 2.5 | Directory traversal during indexing |

### 2.2 Key Crate: chrono

Gotchas for daily notes:
1. **`%V` requires `%G`** — ISO year can differ from calendar year. Use `%G-W%V`.
2. **`%q` is chrono-specific** — not standard strftime. Resolves to January 1.
3. **Weekly patterns need two-step parsing** — regex-extract year + week, then `NaiveDate::from_isoywd_opt`.
4. **Standardize on ISO weeks** (`%V`/`%G`).

Performance: ~30-50ns per parse. For 10K filenames: ~2-5ms.

### 2.3 Candidate: globset

`GlobSet` uses Aho-Corasick for O(n) multi-pattern matching — <1ms for 10K files. Detects if a filename is date-shaped; does NOT extract components. Recommended addition.

### 2.4 Filename → Date Pipeline

```
1. regex — Pattern Detection & Component Extraction
2. chrono — Date Validation & Normalization → NaiveDate
3. chrono — Date Comparison & Range Queries (Ord + Copy)
```

Performance budget for 10K files: ~6ms total.

---

## 3. Performance & Architecture Patterns

### 3.1 Lazy vs Eager Computation

**Consensus: compute on demand, never eagerly.** Salsa (rust-analyzer), Biome, gopls all follow this pattern.

| Feature | Compute When | Cost |
|---------|-------------|------|
| Hover on `[[2026-09-04]]` | Lazily on request | ~1μs |
| Completion for date-shaped references | Lazily on trigger | ~10μs |
| Definition for `[[2026-09-04]]` | Lazily on request | O(1) HashMap lookup |
| Date metadata extraction | At index time | ~25ns × 20K = 0.5ms total |

### 3.2 Definition-to-Nonexistent-File Patterns

**Stress-test finding:** returning a `Location` for a nonexistent file has no precedent in any major LSP. rust-analyzer explicitly avoids this. Editors handle it inconsistently.

**Revised pattern — separation of concerns:**

| Surface | Behavior |
|---------|----------|
| Definition | Return `null` — file doesn't exist |
| Hover | "Note not found" — informational, no creation prompt |
| Code action on diagnostic | "Create from template {X}" — the action surface |

Creation is triggered by the **diagnostic** "date-shaped link has no matching file," not hover or definition.

### 3.3 Diagnostic Publishing Patterns

Push diagnostics with debounce (300ms). Hash diagnostics before publishing (gopls pattern).

**Stress-test finding on date-shaped unresolved links:** PKM tools treat unresolved links as informational, not errors. Future dates are intentional forward references.

**Diagnostic schedule:**

| Diagnostic | Scope | When |
|-----------|-------|------|
| "Date-shaped link has no matching file" | **Past dates only** (date < today) | On file change, debounced 300ms |
| "Date is not valid" | All date-shaped stems | Immediately on parse |
| Future date-shaped links | **No diagnostic** | Never |

### 3.4 Index-Time vs Runtime Tradeoffs

**At index time:** `is_date_shaped: bool` + `parsed_date: Option<NaiveDate>` in `FileEntry`. Intrinsic filename properties — cheap (~0.5ms for 20K files), immutable for file lifetime.

**Important:** `parsed_date` depends on `[periodic]` config format. Config fingerprinting (already being implemented separately) handles re-evaluation when config changes.

**At LSP runtime:** hover (~1μs), completion (~10μs), definition (O(1)).

### 3.5 Crate Performance: chrono vs jiff

chrono ~30-50ns, jiff ~25ns for `YYYY-MM-DD` parsing. Difference across 20K files: 0.5ms — immaterial. Keep chrono.

### 3.6 Natural Language Date Parsing

#### Ecosystem Research

The Obsidian "Natural Language Dates" plugin (nldates-obsidian) uses `chrono-node` (JS, 5.2k stars) for parsing. It is NOT a completion list — it's a single-result parser: type `@tomorrow` → get one date. Supports English + 5 other languages.

**Key finding:** There is no Rust equivalent of chrono-node for full natural language date parsing. `chrono-english` is closest but unmaintained (RUSTSEC-2024-0395) and English-only.

#### Markdown Oxide's Approach (No NLP)

Markdown Oxide does NOT use natural language parsing. It generates a **fixed window** of 15 daily note candidates (±7 days) with relative labels:

```rust
let today = chrono::Local::now().date_naive();
let days = (-7..=7)
    .flat_map(|i| Some(today + Duration::try_days(i)?))
    .flat_map(|date| MDDailyNote::from_date(date, self))
    .filter(|date| !refnames.contains(&date.ref_name))
    .map(LinkCompletion::DailyNote);
```

Labels like "today", "tomorrow", "next Friday" are display-only — the actual insertion uses the date format. This is deterministic, fast, and requires no NLP.

#### Feasibility in Rust LSP

Natural language date parsing is fully feasible:
- **Pure computation** — no async, no network, no file I/O
- **Fast** — <1ms for all patterns (well within 100ms LSP budget)
- **Already proven** — Markdown Oxide uses `fuzzydate::parse` in production

Relevant crates:

| Crate | NL Parsing | Speed | Status |
|-------|-----------|-------|--------|
| `fuzzydate` | Yes | <1ms | Used by Markdown Oxide |
| `chrono-english` | Yes (GNU date -d) | <1ms | Unmaintained (RUSTSEC) |
| `parse_datetime` | Yes (relative) | <1ms | Uses Jiff, no "next friday" |
| `natural-date-parser` | Yes (Pest-based) | <1ms | English only |

#### Ambiguity Handling

"next friday" is ambiguous (US vs UK). Resolution: always resolve relative to today. "next friday" on a friday → next week's friday. Document the behavior.

#### Recommendation: Custom Parser for v1

Write a ~150-line parser handling:
- `today`, `tomorrow`, `yesterday`
- `next monday` through `next sunday`
- `last monday` through `last sunday`
- `+Nd`, `+Nw`, `+Nm` (days, weeks, months)
- `-Nd`, `-Nw`, `-Nm`

**Why custom over a crate?**
- Zero dependency cost
- Full control over ambiguity resolution
- Easier to extend later
- Matches Markdown Oxide's approach (deterministic, no NLP)

**v2 additions:** `chrono-english` for GNU date -d style, time expressions, locale support.

---

## 4. Config Design: `[periodic]`

Designed from scratch for Traces' actual needs. The query-expansion spec's `[periodic]` table is not authoritative.

### 4.1 Naming Convention

Short granularity names: `day`, `week`, `month`, `quarter`, `year`.

### 4.2 Config Structure

```toml
[periodic]

[periodic.day]
format = "%Y-%m-%d"              # optional — default: %Y-%m-%d
folder = "daily"                  # optional — vault-relative
template = "templates/daily.md"   # optional — omit for empty file
match = "file"                    # optional — default: "file"
#  "file"   — stem matches format (ignore folder)
#  "folder" — file is in folder (ignore format)
#  "both"   — stem matches format AND file is in folder
#  "path"   — full vault-relative path matches folder/<date-stem>.md

[periodic.week]
# format defaults to %G-W%V
folder = "weekly"
template = "templates/weekly.md"
match = "folder"                  # common: organize weekly notes by folder

[periodic.month]
# format defaults to %Y-%m
folder = "monthly"

[periodic.quarter]
# format defaults to %Y-Q%q
folder = "quarterly"

[periodic.year]
# format defaults to %Y
folder = "yearly"
```

### 4.3 Matching Modes

| Mode | What it checks | Use case |
|------|---------------|----------|
| `file` | Stem matches the format pattern | Default. Works for flat vaults. |
| `folder` | File lives in the configured folder | Split-folder vaults (user's case). |
| `both` | Stem matches AND file is in folder | Strict matching — both conditions must hold. |
| `path` | Full vault-relative path matches `folder/<date-stem>.md` | Embedded date components in path (e.g., `daily/2026/2026-09-04.md`). |

At index time, the matching mode determines whether `is_date_shaped` is set on `FileEntry`:
- `file` / `both` / `path`: check filename stem against format
- `folder`: check file's parent directory against configured folder
- Combined with `parsed_date` extraction when matched

### 4.4 Design Decisions

- **No `enabled` field.** A granularity is enabled when its `[periodic.<granularity>]` table exists.
- **`format` is optional.** Sensible defaults per granularity. User omits it when the default works.
- **`folder` and `template` are optional.** Omitting `folder` → vault root. Omitting `template` → empty file.
- **`match` defaults to `"file"`.** Most users have flat vaults. Split-folder users set `match = "folder"` or `"path"`.
- **No inheritance.** Each granularity is fully independent.

### 4.5 Default Format Strings

| Granularity | Default format | Example |
|------------|---------------|---------|
| `day`        | `%Y-%m-%d`       | `2026-09-04` |
| `week`       | `%G-W%V`         | `2026-W36` |
| `month`      | `%Y-%m`          | `2026-09` |
| `quarter`    | `%Y-Q%q`         | `2026-Q3` |
| `year`       | `%Y`             | `2026` |

### 4.6 Config-Change Propagation

Config fingerprinting (already being implemented) detects `[periodic]` changes. Only affected files re-evaluate. O(N) per config change, not O(N) per startup.

### 4.7 Integration with Existing Config

`RawConfig` uses `#[serde(deny_unknown_fields)]` (`src/config/raw.rs:15`). Adding `[periodic]` requires:
1. New `PeriodicMatchMode` enum: `File`, `Folder`, `Both`, `Path`
2. New `PeriodicGranularityConfig` struct: `format: Option<String>`, `folder: Option<String>`, `template: Option<String>`, `match: Option<PeriodicMatchMode>`
3. New `PeriodicConfig` struct with optional fields for each granularity
4. Add `periodic: Option<PeriodicConfig>` to `RawConfig`
5. Corresponding resolved type in `Config` (`src/config/model.rs`)

---

## 5. Codebase Alignment (Stress-Test Findings)

### 5.1 `FileEntry`, Not `FileRecord`

`FileRecord` does not exist. The current index entry type is `FileEntry` (`src/index/entry.rs:131`):

```rust
pub struct FileEntry {
    file: FileBase,
    note: Option<Box<Note>>,
    inlinks: Box<[PathBuf]>,
}
```

**Extension plan:** Add `is_date_shaped: bool` + `parsed_date: Option<NaiveDate>`. ~12 bytes growth. Current `FileEntry` is ~160 bytes (assertion at `src/index/entry.rs:346-352`). Must verify assertion holds.

**Serialization:** `postcard` defaults `Option` to `None`. Existing databases need no migration.

### 5.2 Standing Constraint Checks

| Constraint | Status |
|-----------|--------|
| Reuse existing Traces semantics/services | ✅ Reuses `DateValue`, `FileIndex`, `LinkResolver`, `RefreshPlan` |
| Performance is first-class | ✅ Index-time ~0.5ms; lazy LSP features |
| LSP protocol DTOs stay at boundary | ✅ `CreateFile` is protocol-level |
| Preserve existing architecture | ✅ Extends `FileEntry`, doesn't restructure |
| No parallel LSP-only models | ✅ Builds on existing pipeline |

### 5.3 Map Tensions

| Tension | Resolution |
|---------|-----------|
| Ticket 15: zero-match policy | Date-shaped wikilinks follow ticket 15's zero-match → Information + code action |
| Ticket 22: template-path validation | Cross-cutting; ticket 22 should reference ticket 18 as consumer |
| Ticket 24: completion architecture | Ticket 18 specifies *what*, not *how*. Dispatch belongs to ticket 24 |
| Ticket 33: refresh path | Date extraction in `IndexerService::refresh`; defers to ticket 33 |

---

## 6. Concrete Recommendations for Ticket 18

### Q1: LSP-aware daily notes?

**Yes.** Distinct note kind at LSP level. Index-time extraction into `FileEntry`.

### Q2: Matching scope?

**Configurable per granularity:** `file` (default), `folder`, `both`, `path`. Covers flat vaults and split-folder setups.

### Q2a: Completion scope?

**Link targets only.** Task markers and inline fields belong to tickets 19/23.

### Q2b: Natural language dates in v1?

**Yes.** Custom ~150-line parser (zero deps) for: today/tomorrow/yesterday, next/last weekday, ±Nd/Nw/Nm. Full NL parsing (chrono-node equivalent) is v2.

### Q3: All periodic granularities?

**Yes.** Config supports all five. LSP features start with daily, extend incrementally.

### Q3a: Creation for nonexistent date notes?

**Diagnostic code action only.** Definition returns null. Hover shows "Note not found" (informational).

### Q3b: Which template?

**Config-specified.** `[periodic.day].template`; omit for empty file. Template receives parsed date as variable.

### Q4: Completion strategy?

**Hybrid with cap.** Algorithmic (±7 days with relative labels) + on-disk matching. Cap algorithmic candidates.

### Q5: Diagnostic scope?

**Past-date unresolved links only.** Future dates are intentional forward references.

### Q6: Config defaults?

**`format` optional per granularity** with sensible defaults. Minimal config: just `[periodic.day]` with `folder` and `template`.

### Q7: What NOT to build

- **No frontmatter date parsing for resolution.** Filename is sufficient.
- **No Dataview-like query language in v1.** Ticket 21+.
- **No creation prompt on hover.** Diagnostic-driven only.
- **No diagnostic for future-date unresolved links.**
- **No full NL parsing in v1.** Custom parser covers common patterns; `chrono-english` for v2.

---

## 7. Explicit Reuse List

| Existing Service | Used For | Ticket |
|-----------------|----------|--------|
| `DateValue` (`src/date.rs`) | Date parsing from filenames | Existing |
| `FileIndex` / `FileEntry` (`src/index/entry.rs`) | Index-time storage | Existing |
| `LinkResolver` (ticket 15) | Wikilink resolution | Ticket 15 |
| Inverted index (ticket 33) | O(1) stem → path lookup | Ticket 33 |
| `RefreshPlan` (ticket 13) | Single-file index updates | Ticket 13 |
| Config system (`src/config/`) | `[periodic]` configuration | Existing |

---

## Sources

### Digest files read
- `docs/digests/lsp_feel-ix-343-markdown-oxide-digest.txt`
- `docs/digests/lsp_feel-ix-343-markdown-oxide-src-digest.txt`
- `docs/digests/lsp_artempyanykh-marksman-digest.txt`
- `docs/digests/lsp_artempyanykh-marksman-docs-digest.txt`
- `docs/digests/zk-digest.txt`
- `docs/digests/zk-docs-digest.txt`
- `docs/digests/lsp_rvben-rumdl-digest.txt`
- `docs/digests/obsidian_blacksmithgu-obsidian-dataview-digest.txt`
- `docs/digests/obsidian_silentvoid13-templater-digest.txt`
- `docs/digests/obsidian_mdelobelle-metadatamenu-digest.txt`

### Existing research files consulted
- `research/tool-zk.md`, `research/tool-markdown-oxide.md`, `research/tool-marksman.md`, `research/tool-rumdl-boundary.md`, `research/08-dataview-templater-metadatamenu-parity.md`

### Primary sources
- Markdown Oxide: `src/daily.rs`, `src/completion/link_completer.rs`
- NLDates plugin: `argenos/nldates-obsidian`
- chrono-node: `wanasit/chrono` (5.2k stars)
- rust-analyzer: `docs/dev/architecture.md`, salsa docs, issue #9173
- Biome: issue #10605, PR #11522 (config fingerprinting)
- Obsidian Periodic Notes: `liamcain/obsidian-periodic-notes`
- gopls: `internal/lsp/diagnostics.go`
- jiff: `COMPARE.md` benchmarks by BurntSushi
- `fuzzydate` crate: used by Markdown Oxide
- `chrono-english` crate: RUSTSEC-2024-0395 (unmaintained)
