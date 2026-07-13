# Unit Suite Naming

> A test name is the first — and during a red run, often the only — line of documentation you read.

## Core Rule

One behavior per test, one concern per module. A descriptive name lets you understand what broke from the `nextest`/`cargo test` output alone, without opening the test body.

## Quick Start

1. Pick a structure:
   - Multi-unit or complex file → **Structure A** (submodules) — the default.
   - Small file with only 1–2 behaviors → **Structure B** (flat) is acceptable.
2. Pick a module name from the canonical table below (first exact match wins).
3. Name the test function with the formula.
4. Use verb-first naming: `returns_*`, `rejects_*`, `accepts_*`, `parses_*`.

If unsure, default to Structure A with a submodule named after the unit of work, and `action_expected_condition` function names.

## Structure A: With Submodules (Default)

Use for files with multiple functions or multiple concern groups. Hard rule: **if the file has 3+ tests or 2+ units of work, use Structure A.**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    mod lookup {
        use super::*;

        #[test]
        fn returns_none_when_record_is_missing() {}
    }
}
```

Formula: `mod [unit_of_work] { fn [action]_[expected]_[condition]() }`
Combined reading: `lookup::returns_none_when_record_is_missing`

Legacy `should_[action]_[expected]_[condition]` names are acceptable on existing tests — don't churn them just to rename. Use verb-first for new tests.

## Structure B: Without Submodules (Simple Files)

Use only for small files where submodules would add noise for 1–2 behaviors.

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup_returns_none_when_record_is_missing() {}
}
```

Formula: `fn [unit_of_work]_[action]_[expected]_[condition]()`
Combined reading: `lookup_returns_none_when_record_is_missing`

## Formula Components

| Component | Description | Examples |
|---|---|---|
| Unit of Work | Method, struct, or concept being tested | `save`, `lookup`, `parse`, `validation` |
| Action (Verb) | What the code actively does | `returns`, `rejects`, `persists`, `emits` |
| Expected | The outcome or state | `error`, `ok`, `none`, `record`, `true` |
| Condition | Triggering circumstance | `when_missing`, `with_empty_input`, `if_locked` |

## Decision Tree: Choose Structure

1. Testing several independent units in one file? → Structure A.
2. File already uses submodules? → keep them, align naming inside.
3. Only one or two simple behaviors in a small file? → Structure B is acceptable.
4. Unsure? → default to Structure A; it scans better when a run goes red.

## Module Name Selection

Use the first row that matches test intent. Don't invent a new module name if a canonical one fits; if two fit, pick the more behavior-specific one; split modules rather than mixing concerns.

### Core Structure

| Module name | Use when |
|---|---|
| `constructor` | `new`, `try_new`, canonical constructors |
| `builder` | builder APIs and fluent configuration |
| `defaults` | `Default` impl and baseline values |
| `validation` | field/rule acceptance and rejection |
| `invariants` | cross-field/domain invariants that must always hold |
| `integrity` | structural consistency checks (graph/link/schema) |
| `state` | state transitions and lifecycle behavior |
| `accessors` | getters and derived reads |
| `borrowing` | zero-copy / borrowed-view behavior |
| `conversions` | `From`/`TryFrom`/`Into` behavior |
| `formatting` | `Display`/`Debug` rendering |
| `equality` | `Eq`/`PartialEq` behavior |
| `ordering` | `Ord`/`PartialOrd` behavior |
| `hashing` | `Hash` behavior for keys/maps/sets |
| `cloning` | `Clone` behavior |

### Operation-Oriented

| Module name | Use when |
|---|---|
| `lookup` | keyed retrieval (`id`, `path`, `name`, handle) — prefer over many `find_by_*` modules |
| `search` | criteria-based retrieval or matching |
| `filter` | subset selection by predicate |
| `pagination` | limit/offset/cursor mechanics |
| `list` | collection retrieval behavior |
| `create` / `update` / `delete` / `upsert` | pick the exact write operation |
| `parse` | input-to-structure parsing |
| `serialization` | structure-to-encoded-output |
| `deserialization` | encoded-input-to-structure |
| `normalization` | canonicalization/sanitization behavior |
| `indexing` | index build/update/read behavior |
| `caching` | cache hit/miss/evict/fill behavior |
| `transactions` | atomicity/commit/rollback behavior |
| `locking` | acquire/release/contention behavior |

### Infrastructure

| Module name | Use when |
|---|---|
| `fixtures` | shared setup helpers only — no assertions in this module |
| `proptests` | property-based suites only — keep generators and `proptest!` blocks here |

### Unit-Specific

When no canonical name fits precisely, use the exact unit/function name instead of forcing a fit: `process_note`, `parse_frontmatter`. `find_by_*` module names are acceptable when they match a stable public API method family. Don't force a command/query module split unless the code itself is modeled that way.

## Correct Examples

```rust
#[test]
fn returns_error_when_input_is_invalid() {}

#[test]
fn rejects_empty_string_as_input() {}

#[test]
fn parses_valid_markdown_frontmatter() {}
```

```rust
#[cfg(test)]
mod tests {
    use super::*;

    mod process_note {
        use super::*;

        #[test]
        fn returns_blob_when_larger_than_limit() {}

        #[test]
        fn fails_when_frontmatter_is_malformed() {}
    }

    mod validation {
        use super::*;

        #[test]
        fn rejects_names_starting_with_numbers() {}
    }
}
```

## Anti-Patterns

**Naming:**

| Bad | Why |
|---|---|
| `test_foo`, `test_basic`, `test_1`, `it_works` | Tells you nothing when it fails |
| `returns_ok_and_updates_state` | Bundles two behaviors — split into two tests |
| `testValidInput`, `TestValidInput` | Wrong case convention — `snake_case` only |
| `test_validate_...`, `unit_test_for_...` | Redundant prefix — the `#[test]` attribute already says it's a test |
| `misc`, `other`, `general`, `helpers` as a behavior module | Not a behavior — pick the real unit of work |

**Modules:**

| Bad | Why |
|---|---|
| `mod tests_for_validation` | Redundant — use `mod validation` |
| Mixing fixtures, assertions, and proptests in one module | Split by concern: `fixtures`, the behavior module, `proptests` |
| Deeply nested modules without a clear separation benefit | Flatten unless the nesting mirrors real structure |

## Validation Checklist

- [ ] Structure A or B used consistently within the file.
- [ ] Function name follows the formula for the chosen structure.
- [ ] `snake_case`, no `test_` prefix.
- [ ] No bundled behaviors joined with `and`.
- [ ] Descriptive enough to understand the failure without opening the test body.
- [ ] Verb-first (`returns_*`, `rejects_*`, `accepts_*`, `parses_*`) for new tests.
- [ ] Module names are singular (`constructor`, not `constructors`) and canonical where applicable.

## See Also

- [`../SKILL.md`](../SKILL.md#unit-suite-basics) — where these named tests live and how they're structured
- [`../../rust-testing/references/table-driven.md`](../../rust-testing/references/table-driven.md) — naming table-driven cases specifically
