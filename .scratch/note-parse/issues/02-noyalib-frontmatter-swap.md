# 02: Frontmatter parser swap to noyalib

**Source:** .scratch/md-pkm-lsp/issues/19-frontmatter-and-inline-field-intelligence.md (resolved decision; its "`YamlVersion::V1_1` for Dataview compatibility" line is **corrected** by this ticket — see Rationale) + .scratch/md-pkm-lsp/issues/11-source-span-and-position-model.md §6
**What to build:** All YAML parsing and serialization routes through `noyalib` (0.0.51) and the `yaml_serde` dependency (imported as `serde_yaml`) is removed. Frontmatter parsing calls `noyalib::from_str_with_config::<noyalib::Value>` with one shared `ParserConfig::serde_yaml_compat()` — YAML **1.2** semantics with serde_yaml-parity knobs — preserving existing valid-YAML behaviour exactly. The template engine's `to_yaml`/`from_yaml` filters move to `noyalib::to_string` / `noyalib::from_str_with_config` with the same config. Malformed-YAML behaviour: **silent-empty stays, plus a structured try-API is added** — `From<&RawFrontmatter>` keeps `warn!` + empty `Frontmatter` (existing tests pass unchanged) and a new `RawFrontmatter::parse` returns the structured failure to the caller (feeds ticket 19 diagnostics later). Note: md-pkm-lsp ticket 19 lists ticket-11 changes as its prerequisite, but that gate governs the spans half, which is out of scope here; this swap is orthogonal and may run in parallel with 01. `Spanned<T>`/CST remain out of scope (tickets 11/19) but the core-API choice keeps them reachable.
**Blocked by:** None (can start immediately)
**Status:** resolved

## Rationale: YAML 1.2, not 1.1

Ticket 19's "enable `YamlVersion::V1_1` for Dataview compatibility" is factually wrong and must not be implemented:

- The current parser (yaml_serde 0.10.7) is YAML **1.2.2** — its own `test_tag_resolution` cites the 1.2.2 spec and asserts `yes/no/on/off/y/n` → `String`, only `true/false` → `Bool`.
- Obsidian parses frontmatter with `eemeli/yaml`, which defaults to YAML 1.2 (maintainer-confirmed: `yes` → boolean only under an explicit `version: '1.1'` opt-in). Dataview reads Obsidian's output, so V1_1 would make Traces **diverge** from Dataview, not match it.
- This repo's own Dataview-compat inline parser (`parse_bool_at`, src/note/parser/inline.rs) accepts only `true`/`false`; a V1_1 frontmatter layer would contradict the inline layer of the same note.
- V1.1 has real footguns for vault content: sexagesimal (`meeting: 10:30` → 630) and octal retyping of existing string values — silent data retype.

V1.2 satisfies the ticket's hard requirement ("existing valid-YAML behaviour preserved") with zero semantic deltas.

## Design

### Cargo
- Add `noyalib = "0.0.51"` with **default features only** (`std`, `fast-int`, `fast-float`, `strict-deserialise`). Do **not** enable `compat-serde-yaml`, `simd` (documented no-op in 0.0.51), `nightly-simd` (nightly-only), `parallel`, or `lossless-u64`.
- Remove `serde_yaml = { package = "yaml_serde", version = "0.10" }`; regenerating `Cargo.lock` drops `yaml_serde`.

### Shared config (built once, used everywhere)
```rust
static YAML_CONFIG: LazyLock<noyalib::ParserConfig> =
    LazyLock::new(noyalib::ParserConfig::serde_yaml_compat);
```
`serde_yaml_compat()` is YAML 1.2 plus the exact yaml_serde parity knobs (`leading_zero_integer_strings` — `zip: 01234` stays a string, `legacy_binary_numbers`, `float_overflow_strings`, `integer_overflow_errors`, `merge_key_policy: AsOrdinary` — `<<` stays a literal key). It never sets `yaml_version`, so 1.2 booleans hold. One shared config for frontmatter and template filters.

### Frontmatter parse path (src/note/metadata.rs)
- New fallible API: `RawFrontmatter::parse(&self) -> Result<Frontmatter, FrontmatterParseError>` where `FrontmatterParseError` is a `thiserror` enum (`Parse(noyalib::Error)` | `NotMapping`), wrapping `noyalib::from_str_with_config(raw.as_str(), &YAML_CONFIG)`.
- `impl From<&RawFrontmatter> for Frontmatter` delegates and swallows: `parse().unwrap_or_else(|err| { warn!(%err, "failed to parse YAML frontmatter block; ignoring malformed fields"); Self::default() })` — silent-empty behaviour byte-for-byte preserved, structured error available to callers.
- `let noyalib::Value::Mapping(map) = val else { … }` — empty input parses to `Value::Null` (handled by the `NotMapping` arm; `RawFrontmatter::is_empty` still short-circuits first).
- noyalib `Mapping` is string-keyed (`IndexMap<String, Value>`): keys go straight to `FieldKey::try_new(key)`; the `yaml_scalar_to_string` key coercion disappears from this path (delete the helper if it ends up with zero callers).

### Value conversion (src/field.rs, src/note/field.rs)
- `From<serde_yaml::Value> for FieldValueRef` → `From<noyalib::Value>`; keep the `as_f64()`-first order (integers → `Float`, as today) and the `Tagged` arm (noyalib has the same 7th variant); delete the now-dead `as_i64` branch.
- `TryFrom<serde_yaml::Value> for FieldName/FieldKey`: port to `noyalib::Value` if any live caller exists, otherwise delete (verify callers during implementation).
- Port test helpers in src/note/field.rs from `serde_yaml::from_str` to `noyalib::from_str`.

### Template engine (src/template/engine/yaml.rs) — full swap
- `serde_yaml::to_string(value)` → `noyalib::to_string(value)`.
- `serde_yaml::from_str::<serde_yaml::Value>(text)` → `noyalib::from_str_with_config::<noyalib::Value>(text, &YAML_CONFIG)`.
- `Value::from_serialize(&parsed)` unchanged (noyalib `Value` implements `Serialize`).
- All existing `to_yaml`/`from_yaml` format assertions pass unchanged (`"3.14\n"`, `"- a\n- b\n"`, `"{}\n"`, `"null\n"`, …). If formatting differs, tune `noyalib::SerializerConfig` — do not weaken the tests.

### Performance (verified against 0.0.51 source — claims, not assumptions)
- For `T == Value` noyalib **always** takes its dedicated span-free zero-rewalk fast path (`parse_exactly_one_value` + direct `Any` downcast; `de.rs` explicitly excludes `Value` targets from the streaming path). Nothing in this config turns it off; do not chase "streaming fast-path eligibility" — irrelevant for our target type.
- memchr SSE2/NEON structural scanning and SWAR decimal parsing are unconditional; the `simd` Cargo feature is a documented no-op in 0.0.51.
- Config is a single `LazyLock` — no per-note rebuild during a 20K-note cold start.
- **Rejected, with reasons:** `borrowed::from_str_borrowed` (~18% faster zero-copy) — our targets are owned (`NoteFieldValue`, String keys), so the copy merely moves into our layer while infecting `Frontmatter` with a borrow lifetime; `no_schema` (all-strings semantics); `parallel` (cross-note parallelism is already rayon, ticket 33); `recovery`/`parse_lenient` — note as a future candidate for ticket 14 live-buffer parsing, not here.

## Stated behaviour deviation (exactly one)

- **Non-scalar mapping key** (`[a, b]: v`): today the yaml_serde path skips just that entry; `serde_yaml_compat()` sets `NonScalarKeyPolicy::Error`, so the whole block fails → empty `Frontmatter` + structured `Err`. Accepted: pathological input, and a structured failure beats a `"[a, b]"` pseudo-key surviving into `FieldKey`. Pinned by test below.

All other realistic vault content is behaviour-identical (yaml_serde parity preset).

## Checklist

- [x] Frontmatter parsing routes through `noyalib::from_str_with_config` with the shared `serde_yaml_compat` config (YAML 1.2); the frontmatter path no longer calls `serde_yaml`
- [x] Template `to_yaml`/`from_yaml` route through noyalib; `yaml_serde` fully removed from `Cargo.toml`/`Cargo.lock`; no `serde_yaml`/`yaml_serde` references remain under `src/`
- [x] All existing frontmatter and template tests pass unchanged (valid-YAML, malformed-YAML, and format assertions)
- [x] Malformed-YAML behaviour stated and tested: silent-empty kept via `From` (warn logged) + structured `RawFrontmatter::parse` try-API returning `FrontmatterParseError`
- [x] Parity guard tests pin YAML-1.2 semantics on realistic vault content: `draft: yes`/`no` → String; `zip: 01234` → String; `meeting: 10:30` → String; `date: 2026-07-29` → `NoteFieldValue::Date`; `<<` stays a literal key; duplicate YAML keys → last wins
- [x] Stated-deviation test: non-scalar key → structured error → empty Frontmatter
- [x] Performance assertions from the Performance section are not regressed by config changes (no extra features, config shared via `LazyLock`)
- [x] Amendment filed against md-pkm-lsp ticket 19 line 29 (and its echo in issue 20): V1_1/Dataview claim corrected, pointer to this ticket's Rationale
- [x] Tests and lint pass (`mise verify`)

## Comments

- **2026-09-28 (agent, branch `noyalib-frontmatter-swap`, worktree
  `.worktrees/noyalib-frontmatter-swap`):** Implemented per spec. All YAML
  routing now goes through `noyalib` behind the shared `YAML_CONFIG`
  (`src/yaml.rs`, `ParserConfig::serde_yaml_compat()`, YAML 1.2); `yaml_serde`
  is gone from `Cargo.toml`/`Cargo.lock`. New fallible seam:
  `RawFrontmatter::parse -> Result<Frontmatter, FrontmatterParseError>`
  (`Parse(noyalib::Error)` | `NotMapping`); `From<&RawFrontmatter>` delegates
  to it and keeps silent-empty with `warn!`. `TryFrom<serde_yaml::Value>`
  impls for `FieldName`/`FieldKey` deleted (no live callers, verified);
  `yaml_scalar_to_string` deleted (noyalib mappings are string-keyed);
  `From<noyalib::Value> for FieldValueRef` keeps int→`Float` widening and the
  `Tagged` arm. Template `to_yaml` appends the document-terminating newline
  noyalib omits (no `SerializerConfig` knob exists for it); `from_yaml` uses
  the shared config. Amendment: ticket 19 already carried the corrected
  wording (2026-09-27); the surviving echo was in `md-pkm-lsp/map.md`
  (corrected with a pointer to this ticket's Rationale); issue 20 contains no
  V1_1 claim (grep-verified). `mise run verify` green: 2907 tests + 58
  doctests, clippy/fmt/audit clean; `cargo doc --all-features` clean with
  `-D warnings`. Two-axis code review findings addressed (variable naming,
  comment wording).
- **2026-09-28 (agent, review passes):** Three adversarial review tiers
  applied after initial implementation. Design-hardening pass: reduced
  `RawFrontmatter::as_str`/`parse` and `FrontmatterParseError` from
  `pub(crate)` to private (single same-file caller each); **deviated from
  the Cargo section's literal `strict-deserialise` feature listing** —
  `default-features = false, features = ["std", "fast-int", "fast-float"]`
  instead, since `strict-deserialise` only gates `from_str_strict`/
  `from_slice_strict`/`from_reader_strict`, none called anywhere in this
  codebase, and pulls in an unused transitive `serde_ignored` dependency;
  confirmed via `Cargo.lock` diff and codebase-wide grep. Consolidated two
  standalone string-stays-a-string tests into the existing `yaml_1_2_parity`
  `rstest` table. Architecture pass: extracted the duplicated
  `noyalib::from_str_with_config::<noyalib::Value>(text, &YAML_CONFIG)` call
  (previously inlined at both `RawFrontmatter::parse` and the template
  engine's `from_yaml`) into one `crate::yaml::parse(text) ->
  Result<noyalib::Value, noyalib::Error>`, letting `YAML_CONFIG` itself drop
  to module-private. `mise run verify` green after each pass: 2907 tests +
  58 doctests, clippy/fmt/audit/gitleaks clean; `cargo doc --all-features`
  clean with `-D warnings`.
- **2026-09-28 (agent, final passes and closeout):** Two more review tiers
  applied after the design/architecture passes above, plus a strict clippy
  sweep. `#[inline]` added to `crate::yaml::parse` (`src/yaml.rs`) — trivial
  one-line delegation on the per-note hot path
  (`files.par_iter().map(parse_note)` in `src/index/service.rs`), matching
  this module's own convention for trivial wrappers. Full 265-rule
  `rust-skills` checklist cross-checked line-by-line against the complete
  diff (all 26 categories: ownership, error handling, memory, API design,
  numeric safety, type safety, naming, testing, docs, performance,
  anti-patterns, etc.) — zero further findings; `LazyLock<ParserConfig>`
  first-access race under `files.par_iter()` confirmed sound
  (`std::sync::LazyLock`'s internal `Once` serializes exactly one
  initializer run, no soundness or cancellation-safety concern). Ran
  `cargo bench --bench note_parsing --features test-utils -- --quick
  "dense_frontmatter"` to substantiate no allocation/perf regression: dense
  frontmatter workloads at 17–34µs / 28–57 MiB/s, consistent with a
  behavior-preserving refactor (no stored `main` baseline existed in this
  worktree for a `critcmp` diff). Ran the maximal strict clippy sweep
  (`-D warnings -D clippy::all -D clippy::pedantic -D clippy::nursery
  -D clippy::cargo`) workspace-wide: 268 findings total, but every finding
  inside this diff's own files is `redundant_pub_crate` or
  `missing_const_for_fn` — both fire identically on pre-existing,
  untouched code across the *entire* crate (this project deliberately does
  not enable `pedantic`/`nursery`/`cargo` groups; the real gate,
  `mise run lint`, stays clean). No further changes made.

  Final state: branch `noyalib-frontmatter-swap`, 6 commits over `main`
  (`1667e267` swap, `6b605282` test, `de5c3348` cleanup, `f5d34da4`
  least-privilege/feature-trim/rstest-merge, `db430e7e` `yaml::parse`
  extraction, `28e92690` `#[inline]`), 723-line diff across
  `Cargo.toml`, `src/lib.rs`, `src/yaml.rs` (new), `src/field.rs`,
  `src/note/field.rs`, `src/note/metadata.rs`,
  `src/template/engine/yaml.rs`. `mise run verify` green throughout: 2907
  tests + 58 doctests, clippy/fmt/audit/gitleaks/conventional-commit clean;
  `cargo doc --all-features -D warnings` clean.
