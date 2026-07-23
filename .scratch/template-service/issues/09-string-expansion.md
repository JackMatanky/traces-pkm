# String filters (trim_*, truncate, word_count, repeat, regex_*)

Status: ready-for-agent

## Parent

`.scratch/template-service/spec.md`

## What to build

Extend the string filters beyond the case-conversion filters from issue 05 (`snake_case`, `kebab_case`, etc.) with manipulation, truncation, inspection, and regex operations.

All are flat-named pipeline filters taking a string as input.

### Filters (no new deps)

- **`trim_prefix(prefix)`** — Removes a specific prefix if present. Uses `str::strip_prefix`.
- **`trim_suffix(suffix)`** — Removes a specific suffix if present. Uses `str::strip_suffix`.
- **`truncate(length, ellipsis="...")`** — Truncates by character count, appending ellipsis if truncated. Total length including ellipsis ≤ `length`.
- **`truncate_words(count, ellipsis="...")`** — Truncates by word count, appending ellipsis. Splits on whitespace.
- **`word_count`** — Returns the number of words (whitespace-separated tokens).
- **`repeat(n)`** — Repeats the string `n` times. Uses `str::repeat`.

### Filters (require `regex` crate)

- **`regex_replace(pattern, replacement)`** — Replaces all matches of a regex pattern with the replacement. Supports capture group references (`$1`, `$2`).
- **`regex_match(pattern)`** — Returns `true` if the string contains a match for the pattern.

Usage:
```jinja
{{ "Hello World" | word_count }}           {# -> 2 #}
{{ "Hello World" | truncate(5) }}          {# -> "He..." #}
{{ "foo_bar" | trim_prefix("foo_") }}     {# -> "bar" #}
{{ "hello@world" | regex_replace("@.*", "") }} {# -> "hello" #}
```

## Acceptance criteria

- [ ] All 8 filters callable from templates
- [ ] `trim_prefix`/`trim_suffix` no-op when prefix/suffix absent
- [ ] `truncate` respects length limit including ellipsis; no-op for short strings
- [ ] `truncate_words` handles empty, single-word, multi-word correctly
- [ ] `word_count` counts correctly for empty strings and various whitespace patterns
- [ ] `repeat(0)` → empty; `repeat(1)` → identity
- [ ] `regex_replace` supports capture groups; no-match returns string unchanged
- [ ] `regex_match` returns false for no-match
- [ ] Invalid regex patterns produce `minijinja::Error` (not panic)
- [ ] Tests cover unicode, empty strings, edge cases

## Rust guidance

- **File layout:** `src/template/str_ops.rs` — extend the existing `StrOps` struct from issue 05. Add new filter registrations in the same `register` method.
- **`regex` crate:** required for `regex_replace` and `regex_match`. Add to `Cargo.toml`. Compile `Regex::new` on each call (lazy, not cached) — the `ponytail` choice; switch to `LazyLock` only if profiling shows it matters.
- **Non-regex filters:** stdlib wrappers (1–3 lines). No new deps.
- **Registration order:** Register named-look filters near the top of `register()` so they're easy to find in the same file, alongside the existing case filters.
- **Error handling:** Regex compilation failures → `minijinja::Error` (`ErrorKind::InvalidOperation`). Pure string operations are infallible.

## Blocked by

- `.scratch/template-service/issues/05-includes-and-utility-functions.md` (establishes `StrOps` and the `register` pattern)
