# logos crate research — replacement for custom lexers?

Research on [`logos`](https://docs.rs/logos/0.16.1/logos/) (latest stable 0.16.1, per the
[Logos handbook](https://logos.maciej.codes/)), assessed against this repo's two hand-written
lexing sites:

- `src/index/query/filter.rs` — filter-expression tokenizer + recursive descent parser
- `src/note/lexer.rs` — Dataview-compatible inline-field/tag extractor

## What logos is

A derive-based lexer generator. `#[derive(Logos)]` on a token enum turns regex/literal
patterns into a single deterministic state machine at compile time, then exposes a `Lexer`
iterator over the source. Sources:

- Crate root docs (`logos` 0.16.1, cached via rust-docs-mcp) — "Combines all token
  definitions into a single deterministic state machine … Prevents backtracking … Does all
  of that heavy lifting at compile time."
- [Logos handbook](https://logos.maciej.codes/) — attribute reference, regex syntax,
  callbacks, `morph`, 0.16 migration.

### Key capabilities (sources: crate docs + handbook)

| Feature | Details | Source |
|---|---|---|
| `#[token("lit")]` / `#[regex("re")]` | per-variant patterns; stackable; `&str` or `&[u8]` literals | handbook, Attributes |
| `#[logos(skip "re")]` | regex to skip between tokens | handbook, Getting Started |
| `#[logos(error = T)]` | custom error type (`T: Default + Clone + PartialEq + Debug`); `error(T, callback)` variant for spans | handbook, Custom error type |
| Callbacks | `\|lex\| expr` per pattern; returns `()`, `bool`, `T`, `Option<T>`, `Result<T,E>`, `Skip`, `Filter<T>`, `FilterResult<T,E>`; `lex.slice()`, `lex.span()` | handbook, Using callbacks |
| `Lexer::bump(n)` | extend the current match by `n` bytes from inside a callback — the escape hatch when regex is too limiting | handbook, Using callbacks; crate docs, `Lexer::bump` |
| `Lexer::remainder()` | slice of the source *after* the current token, for callbacks that need to look ahead / re-scan | crate docs, `Lexer::remainder` |
| `Lexer::source()` | full source string, so a callback can inspect text *before* the current token (lookbehind workaround) | crate docs, `Lexer::source` |
| `Lexer::span()` / `spanned()` | byte span of the current token; `spanned()` iterator yields `(Result<Token,Error>, Range<usize>)` | crate docs |
| Longest-match + `priority = N` | disambiguates overlapping patterns; ties are a compile error unless prioritized | handbook, Token disambiguation |
| `ignore(case)` | case-insensitive token/regex (0.16; replaced `ignore_ascii_case`) | handbook, 0.16 migration |
| `#[logos(extras = T)]` | user state accessed in callbacks via `lex.extras` | handbook, Using Extras |
| `morph` | switch token types mid-lex (context-dependent lexing); used with a cloned lexer inside callbacks | handbook, Context-dependent lexing |
| No backtracking | compile-time rejection of `.*`/`.+` (unless `allow_greedy = true`) | handbook, Performance pitfalls |
| `new_partial` | partial-lexing for streaming input | handbook, Partial lexing |

### Regex limitations (source: handbook, Regular expressions / Limitations)

- **No lookarounds** (lookahead/lookbehind) or Unicode word boundaries in *patterns* —
  "attempting to use a missing feature will result in a compile-time error."
- Capture groups silently become non-capturing (you only ever get the whole match via
  `lex.slice()`).
- A DFA cannot count balanced nesting — `[a[b]c]` matching needs a callback.
- **The handbook's documented answer to all three**: "Callbacks can also be used to perform
  more specialized lexing in places where regular expressions are too limiting. For
  specifics look at [`Lexer::remainder`] and [`Lexer::bump`]." So "logos can't do X" is a
  claim about *regex patterns*, not about the lexer as a whole — lookbehind can be done via
  `lex.source()` + `lex.span()`, and nesting via `bump`/`remainder` in a callback.

### The canonical "find in free text" pattern

The handbook's Brainfuck and string-interpolation examples show logos used not just for
strict token streams but for *scanning*: define the tokens you care about plus a broad
catch-all (e.g. `#[logos(skip r"[ \t\n\f]+")]`, or a `#[regex(".", ...)]`/`#[regex(r"[^...]+")]`
token that `logos::skip`s everything else), then iterate — logos emits exactly the matches,
in order, non-overlapping. This is the pattern the note lexer would use.

## Codebase lexer 1: `src/index/query/filter.rs`

`tokenize_filter_expr` (`src/index/query/filter.rs:173`) is a hand-written, character-scan
tokenizer over a small filter expression string, producing `FilterToken` (`filter.rs:149`):
`LParen, RParen, Comma, And, Or, Not, Op(CompareOp), Literal(FieldValue), Ident(String)`.

Grammar handled:
- Delimiters: `(`, `)`, `,`
- Operators: `== != >= <= > <`, logical `&& || !` (`scan_operator`, `filter.rs:233`)
- Double-quoted string literals with `\`-escapes (`scan_string_literal`, `filter.rs:209`)
- Case-insensitive keywords `AND`/`OR`/`NOT`, case-sensitive `true`/`false`/`null`, f64
  numbers, and bare idents — all one path in `scan_word` (`filter.rs:266`), where "try
  `parse::<f64>()`, else Ident" is the literal classification.

### Fit: textbook logos grammar

This is *exactly* the shape of the handbook's **JSON parser** example: keywords→literals
(`#[token("true", |_| true)]`, `null`, …), a number token with a `lex.slice().parse()`
callback, a string token with a regex `"([^"\\]|\\.)*"`, punctuation tokens, a `skip` for
whitespace, and then a hand-written recursive-descent parser over `lexer.next()` — the same
architecture `FilterParser` already uses. A logos version:

```rust
#[derive(Logos, Debug, Clone, PartialEq)]
#[logos(skip r"[ \t\n]+")]
enum FilterToken {
    #[token("(")] LParen,
    #[token(")")] RParen,
    #[token(",")] Comma,
    #[token("&&")] #[token("||")] // logical ops
    #[token("==")] #[token("!=")] #[token(">=")] #[token("<=")] #[token(">")] #[token("<")]
    Op(CompareOp),
    #[token("AND", ignore(case))] #[token("OR", ignore(case))] #[token("NOT", ignore(case))]
    // (matches `and`/`And`/`AND`; logos' longest-match keeps `and` from winning over idents)
    #[regex(r#""([^"\\]|\\.)*""#, string_callback)]
    Literal(FieldValue),
    #[regex(r"[0-9]+(\.[0-9]+)?", number_callback)] // priority over Ident below
    Number(FieldValue),
    #[regex(r"[^\s()",=!<>&|]+", ident_callback)]
    Ident(String),
}
```

What logos gives for free:
- Longest-match disambiguation of `>=` over `>`, `==` over `=` (today: ordered
  `strip_prefix` in `operators.rs:34`).
- `ignore(case)` for `AND`/`OR`/`NOT` (today: `eq_ignore_ascii_case`).
- Per-token byte spans via `span()`, enabling precise error messages (today: the whole
  expression fails as `QueryError::UnparsableFilterExpression`, no offset).
- ~90 lines of manual byte-accounting (`Chars` peeking, `char_indices`, offset arithmetic)
  collapse to derive attributes.

Real frictions:
- **Number-vs-Ident classification**: today `scan_word` tries `parse::<f64>()` and falls
  back to `Ident` for the same text — so `1e5`, `-5`, and `4.5` are Numbers while `abc` and
  `12abc` are Idents. logos splits by pattern: the number regex above handles `4.5` but not
  `1e5` or `-5` unless widened (e.g. `-?[0-9]+(\.[0-9]+)?([eE][+-]?[0-9]+)?`), and `12abc`
  would lex as Number(`12`) then Ident(`abc`) — a behavior change. To preserve today's
  semantics exactly, use one broad `Word` regex with a callback that re-runs the current
  f64-then-Ident classification.
- **Error granularity**: unmatched bytes yield `Err` (one per bad token) instead of one
  whole-expression error. The parser maps any `Err` to `UnparsableFilterExpression` — a
  semantic change that also enables better diagnostics.
- **New dependency**: `logos` + `logos-derive` (proc macro). The tokenizer today is ~90
  dependency-free lines; `regex` is already a dep but the filter lexer deliberately does
  not use it.
- **Lint profile**: this repo denies many pedantic/restriction lints in `Cargo.toml`
  (`indexing_slicing`, `arithmetic_side_effects`, `expect_used`, `panic`, …). Derive-
  generated code is not subject to them the same way, but the callbacks are.

**Verdict**: viable and idiomatic. Replacing the tokenizer shrinks ~90 hand-accounted lines
to ~30 derive attributes plus callbacks, with spans for free. The cost is a proc-macro
dependency and a decision on the number/ident boundary. Reasonable to do; not urgent —
the current code is tested and correct.

## Codebase lexer 2: `src/note/lexer.rs`

`extract_inline_fields` / `extract_tags` (`src/note/lexer.rs:98,127`) scan free-form
Markdown prose for dispersed occurrences. Structure:

- `BODY_FIELD_RE`, `TAG_RE` — `regex` `find_iter` over the whole buffer (`lexer.rs:24,36`).
- Wrapped fields `[key:: value]` / `(key:: value)` — a hand-rolled bracket scanner tracking
  **nesting depth and `\`-escapes** (`find_closing`, `lexer.rs:245`).
- **Four independent scans** (body fields, visible wrapped, hidden wrapped, task
  shorthands) are run over the same text, then merged in `finish()` (`lexer.rs:314`):
  sort by byte offset, drop overlaps.
- `ValueParser` (`lexer.rs:332`): a recursive-descent *parser* — comma lists, quoted
  strings, links, durations, dates, numbers, tags — none of which is lexing.

### Fit: real, with the hard parts moving into callbacks

The multi-pass + sort + dedup in `finish()` is the giveaway: **logos emits non-overlapping
tokens in source order, so a single logos pass produces exactly the merged, deduplicated,
position-ordered result `finish()` computes — the merge logic disappears entirely.** My
earlier "logos can't do overlap dedup" claim had the model backwards; it is the current
code that must manually reproduce what a single pass gives for free.

A logos version: one token enum with `Tag`, `BodyField`, `WrappedFieldOpen` (both `[` and
`(`), task-emoji shorthands, plus a broad skip/catch-all so plain prose is ignored
(Brainfuck-example pattern). Callbacks handle the genuinely hard bits:

- **Balanced brackets / escapes** — `[key:: [value]]`, `\]`: match the opener `[`/`(` as a
  token, then in the callback walk `lex.remainder()` with the same nesting/escape counter
  `find_closing` already implements, and `lex.bump(n)` to consume through the closing
  bracket. The hand-rolled logic survives, but relocated to a callback.
- **`foo#bar` rejection** — logos regex has no lookbehind, but the callback can check
  `lex.source()[..lex.span().start]` (the char before the `#`) and return `Skip`/`Filter`.
  `Lexer::source()` makes this a one-liner instead of today's pre-match filter.
- **Task emoji shorthands** (multi-byte emoji + ISO date) — `#[token("🗓️")]` etc. match
  directly; the callback validates the following date as today's `scan_task_shorthands` does.

Costs:
- The `ValueParser` layer is unaffected — it consumes `lex.slice()`/`remainder()` strings
  and stays handwritten either way.
- Four regex `find_iter` passes become one DFA pass (faster over many notes; the existing
  `finish()` merge also disappears).
- Same new-dependency and lint-profile considerations as the filter lexer.
- The catch-all skip regex must be chosen carefully so it never accidentally swallows a
  match-start; that is a real footgun, though the current code's "text already excludes
  code blocks" precondition bounds the input.

**Verdict**: feasible and arguably a *structural simplification* (kills `finish()`), but
the nesting/escape/lookbehind/emoji logic all lands in callbacks that closely mirror the
existing hand-rolled code. Net win: one pass instead of four + no merge. Net cost: a proc-
macro dependency and a careful catch-all regex. Roughly a wash in maintenance terms; a real
win only if scanning many notes makes the single-pass speedup matter.

## Bottom line

- `src/index/query/filter.rs`: **best candidate** — a textbook logos grammar (its JSON
  example is the same shape), spans for free, ~60 lines of byte-accounting deleted. Decide
  the number-vs-ident boundary first; accept `Err`-per-bad-token as an improvement.
- `src/note/lexer.rs`: **feasible, and removes `finish()`'s multi-pass merge**, but every
  hard case (nesting, escapes, lookbehind, emoji) survives in callbacks, so it is closer to
  a wash. Only compelling if note-indexing throughput matters.
- Both changes add `logos` + `logos-derive`. Neither is urgent; the current code is tested
  and correct. If the filter grammar grows (more operators, functions, contexts) or notes
  get scanned heavily, logos becomes the better-maintained option.

## Sources

- `logos` 0.16.1 crate docs — cached and queried via rust-docs-mcp: `Logos` trait
  (`lexer`, `Extras`, `Source`, `Error`), `Lexer` (`new`, `bump`, `remainder`, `source`,
  `span`, `spanned`, `slice`, `morph`), `Skip`, `Filter`, `FilterResult`, `logos::skip`.
- Logos handbook: https://logos.maciej.codes/ (Attributes, `#[token]`/`#[regex]`, Token
  disambiguation, Using Extras, Using callbacks, Context-dependent lexing, Regular
  expressions, Limitations, 0.16 migration, Brainfuck / JSON parser / String interpolation
  examples).
- This repo: `src/index/query/filter.rs`, `src/note/lexer.rs`, `src/note/byte.rs`,
  `Cargo.toml` (lint profile, dependency list).
