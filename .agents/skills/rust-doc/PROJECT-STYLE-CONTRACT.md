# traces-pkm doc/ordering style contract (src/index, src/query)

Read this in full before touching any file. It is the single source of truth
for this pass: every file must read as if written by the same author. Deviate
from it only when a file's existing convention already matches it better than
these examples do.

## Voice

Direct, technical, textbook prose. No "This function...", no hedging, no
promotional language, no conversational asides. State behavior and invariants
as facts.

- Opening sentence of every `///` or `//!` block (through the first blank
  line) is a self-contained one-line summary, imperative or descriptive
  present tense: "Resolves an already-split link target." not "This resolves
  a link target." Terse to the point of an index entry.
- No literal em-dash character. Use the project's existing " - " (space,
  ASCII hyphen, space) for parenthetical asides, or split into two sentences.
  Confirmed zero em-dashes currently in scope; keep it that way.
- Do not restate a parameter's type or the return type in prose when the
  signature already shows it. Describe behavior, invariants, edge cases.
- Skip doc comments on: derived trait impls (`Debug`, `Clone`, `Default`,
  etc.), and truly obvious one-line accessors on self-explanatory newtypes
  (e.g. `fn to_path_buf(self) -> PathBuf { self.0.to_path_buf() }` on a
  single-field path wrapper needs no comment). Document everything else,
  public or private, including private dispatch/helper functions whose
  purpose is not obvious from name and signature alone; keep those to one
  or two lines, not a full section treatment.

## Header sections (only include the ones that apply)

1. Summary + behavior/invariants prose.
2. `# Arguments` only for 3+ parameter functions where names alone don't
   carry the meaning; 0-2 parameter functions describe params inline in
   prose using `` `param` `` backticks.
3. `# Errors` on every `Result`-returning function: one bullet per variant
   that can actually be produced by that call path, each linked
   (`` [`Variant`] ``) with the link definition trailing the comment block
   (`` [`Variant`]: ErrorType::Variant ``). If the function forwards a
   single opaque error type without meaningful variants to distinguish,
   a one-line prose description is fine instead of bullets.
4. `# Panics` only if the function can actually panic; state the exact
   precondition.
5. `# Safety` mandatory on every `unsafe fn`/`unsafe trait`, stating the
   caller's obligation. (Scope currently has none outside redb's own API;
   if any `unsafe` appears, this is non-negotiable.)
6. `# Examples` only for genuinely public, externally-callable API surface
   where a usage snippet adds real information beyond the signature.
   `src/index` and `src/query` are internal crate modules exercised by
   `src/cli` and `src/template`, not a published external API surface;
   favor precise behavior/invariant prose over contrived examples for
   `pub(crate)`/private items. Add `# Examples` where a doctest already
   exists or where a top-level entry point (`IndexerService`, `QueryService`,
   `QueryBuilder`) benefits from one, gated `#[cfg(feature = "test-utils")]`
   the same way existing doctests in this crate already do (see
   `src/query/builder.rs`, `src/query/results.rs`, `src/query/service.rs`
   for the established pattern before adding new ones).

Intra-doc links: link every type, function, trait, and error variant
mentioned. Standard library types get linked too (`` [`Vec`] ``,
`` [`HashMap`] ``, `` [`Option`] ``). Do not introduce a link to a private
item from a `pub` item's doc comment (breaks `cargo doc` without
`--document-private-items`); link the smallest public-or-visible-enough
ancestor instead, or drop the link and name the concept in prose.

## Regular (non-doc) comments

- Keep only comments that explain **why**: an invariant, a non-obvious
  algorithmic choice, a correctness argument, a performance tradeoff, or a
  subtlety a future editor could break without noticing.
- Delete comments that restate the next line of code ("// increment
  counter").
- `// SAFETY: ...` immediately above any `unsafe` block, stating exactly
  what makes it sound.
- No inline trailing comments on the same line as code; put the comment on
  its own line above.

## File ordering (docs/refs/canonical_ordering_discipline.md)

Read that file for the full rules; the practical application here:

- A file reads top to bottom as a tour: the module's primary public type(s)
  and their `impl` blocks come first, in the order a consumer would reach
  for them. Deeper implementation-detail types used only inside that
  module's own machinery come after, roughly in the order the primary type
  descends into them (container type, then its element type, then that
  element's field type, and so on).
- Within one `impl` block: constructor first, then the methods a caller
  actually calls, roughly in descending frequency of use; a private helper
  used by exactly one method goes immediately below that method (locality);
  a private helper shared by several methods, or pure low-level plumbing
  (arena index accessors, etc.), goes at the bottom of the block.
- `pub` methods before `pub(crate)`/`pub(super)` before private, when that
  doesn't fight the locality rule above.
- No `impl` block precedes the type or trait it implements.
- Free functions used by exactly one caller go directly after that caller;
  free functions used broadly, or forming their own leaf utility, go at
  file scope after everything that depends on them.
- `#[cfg(test)] mod tests` always last; internal test module organization
  (grouping by scenario) is already reasonable in this codebase and does
  not need reordering to this rule, only comment quality.
- Struct fields: `pub`, then `pub(crate)`, then private, top to bottom.
- Single `#[derive(...)]` per item: `Copy` first, then std traits
  lexicographic, then third-party traits lexicographic.
- Do not change public signatures, rename items, or alter logic. This is a
  documentation and ordering pass only; every reordering must be a verbatim
  move (cut the item, paste it at its new location unchanged), and the
  moved code must still compile and pass its existing tests unmodified.

## Verification (per file, before reporting done)

1. `cargo check --lib --features test-utils` compiles clean.
2. No new `cargo doc --no-deps --document-private-items --lib --features
   test-utils` warnings introduced by your file (a pre-existing baseline of
   10 warnings, all "links to private item `IndexError::Store`" or
   "`Self::run_from_store`" in `src/index/service.rs` and
   `src/query/service.rs`, is being fixed separately; do not worry about
   those two files' pre-existing warnings unless you are assigned one of
   them, in which case fix the link to point at a visible item instead).
3. `cargo test --lib --features test-utils <module>::` still passes for
   every test in your file, unmodified in behavior.
4. Do not run the full workspace test suite, full clippy, or `cargo fmt` on
   the whole tree. The integration pass does that once at the end.

## Reference exemplar

`src/index/inlinks.rs` has already been brought to this standard. Read it in
full before starting your file; match its voice, its header usage, its
comment density, and its ordering logic exactly.
