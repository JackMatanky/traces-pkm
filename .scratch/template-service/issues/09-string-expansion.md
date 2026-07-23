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

- **File:** `src/template/engine/str_ops.rs` — extends the existing file in the `engine/` submodule. Add new `env.add_filter(...)` calls in the existing `pub(super) fn register` associated fn, alongside the case-conversion filters that are already registered there.
- **`regex` crate:** required for `regex_replace` and `regex_match`. Add to `Cargo.toml`. Compile `Regex::new` on each call (lazy, not cached) — the `ponytail` choice; switch to `LazyLock` only if profiling shows it matters.
- **Non-regex filters:** stdlib wrappers (1–3 lines). No new deps.
- **Registration order:** Group new filters logically after the existing case-conversion filters inside `register`.
- **Error handling:** Regex compilation failures → `minijinja::Error` (`ErrorKind::InvalidOperation`). Pure string operations are infallible.

## Blocked by

- `.scratch/template-service/issues/05-includes-and-utility-functions.md` (establishes `StrOps` and the `register` pattern)

## Implementation notes

Implemented in `.worktrees/string-expansion` on branch
`issue/09-string-expansion`, commit `263d881` (parent `65ace72`). Not
yet merged to `main`.

All 8 filters are registered in the existing `StrOps::register`
(`src/template/engine/str_ops.rs`), alongside the five case-conversion
filters from issue 05, exactly as the Rust guidance directs. `regex =
"1.13.1"` was added to `Cargo.toml` via `cargo add regex` (latest at
implementation time); `Regex::new` is compiled fresh on every
`regex_replace`/`regex_match` call, per the issue's `ponytail`
guidance — no `LazyLock` cache.

Two deliberate implementation choices beyond the issue's guidance:

- `truncate`/`truncate_words` count by `char`, not byte, so multi-byte
  UTF-8 input is never split mid-character. `truncate` additionally
  handles the case where the ellipsis alone doesn't fit within
  `length` (returns the ellipsis truncated to `length`, rather than
  underflowing `length - ellipsis_len`).
- `truncate_words` makes a single pass over `str::split_whitespace()`:
  the same iterator both builds the kept words (via
  `.by_ref().take(count)`) and, via one trailing `.next()`, detects
  whether a word was left out — no intermediate `Vec` is collected
  just to compare word counts. On the no-truncation path it returns
  `value` unchanged (preserving original whitespace) rather than a
  single-space-rejoined reconstruction.

`kwargs: Kwargs` (for `truncate`/`truncate_words`'s `ellipsis`
keyword argument) triggers a `clippy::needless_pass_by_value`
pedantic warning on these two free functions — the lint doesn't fire
on `DateOps`'s equivalent closure-based `Kwargs` parameter, only on
standalone `fn` items — even though `Kwargs::assert_all_used(self)`
consumes it by value, making the suggested `&Kwargs` a compile error.
Suppressed with `#[expect(clippy::needless_pass_by_value, reason =
"...")]` per the repo's existing convention
(`src/config/file.rs:435`).

### Acceptance criteria status: 10/10 met, 0 unfulfilled

(Checkboxes above are left as originally written — this list
documents status only.)

- MET — All 8 filters callable from templates: registered in
  `StrOps::register`; wiring unchanged from issue 05 (`StrOps::register`
  was already called in `TemplateEngine::new`, `src/template/engine.rs`).
- MET — `trim_prefix`/`trim_suffix` no-op when absent:
  `str::strip_prefix`/`strip_suffix` return `None` on no match,
  `.unwrap_or(value)` falls back to the original. Tested in
  `str_ops.rs::tests::trim_prefix`/`trim_suffix`'s
  `no_op_when_{prefix,suffix}_absent` cases.
- MET — `truncate` respects the length limit including ellipsis;
  no-op for short strings: tested in
  `str_ops.rs::tests::truncate::truncates_by_character_count`
  (`respects_the_length_including_the_default_ellipsis`,
  `no_op_for_short_strings`, `no_op_at_the_exact_length_boundary`) plus
  `accepts_a_custom_ellipsis` and
  `shrinks_the_ellipsis_when_it_alone_exceeds_the_length`.
- MET — `truncate_words` handles empty/single-word/multi-word: tested
  in `str_ops.rs::tests::truncate_words::truncates_by_word_count`'s
  `no_op_on_an_empty_string`, `no_op_on_a_single_word`,
  `truncates_a_single_word_phrase`, and
  `truncates_a_multi_word_phrase` cases.
- MET — `word_count` counts correctly for empty strings and various
  whitespace: tested in `str_ops.rs::tests::word_count::counts_words`
  (empty string, mixed tabs/newlines/spaces, whitespace-only).
- MET — `repeat(0)` → empty, `repeat(1)` → identity: tested in
  `str_ops.rs::tests::repeat::repeats_the_string`'s
  `zero_repetitions_is_empty`/`one_repetition_is_the_identity` cases.
- MET — `regex_replace` supports capture groups, no-match returns
  unchanged: tested in
  `str_ops.rs::tests::regex_replace::supports_capture_group_references`
  and `replaces_matches::case_2_no_match_returns_the_string_unchanged`.
- MET — `regex_match` returns `false` for no-match: tested in
  `str_ops.rs::tests::regex_match::matches_the_pattern::case_2_false_on_no_match`.
- MET — Invalid regex patterns produce `minijinja::Error`, not a
  panic: both `regex_replace`/`regex_match` map `regex::Error` through
  a shared `regex_compile_error` helper to
  `ErrorKind::InvalidOperation`. Tested in
  `str_ops.rs::tests::regex_replace`/`regex_match`'s
  `an_invalid_pattern_raises_a_minijinja_error_instead_of_panicking`.
- MET — Tests cover unicode, empty strings, edge cases: unicode cases
  across `trim_prefix`/`trim_suffix`/`truncate`/`repeat`/
  `regex_replace`; empty-string cases across all 8 filters; edge cases
  include the ellipsis-longer-than-length truncation boundary and the
  zero-count `truncate_words` case. 30 new tests in
  `str_ops.rs::tests` (60 total in the file, including the 30 issue-05
  case-conversion tests unchanged).
