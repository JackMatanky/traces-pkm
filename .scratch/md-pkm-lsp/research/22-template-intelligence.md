# Ticket 22 — Template-language intelligence: consolidated research

Consolidated (2026-09-30) from **fourteen** single-purpose reports fired as ticket 22's grounding and
stress passes: (pass 1) web prior art, prior-decisions review, codebase current-state (CodeGraph),
minijinja API (rust-docs-mcp), then three stress passes over Round-1's six decisions (ticket-graph,
technical red-team, adversarial); (pass 2) three stress passes over Round-2 answers Q7–Q16;
(pass 3) three stress passes over Round-3 answers Q17–Q22; (pass 4) two final pre-lock passes over
Round-4 answers Q23–Q24 (technical verification + adversarial/internal-consistency audit). All
source files merged here and deleted. Planning artifact only; no production code touched.

**Method notes.** Web claims carry URLs and keep the capability axis (what a tool's source does)
separate from the adoption axis (what the ecosystem recommends). Rust/crate claims come from
`rust-docs-mcp` and *cached crate sources only*. Code citations are to the working tree as of
2026-09-29/30; minijinja citations to 2.24.0 source cached under
`.rust-docs/cache/crates/minijinja/2.24.0/source/`. **V** = verified against source; **R** =
carried from an earlier pass; **I** = inference. Pass-2 verdicts: no decision FLAWED outright;
Q12/Q14/Q16(a)/Q16(c) SURVIVE, the rest AMEND (amendments folded below). Pass-3 verdicts: Q17
FLAWED as specified (intent survives), Q18/Q19 AMEND, Q20(b)/Q21 SURVIVE, Q22 contested. Pass-4
verdicts: **Q23(a′) AMEND (mechanism survives all four attacks), Q24(b) SURVIVE** — amendments
folded into §9.

---

## 0. Grounding corrections — ticket claims vs verified fact

| # | Ticket claim | Verdict |
|---|---|---|
| 1 | *"Minijinja's own parser (`env.parse()`) is available"* (issues/22:9) | ❌ **No `Environment::parse` exists** in any probed version. The only public parser is `minijinja::machinery::parse(source, filename, SyntaxConfig, WhitespaceConfig) -> Result<ast::Stmt<'_>, Error>`, gated behind the **non-default `unstable_machinery` feature** ("no semver guarantees"; repo `Cargo.toml:115` uses defaults only). Sub-question (i) is "opt into an explicitly-unstable cargo feature vs hand-rolled walk". |
| 2 | *"parse errors already preserve spans (`set_debug(true)` …)"* | ✅ **Confirmed, refined**: parse errors carry name + 1-based line + **byte range** even without `set_debug(true)` (parser attaches unconditionally, `parser.rs:1383-1389`; `Error::range()` = `repr.span`, `error.rs:304-308`; `set_debug(true)` — already on, `engine.rs:121` — matters for *render-time* `template_source()`). ❌ **No `Error::column()`** — derive from `range().start` + `LineIndex`. |
| 3 | *"Objects don't expose a reflectable schema → hand-maintained metadata table"* (issues/22:13) | ⚠️ **Half wrong.** Member **names** ARE runtime-derivable: `env.globals()`; every repo namespace implements `Object::enumerate()` → `Enumerator::Str(METHODS)` with existing drift tests. **Signatures/arity/docs are genuinely not reflectable** (`Value::from_function` erases; sealed `Function`; name-string `call_method`). Derive names, hand-author docs. |
| 4 | *"no root-template AST caching; every render re-parses"* | ✅ Confirmed (`engine.rs:194`; minijinja caches only *compiled* loader-loaded templates by name, never ASTs). Refinement: **includes/extends ARE memoized across renders** (`minijinja loader.rs:118-141`) — stale include content unless `env.clear_templates()` runs. Further (pass 2, V): **`template_from_named_str` does not populate the cache at all** (`minijinja environment.rs:420-433`) — analysis-style parsing is cache-neutral; only `get_template` fills the memo; only `clear_templates(&mut)` empties it. |
| 5 | *"render with no human present either hangs or immediately errors"* | ⚠️ **Imprecise.** Non-TTY: `TerminalDialogProvider` silently returns *fallback values* — no hang/error, but plausible-false data (`src/dialog/terminal.rs:7-18,50-108`). **Parse-only analysis avoids all of it by construction.** |
| 6 | Include go-to-def should reuse `TemplateLoader::find` | ✅ Sound. `env.get_template()` proves resolvability but returns no path; use `find` for locations. |
| 7 | Ticket line refs `engine.rs:71,117` / `172-175` | Drifted: struct `:74`, `set_debug` `:121`, `template_from_named_str` `:194`. |

### Corrections to this research itself (all passes)

- **C1 — loader rules corrected:** `find_path_in` does **exact `dir/<name>` only, any extension**
  (`loader.rs:122-129`); stem-match via `find_name_in` runs **only when the name has no extension**
  (`:148-150`), over direct children, any extension, **case-SENSITIVE** (`file_stem() == Some(key)`
  — plain `OsStr` equality, `loader.rs:174`; zero case-folding anywhere in `src/template/`), ≥2 hits
  → `AmbiguousTemplate` (`:182-192`). **There is no `index.md` rule and no implicit `<q>.md` probe.**
  *Pass-2 fix: earlier wording here said "case-insensitive" — wrong; load-bearing for Q12's fix
  spec, which must mirror `find` exactly or recreate the list/find divergence it fixes.*
- **C2 — no uniform `Stmt::span()`:** `Expr::span()` uniform; `Stmt` needs the adapter's ~20-arm
  match.
- **C3 — "ADR 0001/0005 register hover/completion behind feature flags" was unsupported** (pass-2
  grep: no such content) — claim dropped.
- **C4 — "assignment tracker explicitly out of scope" is half-true (pass 2):** minijinja's
  `find_undeclared` already ships a complete `AssignmentTracker` (loop targets, `set`/block,
  `with`, macro/import aliases, scope push/pop — `minijinja compiler/meta.rs:218-291`); 22 doesn't
  write *that*. What 22 still owes for a spanned undefined-variable diagnostic is **span
  acquisition** (the API returns names only) — see Q18.
- **Loader false-positive bug:** if `daily.md` and `daily.txt` coexist, `list_available` emits
  `daily` but `find("daily")` → `AmbiguousTemplate` (`:89-98` + `:173-191`, asserted by test
  `:422-446`) — today's list promises a name `find` rejects.

Two load-bearing consequences of §0-1: **(a)** AST access requires
`features = ["unstable_machinery"]`; **(b)** static "variables referenced" is available
*feature-free* via `Template::undeclared_variables(nested)` (see Q9 for its traps).

---

## 1. The static-analysis question (sub-question i)

### What the minijinja AST gives us (2.24.0, behind `unstable_machinery`)

- Entry: `machinery::parse` returns **`ast::Stmt`** (top node `Stmt::Template(Spanned<Template>)`
  wrapping `Template { children: Vec<Stmt> }` — pass-3/4 correction, `parser.rs:1392-1399`,
  `ast.rs:56-57,289-291`); companion `parse_expr`. **AST is not `Clone`, borrows `&'source str`**;
  `Send+Sync` automatic.
- **Every node is `Spanned<T>` — a struct with private `inner: Box<(T, Span)>` reached via
  `.span()`/`Deref` (`ast.rs:16-37`), not a public tuple alias; no pattern-match on `(node, span)`** —
  offsets **UTF-8 bytes** (compose with ticket
  11's `BytePos(u32)`/`LineIndex`); line 1-based; **col 0-based chars — never send minijinja
  cols to the wire** (UTF-16 mismatch). `Expr::span()` uniform; `Stmt` needs C2's match.
- Nodes: `Include { name, ignore_missing }`, `Extends`, `Import`/`FromImport`, `Call` +
  `identify_call() -> CallType::Method(&Var("ui"), "text_input")`, `GetAttr`, `Filter`, `Test`,
  `Var`. **Member-name span not stored** — derive as `span.end - name.len() .. span.end` (assert
  ASCII; all repo `METHODS` are). `Expr::as_const()` folds only pure literals/const-ops —
  `_ => None` for `Var`/`GetAttr`/`Call`/`Filter`/`Test` (`minijinja compiler/ast.rs:210-237`) →
  dynamic targets are **provably silent** (no FP possible); a literal `"a" ~ "b"` folds (correctly
  literal). The adapter must still distinguish literal / static-candidate-list / dynamic (the VM
  really tries lists, `vm/mod.rs:836+`).
- **`ignore missing` is runtime-only** (VM swallows the miss; `Import`/`FromImport` compile with
  `Include(false)` — imports have no ignore-missing, `codegen.rs:395-417`). Static analysis must
  read the AST flag.
- `compiler::meta` (referenced-names) is **not** re-exported → the adapter writes its own
  traversal; **no visitor API** ships. **Real cost = traversal**: ~250–400 lines over ~20 `Stmt` +
  ~15 `Expr` variants; **forbid `_` match arms** (a new body-carrying variant must break the build).
- **Default features load-bearing**: `Stmt::Include/Extends/Import` are `#[cfg(multi_template)]`
  etc. — *minijinja's* cfgs. Never `default-features = false`. Repo never customizes syntax →
  derive `SyntaxConfig` from env via one helper, `Default` fallback documented. Feature cost ≈ zero
  (compiler modules always compiled; adds re-export + Debug impls; no optional deps); never stable.

### Parse errors as diagnostics (feature-free path)

`ErrorKind::SyntaxError` only from the parser; compile-and-map-Err yields **parse-time diagnostics
with name+line+byte-range, zero new deps, no execution** (miette precedent `src/query/error.rs:106-120`).
`Error::range()` has "no absolute guarantee" → **span-precedence ladder, owned by 22 (Q10)**.

### Caching position

Nobody persists template ASTs — cache *derived* per-file indexes, rely on cheap reparse (µs–sub-ms).
Fit with 33: memo in the **LRU tier**, inside <100MB/<500MB; completion <20ms, hover <10ms,
diagnostics <20ms p95. Include-memo staleness (§3.5) is correctness, not perf.

### Constraints this branch must respect

Static-analysis-only default (render dry-run escalates 09); never-fail parsing (rust-analyzer
precedent, extended to the template path); reuse over parallel models (`map.md:23`). **Named
tension (`map.md:27` vs `:23`)**: unstable-feature reuse vs hand-rolled scanner — hand-rolled full
parser rejected for grammar divergence (minijinja's upstream COMPATIBILITY.md); hand-rolled *token scanner* remains the
honest feature-free fallback tier (in-repo precedent: logox note lexer, query grammar, 21's depth
counter); churn mitigated (adapter + pin + tests), not eliminated.

---

## 2. Completion & hover metadata (sub-questions ii–iii)

### Derivable vs hand-authored

| Need | Derivable? | Source |
|---|---|---|
| Namespace/global names | ✅ | `env.globals()` (functions share globals) |
| Namespace member names | ✅ | `Value::as_object()` → `DynObject::enumerate()` → `Enumerator::Str(METHODS)` |
| Filters/tests — **existence** | ✅ | `minijinja::tests::is_filter`/`is_test` with `env.empty_state()` |
| Filters/tests — **enumeration** | ⚠️ | **No public `filters()`/`tests()`** — see Rank-1 strategy below |
| Row-field accessor names | ✅ | `ACCESSOR_NAMES` consts (`src/query/grammar/field.rs:53,110,162`) |
| **Signatures / docs** | ❌ | erased by `Value::from_function`; no tool anywhere reflects them |

`enumerate()` is pure (repo-impl property, not a minijinja guarantee) — don't call `get_value`.

### Filter/test enumeration — Rank 1 (recommended; survives both passes)

Problem: registration-time capture sees only this repo's ~46 registrations and **zero** of
minijinja's ~46 builtin filters / ~57 builtin tests (incl. aliases). **Rank 1: hand builtin consts
(pinned to 2.24.0) ∪ repo registration names, probe-filtered via `is_filter`/`is_test` at startup,
failures dropped.** Oracle in `#[test]` only: `format!("{:?}", env)` — `impl Debug for Environment`
unconditionally prints BTreeMap **keys** (`environment.rs:86-96`, `utils.rs:475-481`) — assert
probe-filtered set == scraped set. Production depends only on stable public probes + our consts.
Ranks 2/3/5/6 rejected (runtime format-scraping in prod; capture-only has no builtin coverage and
no oracle; build-script scraping breaks offline). **Diagnostics never enumerate**: probe the
AST-found name directly (O(1), always correct).

### Cross-check — what it can and cannot guard

- Table ⊆ engine: probe every key at startup (soft log) + hard `#[test]`. **Never panic in
  release** — log + degrade.
- Engine ⊆ table: Rank-1 Debug-scrape oracle, `#[test]` only.
- **Signature/doc staleness is NOT guarded** (names only): an *accepted drift risk* with a
  proximity forcing function (D3's key-set-equality tests vs `const METHODS` make add/remove force
  the table open; table lives beside impls; signature edits ride the impl PR). Failure mode if it
  drifts anyway: stale hover *text* — display-only; completions/diagnostics unaffected (probe-
  validated). Do not claim prevention the check cannot provide.

### Helper inventory (raw material)

`ui` 4 members (`engine/ui.rs:34`), `file` `write_to`/`include` (`file.rs:40`), `date` 5
(`date.rs:53`), `query`/`lists`/`tasks` `from(expr?)` one impl three globals (`query.rs:89`),
`schema.get` + bound Schema 4 (`schema.rs:75,80`), `QuerySet` 9 (`query.rs:310-344`), non-Object
registrations ~40 filters/tests + `uuid` (`engine.rs:157-165`), plus minijinja builtins.

### Prior art on metadata (web)

No tool reflects docs/signatures from a live engine — unanimously hand-written registry (jinja-lsp
layered Core<Custom<Pack<Hint>), upstream docs copy (jinja-ls), CI-generated pinned JSON
(Shopify `theme-liquid-docs`), in-template comments (Liquid `{% doc %}`). **Templater's `tp.*`**:
two-level staging, regex trigger, **documentation-as-data**. Hand-authored table is the industry
standard; startup cross-check + probe filter are our anti-drift additions. No Obsidian-plugin LSP
precedent.

### Downstream contracts

- **24 owns** shared completion + trigger reconciliation; 22 supplies data + staging proposal +
  **trigger `.` request** (collides with rumdl's `(`,`#`,`/`,`.`,`-`; `#` with 16/21 — 24
  reconciles, 32 documents). Precedent: 21 filed triggers for 24 (`issues/21:49`).
- Explicit `textEdit` ranges; <20ms p95; **local-before-global** ordering (`prefers_local_over_global`,
  `loader.rs:777`).
- **Hover content** = 19's safe-markdown subset + plain-text fallback (`issues/19:75`);
  **`MarkupContent` kind negotiation = 26's** (`issues/26:12`); hover <10ms p95. *(C3: drop the
  ADR-0001/0005 feature-flag claim that previously sat here.)*
- Scope boundary: filter/test/namespace completion applies **outside query-string contexts** (21's
  cursor-in-query flag owns those).

---

## 3. Diagnostics & include navigation (sub-questions iv–v)

### 3.1 Cadence — what 22 decides vs what is 25's

Precedent: theme-check `checkOnOpen/Change/Save` + `onlySingleFileChecks`; gopls ~1s; jinja-lsp
per-keystroke version-dedup. In-repo: 20 uses on-open+300ms+on-save; 33 forbids per-keystroke.
**Hard constraints:** `map.md:31` — **25 owns trigger policy, numbers, severities, aggregation,
push-vs-pull, code namespacing** (25:12 verbatim). 22's content: the *capability*, source/codes as
**inputs**, and two semantic rules it owns (§3.2). Frame cadence as a **proposal to 25**: recompute
on `didChange`, parse pass is µs–sub-ms.

**Delivery + timer (settled shape, β — needs user confirm as Q19):** 14's answer specifies
`tokio::time::sleep` *inside the handler* (`issues/14:25`); under `concurrency_level(1)` (12's
decision) a sleeping handler occupies the only dispatch slot → HOL-blocks completion during the
window → threatens 33's p95. Pass-2 findings: **ticket 12 contains no literal background-task ban**
(it decides sequential dispatch, stale-cancellation, sequential refresh; even pre-opens "background
refresh can be layered on later", `issues/12:46`) — so the timer is an *addition* to 12/14, not a
contradiction; and **`Environment<'static>` is empirically Send+Sync** (verified by throwaway
compile; `MemoMap` Mutex-backed, loader `Fn + Send + Sync`, globals `Arc<BTreeMap>`), but
`clear_templates(&mut)` is impossible through a shared `Arc`.
**Recommended shape β:** the dispatch-lane handler **computes diagnostics synchronously** (µs parse:
`machinery::parse` is an env-free free fn / `template_from_named_str` cache-neutral; probes against
the env it owns) and hands **(version, diagnostics)** to one long-lived task whose only job is
**sleep + version-gated publish** via a `Client` clone. Env stays single-owner with working `&mut`
invalidation; the task touches only owned data. **22's Answer must carry explicit `Amends 14 §2`
(in-handler sleep superseded for the diagnostics path) and `Amends 12 §1/§3` (exactly one
long-lived debounce-task exception to sequential dispatch) paragraphs, plus a one-line
clarification to 09's tokio scope (debounce timer = dispatch machinery)**; alternatives B (14's
in-handler sleep — correct ordering, HOL cost) and C (sync per-keystroke — needs 25 to re-scope
33's ban) priced in one line each. β is close to option C, so it is a **25 coordination point**.

### 3.2 Diagnostic classes and the two owned semantic rules

- **Phase-1 classes (Q9(c), amended):** syntax; include/extends/import resolution failures;
  helper/filter name typos (existence probes); **optional undefined-variable with three stated
  conditions** — see Q18. Render-only classes (query/schema/dialog/arg failures, `error.rs:85-98`)
  out of static scope.
- **Owned rule 1:** `ignore missing` never an unconditional error (severity *proposed* ≤
  Information → 25). Import/from resolution failures ARE always errors (no ignore flag exists).
- **Owned rule 2 (SCOPED, graph MUST#6):** while a document fails to parse, **22's own
  parse-dependent classes** (resolution, name-typo, undefined-variable) are suppressed. This must
  NOT read as blanket authority over every source on the document: suppression of *other* sources'
  diagnostics (e.g. 21's query diagnostics) on an unparseable document is **25's aggregation
  policy — a proposed input, not 22's decision.**
- **`AmbiguousTemplate` is its own class** (not "not found"): warning + `relatedInformation`
  listing candidates (`candidates: Vec<PathBuf>`, `path.rs:209`) — severity → 25. The class/content
  is 22's; severity routing is 25's (Q16(a) SURVIVES).
- `source: "traces-template"` + codes emitted at the **LSP protocol boundary** (map.md:24),
  registered into 25's namespacing; cross-checked vs rumdl `MDxxx` by 32; distinct from CLI render
  codes (`cli/error.rs:636-654`) by construction.
- **Two-server coexistence (Q16(b), amended):** no owner anywhere. Must-carry verbatim sentence
  into 22's inputs-to-25: *"template `.md` files are the document class where rumdl + Traces
  `publishDiagnostics` overlap is guaranteed; whether rumdl should lint template files at all is
  client-config territory."* Flag reaches 32 via 25 (22→25→32; **no direct 32←22 edge**), but given
  map.md:39's documented propagation-gap precedent, the flag must be restated at 25's or 32's
  resolution (edge-or-restatement mechanism) — "flag at resolution" without a mechanism is how the
  prior gap happened.
- Push-only (21's Phase 1).

### 3.3 Loader mechanics (C1-corrected)

- `find` (`loader.rs:85-99`): per directory (local first, global, deduped — test `:777`): exact
  `dir/<name>` any extension → else, only if name extension-less, **case-sensitive** stem match
  among direct children (any extension); 0→miss, 1→hit, ≥2→`AmbiguousTemplate`. **Ambiguity in the
  first directory aborts before global is reached** (`:89-98`). No `index.md`, no recursion.
- `list_available`/`stems_in` (`:248-291`): **top-level, `.md`-only** (`ext == "md"` case-sensitive),
  stems only — backs `--list` (`cli/template.rs:162-166`), picker (`:233-258`), `completions
  --list-templates` (`cli/completions.rs:93-98`); six loader tests (`:586-659`) assert this, all
  with **unambiguous fixtures** — no test asserts the buggy listing.
- All loader items `pub(super)` — `template::analysis` is a *descendant* of `src/template/`, so
  **`pub(super)` is already reachable with zero widening** if the module lives engine-adjacent
  (visibility inherits downward). Widening needed only if the module lives *outside* `src/template/`
  — an explicit ticket-34 input either way.
- `env.get_template` memoizes what it loads (feeds D6 staleness) — use `loader.find` for paths.

### 3.4 Discovery config

`TemplateConfig { local, global, output }` — plain dirs, no globs (`config/model.rs:259-293`);
default local `.traces/templates/`. One long-lived env per analysis host (Q7 amendment for 30);
`set_loader` guarantees no load until asked.

### 3.5 Stale state & invalidation (D6)

- **Mechanism = `env.clear_templates()` O(1)** (`environment.rs:364-366`, needs `&mut`). Engine
  rebuild strictly more expensive (re-pays eager schema load `engine.rs:133`) and needed only for
  schema/config changes — "invalidation" must not invite rebuild-by-reflex.
- **`RefreshPlan` has no hook site** (`collect` is `pub(super)` to `src/index`, `refresh.rs:277`;
  hanging template invalidation off it would invert index→template deps). **Wiring is caller-side**:
  the same LSP handler that drives 13's single-file `RefreshPlan` also calls invalidate —
  *parallel consumers of the event stream, never nested.* (`Amends 13` note owed.)
- **Trigger set matters more than the hook:** default local dir sits under `.traces/` which is in
  `IGNORED_DIRS` (`src/env_vars.rs:15`); global dirs can live **outside the Project Root**
  (`config/builder.rs:76-77`) — an index-refresh hook never fires in default config. Triggers:
  (a) `didChangeWatchedFiles` on local+global template dirs (**primary**); (b) `didChange`/`didSave`
  of an open template; (c) config change to `TemplateConfig` — **conditional on 31 granting live
  config reload; else restart-only.** Acceptance: verify `.traces`-pruned and outside-root dirs
  receive invalidation.
- **Memo keying by document version alone is forbidden** — drop all derived analysis on any
  template-dir event (A's include-resolution result goes stale when B changes, A's version never
  moved). Reparse is µs–ms.
- **Explicitly out of scope:** the engine's `SchemaService` (schema TOML changes → engine rebuild,
  ticket 20's territory; name it so readers don't assume "engine is fresh"). **Pass-3 attribution
  correction:** "engine rebuild on schema change → 20's territory" is **unsupported** — 20's Answer
  contains no reload/rebuild decision (0 grep hits for `TemplateEngine`/`rebuild` outside 22), 31
  covers config.toml only (not `.traces/schemas/`), and 10's facade `SchemaService` is load-once
  (`10:14`). Record the schema-dir propagation gap as an **un-owned open item** with owner candidate
  31 (owed line at its resolution), not as an attribution to 20. `IndexerService` and
  completion name sets need no invalidation (env immutable post-construction; `list_available`
  walks disk per call).
- **Env-handle ownership** → Q19 (the D6 decide-now).

### 3.6 Engine construction cost → Q7/Q23(a′)

`TemplateEngine::new` does eager `SchemaService::load_verbose` (`engine.rs:133`) — **the only
heavy/failing eager step** (`IndexerService::from` is cheap, `index/service.rs:42-51`) — and
**fails construction on malformed Schema TOML / unreadable dir / extends cycle** (`engine.rs:100-104`,
`service.rs:54-57`, `TemplateError::SchemaLoad`) even if the template never uses `schema.*`. Under
one env, a schema typo (a *transient editing state* in 20's workflow) would kill **all** template
intelligence including parse diagnostics (filters/tests resolve at runtime — compilation never
touches schema, `codegen.rs:748-763`). → construction policy is **Q23(a′)**: LSP path is
non-fatal (empty-registry fallback, §9); §3.6's kill-all consequence survives only on the CLI
side (fail-closed, tested — two-policy note, §9). Cost framing stands: cost stays eager.

---

## 4. Web prior art (compressed)

| Tool | Source says (capability) | Ecosystem says (adoption) |
|---|---|---|
| alex-oleshkevich/jinja-lsp | Static-only over tree-sitter; layered hand-written doc registry; per-file derived index + version-deduped publishes | ~0 stars — reference design only |
| uros-5/jinja-lsp | tree-sitter-jinja2 (**no minijinja dep**), ropey incremental reparse | ~176 stars — the Jinja LSP people try first |
| noamzaks/jinja-ls | Error-tolerant parse; signature help; `{# @param #}` docs | 16 stars; Django Forum demand |
| Shopify Liquid | Official CLI-shipped LS; CI-generated pinned doc JSON; `checkOnOpen/Change/Save` all true + `onlySingleFileChecks` | Highest adoption |
| gopls templates | Real parser; **parse-errors only**, hover TODO | Official but opt-in, default off |
| djlint (`jinja` profile) | **T040** flags missing include/extends names *because "the engine only raises at render time"* | **The documented recommendation for MiniJinja users** (~950 stars) |
| Symfony Twig LS | Boots app kernel to introspect | Contrast: unavailable to us |

Cross-cutting: hand-maintained/generated docs beat reflection; strict parsers debounce + tolerate
"no diagnostics while broken" (never error-recover inside the engine's parser); live-debounced
single-file + cross-file-on-save is the norm; cache derived indexes, not ASTs; include resolution =
roots + file index (ours: reuse the loader as single authority); static variable knowledge needs
sibling/host scanning or annotations; diagnostics + completion + include nav is already
best-in-class. **No dedicated minijinja LSP exists** — first-of-kind; generic Jinja grammars can
diverge (minijinja's upstream COMPATIBILITY.md) — reason to use the engine's own parser despite the gate. Key URLs
(verified Sep 2026): gopls go.dev/gopls/features/templates, djlint docs `languages/jinja`,
github.com/alex-oleshkevich/jinja-lsp, uros-5/jinja-lsp, noamzaks/jinja-ls,
Shopify/theme-liquid-docs.

---

## 5. What 22 owes downstream / wiring status

**Edges:** 24←22 ✓ · 25←22 ✓ (added 2026-09-30) · 26←22 ✓ (added 2026-09-30) · 22←08 ✓.
**27←22: none — decided Q24(b)** (flagged line in 27's Question, applied at lock; 27 stays
edge-free per `issue-tracker.md:27` — an edge would re-block an unclaimed ticket for optional
input; re-evaluate only if 27's Question gains a template-links item). **Policy recorded (for
the Answer): edges = inputs answerable-today-but-blocked (24/25/26); flagged lines =
optional/deferred inputs (27, 31, 34).** **Do NOT add:** 32←22 (flag rides 22→25→32), 34←22 /
22←34 (Q8), edges to resolved tickets (amendment notes).

| Downstream | Owed |
|---|---|
| **24** | data source, staging proposal, trigger `.` request, local-before-global ordering, Q15(c) degrade hand-off line **incl. the construction-failure disabled state** (24 blocked-by 22 ✓) |
| **25** | **one inputs list**: phase-1 class list, `traces-template` source + code set, fallback-range ladder rule (BytePos, boundary-mapped), proposed severities (`ignore missing` ≤ Information; `AmbiguousTemplate` Warning; **undefined-variable ≤ Information with its known-FP rationale — includer-context partials + empty-context soundness condition + empty-set-on-failure trap**), cadence proposal (recompute on `didChange`), rule-2 suppression-of-other-sources as *proposal*, two-server overlap sentence verbatim, **single-publisher-per-URI + β timing as coordination point**, **SchemaLoad surface class if adopted**, **construction-failure degradation note: during degradation 22 publishes only env-free classes (syntax + resolution); degrade silences classes, never ranges (Q10's ladder unaffected); env-free syntax parsing falls back to `SyntaxConfig::default()` when no env exists (repo never customizes syntax — `SyntaxConfig` is env-derived today, §1); degraded-state surface = SchemaLoad class (v2) / showMessage (v1)** — all provisional per `map.md:31` (25 blocked-by 22 ✓) |
| **26** | hover content only; `None`-for-no-doc framed as template-kind instance, generic "hover with nothing to show" rule deferred to 26; **tiered producer contract `Doc(entry)` / `Identified(kind, name)` / `None` — which tiers render = 26's call (Q21)**; Q15(c) hand-off line **incl. the construction-failure disabled state**; kind negotiation stays 26's (26 blocked-by 22 ✓) |
| **12 / 14 / 09** | timer paragraphs: `Amends 12 §1` (dispatch — **cite §1, not §3; §3 refresh exclusivity unaffected; cite `12:46` pre-existing license**), `Amends 14 §2` **scope-limited to the template-diagnostics publication path (14's 150ms content re-parse sleep stands)**, **09 = one-line clarification note, not an amends** (timer = dispatch machinery inside 09:22 scope); **carry A6's FIFO/coalesce/epoch design + single-publisher invariant; mark β timing provisional to 25** (Q19) |
| **13** | `Amends 13`: invalidation invoked by the same handler as single-file `RefreshPlan`, never via a hook |
| **33** | `Amends 33`: derived-analysis memo lives in the LRU tier within existing caps/budgets; **engine construction runs eagerly at `initialize` — ON the cold-start path (Q23(a′)): one `load_verbose` of the schema TOML dir (`engine.rs:132-133`), KB-scale schema parse, duplicating 10's facade load of the same dir (two-SchemaService fact), `IndexerService::from` cheap (`index/service.rs:42-51`) — noise vs 33's <2s@20K budget dominated by the rayon scan, non-fatal so a broken schema dir costs only the failed attempt; failure-degrade mode = one added axis in 33's degradation table (genuinely net-new — the table is the memory-pressure trio at `33:85-88`)** |
| **20** | **construction-failure surface: name a real vehicle — 20 has NO schema-TOML diagnostics channel (its `traces-schema` diagnostics are note-frontmatter only); v1 = `window/showMessage`/log, optional v2 = range-less schema-file diagnostic owed as a **25 inputs-row input** (20 is `Status: resolved` — an owed input to a resolved ticket is unactionable; §5's 25-row SchemaLoad line is the landing spot, optional `Amends 20` if 20's deferred-positions record needs widening; v1 needs no routing); plus re-init owner = schema-dir `didChangeWatchedFiles` riding 10's refresh (owed to 31's resolution; 31 covers config.toml only, 20 has no dismissal event); two-SchemaService fact stated (facade instance 10:31 + template-engine instance `engine.rs:133` — propagation must touch both)** (Q17, re-decided Q23(a′)) |
| **30** | "one env **per analysis host**" phrasing |
| **31** | trigger (c) conditional on live config reload; **schema-dir propagation gap named as open item owed to 31's resolution** |
| **18** | named consumer of template enumeration/path-validation (S1 semantics) for "Create from template"; **existence check overlay-aware (Q20(b))** |
| **34** | **inputs-pending line applied** at lock (sibling vehicle with 27's): module placement, `pub(super)` widening scope, `unstable_machinery` gate + CLI-build cost (no edge — Q8) |
| **27** | **flagged line applied** (decided Q24(b)): appended to 27's Question at lock, immediately after its documentLink bullet — include/extends spans as documentLink candidates + overlay-aware existence (Q20(b)) + "decide at resolution whether `documentLink` covers template references or leaves them to go-to-definition" + the re-evaluate-the-edge clause; **no `Blocked by: 22` edge**; timing fallback = if 27 is claimed/resolved before the line lands, restate at 27's resolution |
| **own Answer** | **grounding supersession block** (graph MUST#5): `env.parse()`→`machinery::parse`; names-derived/docs-hand-authored; corrected line refs (§0); **plus §3.5's "20's territory" superseded — Q7's "eager = startup-only" NOT superseded (a′ reinstates it with the non-fatal qualifier; only the Q17-lazy knock-on claimed otherwise — withdrawn, see §9)** |

Verified clean (pass 2): 10's facade consistent with D2; 09's runtime/DialogProvider consistent
with Q7/Q9; 19's hover/kind split intact; map.md :22/:24/:29/:31 all respected; no ticket loses an
edge.

---

## 6. Round-1 decisions with stress-1 verdicts

*(Unchanged from pass 1 — all three pass-1 verdicts were amend/reconsider→amend; amendments
integrated above: D1 exact pin + golden-span tests + `_`-arm ban + degradation path + package-wide
gate tied to 34; D2 = protocol-free invariant decides, working placement `src/template/analysis.rs`
is a 34 input, needs `pub(super) fn derived_names()` accessor, never expose `&Environment`; D3 =
derived names + hand-authored ~25-entry phase-1 table + Rank-1 filter enumeration + cross-check
names-only; D4 = capability vs 25's ownership split + timer placement + two owned rules + boundary-
attached codes; D5 = soundness invariant "offered ⇒ find-resolvable" + S1 scope + prefix-driven
depth + CLI-surface option (Q12) + extends/import scope (Q14); D6 = no AST cache + LRU memo +
caller-side invalidation + `clear_templates()` + decide-in-22 ownership.)*

---

## 7. Round-2 decisions (Q7–Q16) with stress-2 verdicts and amendments

| Q | Decision | Verdict | Strengthened form |
|---|---|---|---|
| 7 | One environment | **AMEND** | One env **per analysis host** (30). Eager cost accepted as startup-only — **confirmed under Q23(a′)** (construction runs at `initialize` = startup) and amended to non-fatal + empty-registry fallback. **Construction policy = Q23(a′)** (Q17 was FLAWED). Env-handle/timer shape = **Q19** (β recommended). |
| 8 | Don't sequence 34 | **AMEND** | Survives; record as **conditional re-open triggers**, not "tension": (i) if analysis lands outside `src/template/` (workspace split), zero-widening is void — re-run visibility calc before coding; (ii) if 34 picks single-crate, gate must ride an LSP-only feature so CLI-only builds opt out — *CLI carrying the feature is not accepted by 22, it's a cost input 34 may reverse*; (iii) `derived_names()` visibility re-checked under chosen crate graph. Reason recorded: **34 changes plumbing, not semantics; map is planning-only so plumbing resolves before implementation** (not "one ticket per session"). |
| 9(c) | Phase-1 = syntax+resolution+typo+undef-var | **AMEND** | Keep all four **iff** Q18 (span mechanism) is answered. Conditions: `undeclared_variables(false)` (**nested=true returns dotted paths that defeat subtraction**); **subtract `env.globals()` keys or the check flags `ui`/`date`/`query`/`uuid` on nearly every template** (verified: minijinja "does not special case global variables", `template.rs:419-424`); empty-render-context coupling recorded (`engine.rs:194`, so after subtraction a flagged name really is undefined at render); includer-context FP recorded as known limitation (partials reading includer-set vars — `find_undeclared` skips Include/Extends, `meta.rs:276`); severity proposed Information → 25; empty-set-on-failure (`template.rs:433-435`) documented, rule 2 sits above it. C4: minijinja's own assignment tracker exists — 22 owes only spans. |
| 10 | Fallback range owned by 22 | **AMEND** (ownership survives) | **Span-precedence ladder, not one value**: (1) nearest AST-node span (include/extends/filter/Var); (2) `Error::range()`; (3) document range `0..len` **only as a tripwire** — class-3 firing indicates a bug in (1)/(2), not a normal path (whole-file-as-general-fallback would make Q9's name-only class the common squiggle = noise). All `BytePos`, mapped at boundary; cosmetic choice *proposed* to 25. |
| 11 | Hover-without-entry → `None` | **AMEND** | **Difficulty question answered: trivial either way** — position→entity is shared with D5 (both need entity-at-offset; `None` saves no work); delta = one `match` arm in 22's content producer; **not entangled with 26** (hover is not in 26's resolvable-entity abstraction, `26:10` vs `:12`); Markdown safety = inline-code wrap (identifiers ASCII+`_` under default features, `lexer.rs:183-196`). **Strengthened shape: tiered return `Doc(entry)` / `Identified(kind, name)` / `None`** — user's `None`-lean preserved for docs (no table entry ⇒ no doc payload; no token echo), but returning `Identified` keeps 26's presentation choice open (a hard `None` forecloses 26 rendering "filter — docs unavailable"; kind is already probe-derived, so `Identified` is a *checked* fact, not an echo). Which tiers render = 26's call. |
| 12(a) | Narrow fix: drop ambiguous stems in shared `list_available` | **SURVIVES** | Verified: no test/flow depends on the buggy listing (all six listing tests use unambiguous fixtures; no multi-extension fixture repo-wide); picker has no disambiguation flow; CLI ambiguity UX is the error path with candidates (`cli/error.rs:618-620,689-694`); `map.md:23` argues against the LSP-only alternative; user sign-off here = the CLI-delta record (no CLI ticket exists). **Implementation amendment:** `retain(|stem| find(stem).is_ok())` **after the local/global merge** — naive per-dir duplicate-filter is a no-op (ambiguity always involves a *non-listed* sibling like `daily.txt`) and unsound (cross-dir: local `daily.txt`+`daily.tpl` ⇒ `find` aborts at local before global, `:89-98`); `retain(find)` also honors exact-name precedence (`daily`+`daily.md` is NOT ambiguous) and `map.md:23` reuse. Zero test changes + **one new regression test**; update doc comments (`loader.rs:237-246`, `service.rs:77-84`). Optional enhancement (not a condition): list the exact `.md` name instead of pure omission — keep consistent with S1's exact-name forms. |
| 13 | Buffer-authoritative include content, reconsidered with 14 | **AMEND** | **Reconsideration result: not an open question — a scope-extension correction to 14.** 14 is **resolved** with a transient-replacement model + `ContentResolver::text_of` facade ("editor content replaces disk while open", `issues/14:21,27`) that **never mentions templates** (0 grep hits). Three statements replace the deferral: **(i)** *phase-1 reads no include content* — resolution is existence via `find` only; `undeclared_variables` ignores includes — so overlay-vs-disk is **moot for 22 today**; **(ii)** *when content reading lands (transitive analysis, preview, `get_template` resolvability proofs), it is buffer-authoritative* — bound by `map.md:22`, served by 14's `ContentResolver::text_of`, never direct disk read; loader seam = overlay handle (`Arc<RwLock<HashMap<Url, BufferState>>>`, 14's map) injected at engine construction into `TemplateLoader::load`'s sole disk read (`path.rs:160-162` via `engine.rs:122-125`); `didClose` → map removal reverts to disk automatically; `Amends 14` note: overlay scope includes open *template* documents; verify `text_of` serves non-indexed paths (template dirs can sit outside Project Root); **(iii)** *existence of unsaved-new templates* = **Q20**. |
| 14 | extends/import/from in discovery scope | **SURVIVES** | Verified uniform at three layers (runtime: all four resolve via loader callback; codegen: import/from → `Include(false)`, `codegen.rs:395-417`; loader docs promise include/extends). FP attack fails on verified `as_const` (`_ => None` for Var/Call/Filter) → dynamic targets silent by construction. **Two named notes:** (a) **exclude `file.include(...)`** — different resolution path (`SafeRelativePath`, root-relative, `engine/file.rs:7-17,78-83`), named so it doesn't silently fall out of scope; (b) honest false-negative: `{% set layout = "base.md" %}{% extends layout %}` is silently undiscoverable — correct trade (silence never lies), named so nobody "fixes" it with string-sniffing. |
| 15(c) | Data-source guarantee; degrade→24/26 | **AMEND (×2)** | Guarantee must be **scoped as API contract**: *env-derived tier total (globals/members/filters/tests/table — never fails, never empty-by-accident); doc-derived tier `Option`/tiered, returns `None` when the document fails to parse; diagnostics exempt by design (owned rule 2); cursor-context detection ("am I inside `{% include "`?") is parse-dependent → 24's degrade call.* Document the `undeclared_variables` empty-set-on-failure trap (indistinguishable from "clean"). **Pass-3/4 amendment: a third explicit state is required — construction-failure/intelligence-disabled, distinguishable from empty-by-success (Q23(a′)'s empty-registry fallback is exactly this collision: failed-load data is byte-identical to a schema-less vault — the distinction keys off the stored `Err` record, not engine shape: state machine `Ok(full)` / `Ok(empty-registry, Err recorded)` / `Err(no env)`, exposed even when the fallback serves data; thread it to 24's degrade call and 26's render choice).** Hand-off line: "while unparseable, data source stays live (contract above); degrade-vs-suppress UX = 24 (completion) / 26 (hover)". Deferring the *facts* would be wrong; deferring the *UX policy* is correct. |
| 16 | Three confirmations | **(a)(c) SURVIVE; (b) AMEND** | (a) class+content = 22's, severity → 25 — already correct. (b) keep flagging, but verbatim must-carry sentence into 25's inputs + restatement/edge at 25/32 resolution (see §3.2). (c) stated as **accepted drift risk with a proximity forcing function**; failure mode display-only; never imply the convention *prevents* drift. |

---

## 8. Round-3 decisions (Q17–Q22) with stress-3 verdicts and amendments

Pass-3 verdicts: **Q17 FLAWED as specified (intent survives — re-decided as Q23(a′), chosen),
Q18/Q19 AMEND (convergent, adopted below), Q20(b)/Q21 SURVIVE, Q22 contested → resolved (b).**

### Q17(a) — lazy construction + degrade + "surface via 20's diagnostics" → FLAWED (mechanism), intent survives — **re-decided: Q23(a′) chosen**
Both pass-3 technical and adversarial converge on the same three defects; the *intent* ("server
never dies") is untouched:
- **The named channel does not exist.** 20's diagnostics are per-*note* frontmatter validation
  (`SchemaValidator::validate(&Note, &Schema)`, `issues/20:57`, kinds `20:109-117`) — no
  schema-TOML/construction-failure class anywhere in 20 or any ticket; today's failure path is
  `tracing::warn!` (`src/schema/service.rs:289-301`). **v1 vehicle = `window/showMessage` +
  server log, once per failed construction**; optional v2 = range-less diagnostic on the schema
  TOML URI, owed to **25's inputs row** (net-new capability; 20 deferred schema-file positions — but 20 is `Status: resolved`, so the obligation lands on 25, §5 20-row).
- **Failure surface is narrower than "template features off".** The only fallible steps are
  `resolved_schema_directory()?` and `load_verbose?` (`engine.rs:132-133`); everything else is
  infallible. Survives pre/post-failure: **syntax diagnostics (env-free `machinery::parse`),
  loader-based resolution/goto-def/template-name completion** (`TemplateLoader` is Config-derived,
  never touches env). Off: filter/test/global/namespace completion, hover docs, undef-var
  (globals subtraction needs `env`). → degrade is **env-tier off**, not everything off — and this
  tier is also D1's honest feature-gate fallback.
- **Sticky failure, no reset owner.** `OnceLock::get_or_try_init` is unstable (local rustc probe;
  stable pattern = `get_or_init(|| Result)` + `take(&mut)`, or simply
  `Mutex<Option<Result<…>>>` uncontended under N=1). No trigger exists: schema-dir watch is not in
  D6's set, 31 covers config.toml only, 20 has no dismissal event. **v1 = sticky until restart,
  with one named `reset_engine()` API** so whichever trigger lands later (schema-dir
  `didChangeWatchedFiles` riding 10's refresh — owed to 31) calls the same site; construction
  failure and schema-change rebuild must not fork two replacement paths.
- **Repo friction to own:** fail-closed is a *tested product decision* —
  `a_broken_schema_now_breaks_construction_even_when_the_template_never_touches_schema`
  (`src/template/engine/schema.rs:1215`, plus `:1072`, `:1093`, comment "Accepted behavior
  change"). An LSP fail-open forks policy from the CLI fail-closed: defensible (one-shot process
  vs long-lived server) but must be an **explicit two-policy note**, not silent divergence.
- **Chosen (Q23(a′)):** eager attempt at `initialize` + non-fatal failure (store `Err`, degrade,
  record) + **empty-schema-registry fallback** (registry absent → `schema.*`/`query` member
  knowledge empty, every other global alive — `SchemaOps`/`QueryOps` take `Arc<SchemaService>`,
  `engine.rs:138-157`, so the split is structurally available; **no empty-registry constructor
  exists — needs two additive APIs: `SchemaService::empty()` (private fields, no `Default`) +
  an engine-level seam (`new_with_schema_service` / construction-policy param); no `Option`
  plumbing through the ops**) + **rebuild trigger owed to 31's resolution — v1 sticky until
  restart** — no `OnceLock` state machine; also avoids first-use hover-cost (first
  `load_verbose` inside an interactive request against 33's p95). Rejected alternative: lazy
  `Mutex<Option<Result<…>>>` first-use construction (Q17(a)) — pays that first-use cost and
  keeps the state machine. **Stress-4 amendments (folded):**
  - **Failure ladder, three rungs — split "degrade env-tier" per rung:** (0) config absent/
    untrusted at initialize (`ConfigService::load` can fail — `LocalConfigAbsent`,
    `Untrusted`, non-interactive, `service.rs:202-218`) ⇒ no Config ⇒ no loader
    (`From<&Config>`, `loader.rs:43`) ⇒ **parse-only tier, env off**; (1) schema dir resolve/
    load fails ⇒ fallback engine, **env alive**, registry empty + failure flag recorded; (2)
    full.
  - **State exposure + two named degradation depths:** the failure flag is stored and exposed
    beside the fallback data (state = "degraded, schema registry unavailable, data = empty") so
    Q15(c)'s distinction survives — **depth A** (fallback): schema-tier-empty + flag, env alive;
    **depth B** (rung 0 / no fallback): the env-tier-off list above.
  - **Log sink named:** `Client::log_message` (editor Output channel) **and** `tracing::warn!`
    (parity with CLI construction warnings, `service.rs:289-301`).
  - **Message content (C3):** schema-directory path + what is disabled + duration ("until
    restart") — the toast carries the whole message; recovery under v1 = restart, say so.
  - **`reset_engine()` scope:** shares initialize's full construction path (Config → loader →
    engine; `TemplateEngine::new` is monolithic `engine.rs:110-166`, loader clone lives inside
    minijinja), **re-injects the same overlay `Arc` from state** when Q13(ii)/Q20(b) land, called
    only by lane handlers (never the β task), kept out of the cheap tier (`clear_templates()`
    stays the O(1) invalidation).
  - **Two-policy note needs a landing spot:** `ConstructionPolicy { FailClosed,
    DegradeEmptyRegistry }` (or `new_lenient`) threaded through the shared
    `TemplateService::new`/`TemplateEngine::new` — CLI passes `FailClosed` (three fail-closed
    tests untouched), LSP passes `Degrade`; **the `Degrade` path is LSP-only with zero existing
    test coverage — new tests required.** CLI stays fail-closed; no CLI ticket exists, so it is
    recorded as a CLI-delta for user sign-off.
  - **v2 range-less diagnostic obligation routes to 25** (20 is resolved — §5 20-row); optional
    variant (A3-split: schema-free env build + separate registration step) — **if adopted later
    it subsumes this fallback (record the interaction, don't build both).**
- **Knock-ons:** Q15(c) gains the third disabled state (§7); 33 gains "construction eager at
  `initialize`, non-fatal, KB-scale schema load duplicating 10's facade load" + a
  failure-degrade axis (§5); **Q7's "eager = startup-only" is confirmed, NOT superseded (the
  supersession entry was a Q17-lazy consequence — withdrawn)**;
  `warn_schema_construction_diagnostics` precedent already called at `engine.rs:134`.

### Q18(a) — undefined-variable spans: AMEND (convergent; outcome intact — class stays phase 1)
- **The stated mechanism was FP-unsound.** With `nested=false`, `find_undeclared` inserts at first
  lookup *and* assigns (`meta.rs:106-116`), and `Stmt::Set.target` is itself a spanned
  `Expr::Var` (`meta.rs:248-250`) — so a naive "highlight every `Var` whose name ∈ set" re-walk
  provably squiggles assignment sites (`{{ x }}{% set x = 1 %}`) and valid in-scope reads
  (`{% for x %}…{% endfor %}{{ x }}` → set `{"x"}` hits the in-loop read too).
- **Amended mechanism:** the re-walk **is** a scope-correct, span-collecting mini-tracker
  (`nested=false` only), emitting spans for its own flagged lookups; the `undeclared_variables`
  name set is demoted to (a) **gate** — tracker-minus-globals empty ⇒ class silent — and (b)
  **`#[test]` oracle only** (tracker names == `undeclared_variables(false)` − globals on
  fixtures — the same Rank-1 pattern as filter enumeration). Production then drops the
  `undeclared_variables` call entirely (it needs a *compiled* Template and re-parses internally,
  `template.rs:425-435`) → **one parse per recompute**. Globals subtraction at emission
  (`globals.contains(name)` skip) — mandatory either way (`template.rs:419-424`; builtins
  `range/dict/debug/namespace` are in `env.globals()`, `defaults.rs:226-251`).
- **Honest sizing: ~100–150 lines + golden tests on top of D1's existing adapter walk** (not
  "~50" — that was the unsound variant; full `meta.rs` ≈ 240 lines, `nested=false` trims it;
  D1's walk already exists and pays the feature gate).
- **Severity: Information-always proposed to 25** (suppression logic would cost more than the
  class and hide TPs; 25 may downgrade). **Known-FP recorded:** includer-context partials
  (`perform_include` shares state, `vm/mod.rs:831-875` — a partial reading an includer-`set`
  var is valid at render, statically flagged; `find_undeclared` skips Include/Extends,
  `meta.rs:276` — the tracker copies that arm for oracle parity). **Soundness condition:**
  empty render context holds *today* on every production path (`engine.rs:195`,
  `service.rs:270`) but `GLOSSARY.md`'s Template Variable + ADR-0001's "AI provides variables"
  anticipate a future non-empty context — condition recorded next to the glossary term, and it
  argues for Information, not for dropping the class. **No-AST tier (D1 degradation):** no
  undef-var class (name-only spans already rejected by Q10's ladder). Spans output
  `Range<BytePos>` only (`TryFrom<usize>` narrowing per 11 §3).

### Q19 — β confirmed: AMEND (delivery mechanics; env-ownership core survives the best attack)
**Survives:** single-owner env, background task holds only owned payloads + `Client` clone
(verified cheap clone, `client.rs:49-53`), find-only analysis resolution (D5's invariant is
find-based — reword §13(ii)'s "`get_template` resolvability proofs" to "future proof-needs run
buffer-authoritative through the same env"), bare-`&mut` invalidation, no race under
`concurrency_level(1)`. Citations now **locally verifiable**: tower-lsp-server 0.23.0 is in the
rust-docs cache (`Client` clone, `futures::channel::mpsc(1)`, `publish_diagnostics` with protocol
`version` param, `concurrency_level`) — flag only that the Cargo.toml version is unpinned.
Lane cost ≥10× headroom vs 33's 20ms (1 parse + ≤~10 probe lookups + ≤~5 `find`s + tracker walk).
**Amendments (A6):**
- **All template-diagnostic traffic routes through the task as FIFO messages from the single lane
  sender: `Compute(uri, v, diags)` / `Clear(uri)` / `Opened(uri)`.** Task coalesces by URI
  (publish-latest-wins — never publish a stale successor), publishes `Clear` immediately (preserves
  14's no-debounce close intent at one-message cost), gates `Compute` on *(URI still open) ∧ (v ≥
  last published) ∧ (no newer pending)* + per-URI reopen version reset. This is required, not
  optional: version-gating alone cannot catch `didClose` (no new version is issued) → **ghost
  diagnostics** after 14's synchronous clear; and handler-clear vs task-publish over `Client`'s
  multi-sender channel is nondeterministic (pass-3 finding — no NF glossary survives; client
channel is multi-sender, arrival order unobservable). Never discard a computation without a pending
  successor assumption stated (25 owns cadence — if 25 picks compute-on-save, discard = stale
  until next save).
- **Single-publisher-per-URI invariant:** `publishDiagnostics` is per-URI full-replace — 22's task
  must be the only writer for template URIs (other sources' diagnostics merged on the lane before
  hand-off), **stated as a proposal to 25**; if 25 subsumes publishing, the amends shrink
  (coordination point recorded, not assumed).
- **Amendment text precision:** `Amends 12 §1` (dispatch — cite `12:46`'s pre-existing license;
  §3 refresh exclusivity *unaffected*, only a cross-ref to §2's stale-discard analogue);
  `Amends 14 §2` **scope-limited to the template-diagnostics publication path** (14's 150ms
  content re-parse sleep stands; 14:25 is the map's only in-handler-sleep decision — 20/21/13/33
  state numbers only); **09 = one-line clarification note, not an amends** (timer = dispatch
  machinery inside 09:22's scope). Timing itself stays provisional-to-25 (`map.md:31`). β
  generalization to all sources = 25's three-tier model decision.

### Q20(b) — overlay-aware discovery: SURVIVES (both passes) with definitional amendments
Attacks that failed: YAGNI (`map.md:22` standing constraint *is* live intelligence — existence is
exactly what phase 1 reads; (a) would need a standing-constraint exception); "convenience only"
(option (a) creates transient false-positive "not found" diagnostics **and** makes Q12's
`retain(find)` delete unsaved-new names from completion — degrading two phase-1 outputs; nvim/Helix
`:e dir/new.md` flow has no disk file until `:w`, and the LSP is editor-agnostic; 18's
Create-from-template resolves through the same discovery); "extends resolved 14" (reuses Q13(ii)'s
conceded seam — reuse, not parallel model); "three-way ambiguity" (existence = **set union** of
disk entries and open-buffer paths, then find's existing exact-then-stem rules run over the union —
no precedence ladder to invent). **Adopted design:**
1. **Virtual-child rule:** overlay entry participates inside `find`'s existing per-directory
   two-phase check — exact when `symlink_metadata` misses (`:122-129`, any extension), stem merge
   in `find_name_in` (`:156-181`, same case-sensitive `file_stem() == key`, same ≥2 →
   `AmbiguousTemplate`), `stems_in` adds overlay children with `ext == "md"` (`:281-286`).
   Precedence unchanged (exact-then-stem within a dir, local-then-global across dirs) — overlay is
   not a third tier. Entries outside both configured dirs never participate (their per-document
   features still work).
2. **`find` AND `list_available` adopt the seam together** — single authority; otherwise the
   list/find divergence Q12 fixes reopens on a new axis and D5's "offered ⇒ find-resolvable"
   breaks for overlay names. `retain(find)` stays sound with zero changes.
3. **One code path, two configs:** overlay handle = `Option<Arc<RwLock<…>>>` injected only by the
   LSP at loader construction (same seam as Q13(ii)); CLI constructs with `None` → disk-only
   naturally. Satisfies `map.md:23`; CLI delta recorded like Q12's. `TemplateLoader::load` (render
   callback) needs **no** overlay — LSP never renders (09).
4. **Ambiguity defined:** overlay `daily.md` + disk `daily.md` same path = 14's transient
   replacement, not ambiguity; overlay `daily` + disk `daily.txt` same dir = genuine
   `AmbiguousTemplate` (candidates may include never-created paths — diagnostic-content layer, 22's
   class / 25's presentation, filter or accept noted); exact-name precedence unchanged.
5. **didClose:** unconditional removal (`14:23,33`) ⇒ closed-unsaved-new disappears from discovery;
   subsequent not-found is *true* (honors filesystem authority). Save-as path change = client
   didClose+didOpen (assumption stated). Untitled buffers (`untitled:` scheme) have no path — can
   never participate (stated limit).
6. **Amends 14 widened to discovery scope:** overlay includes open template documents **and
   template-name discovery** (`find`/`list_available`); `didClose` removal also removes the name
   from discovery; verify `text_of` serves non-indexed paths (template dirs may sit outside
   Project Root).
7. Cost: O(open docs) per call (<100 open files per 14); no cache, consistent with 14's no-LRU.

### Q21 — tiered hover `Doc`/`Identified`/`None`: SURVIVES (two owed sentences)
Attacks fail: clutter (which tiers *render* is 26's call — supplying the enum is the normal
data-flow, 26 blocked-by 22); over-commit (22 already owes 26 hover content; the enum *is*
content). **Owed sentences:** (1) `Identified` only for **kind-verified** entities (probe-derived
kind or table entry) — unverified positions fall to `None`, else it degenerates into token echo;
(2) **kind derivation rule:** non-empty `Str` enumerator → "namespace"; empty enumerator +
registered-in-globals → "function" (`uuid`, `range`… `ObjectRepr::Plain` + `Enumerator::NonEnumerable`,
`functions.rs:290-298` — `ValueKind::kind()` cannot split these, `value/mod.rs:1206-1223`);
else → "value"; unknown-root attribute (`qs.items`) → `None` by construction.

### Q22 — edge 27←22: contested → **resolved (b): no edge, flagged line**
Technical: no cycle (confirmed), no counter-indication. Adversarial: **courtesy edge-creep** —
27's Question has zero template content (`27:13` is one markdown link-items bullet), 27 is
unblocked *today* (05, 15 resolved), and `issue-tracker.md:27` means adding `22` (claimed,
mid-grilling) **re-blocks an open ticket** for input it may only optionally consume; contrast
24/25/26, whose Questions contain items unanswerable without 22. **Decided Q24(b):** the §5
flagged-line vehicle conveys the same knowledge at zero scheduling cost — drafted line applied to
27's Question at lock; timing fallback: if 27 has been claimed/resolved before the line lands,
restate it at 27's resolution (§3.2's edge-or-restatement rule for flags).

---

## 9. Round-4 answers (Q23, Q24) — decided, with stress-4 verdicts and amendments

1. **Q23 = (a′) chosen — eager attempt at `initialize`, non-fatal, empty-schema-registry
   fallback.** Verdict: **AMEND** (mechanism survives all four attacks; amendments folded into
   §8's Q17 section above). Attack record: (i) cold-start — 33 has *no* deferral expectation
   (schema KBs "always resident", `33:66/:81`; budget dominated by the rayon scan; eager-vs-lazy
   delta ms-scale vs a 2s budget, immaterial; eager symmetrically *removes* `load_verbose` from
   first interactive request); (ii) empty-registry masking — AMEND, fixed by exposing the stored
   `Err` beside the fallback data + naming depth A/B (LSP never renders, so only absent data, a
   one-time toast fires, v2 schema-file diagnostic named as durable surface); (iii) showMessage
   loud enough — SURVIVES: the LSP spec's lifecycle text names `window/showMessage` and
   `window/logMessage` as the *only* notifications permitted before `InitializeResult`
   (framework-legal via `send_notification_unchecked`); toast must carry path + disabled scope +
   "until restart"; (iv) two-policy note — principled (one-shot render vs long-lived analysis)
   and now has a landing spot (`ConstructionPolicy` at the shared constructor); record the
   no-owner fact: no CLI ticket exists, CLI stays fail-closed, user sign-off at 22's resolution.
   Three pass-3 obligations all present: named channel, `reset_engine()` + owed trigger (line,
   not edge — 31 is unblocked today), two-policy note + seam. The other three pass-4 obligations
   also load: two additive APIs, state keyed off stored `Err`, config-rung parse-only tier.
   Rejected (a-as-chosen): lazy `Mutex<Option<Result<…>>>` first-use — first-use cost inside an
   interactive request against 33's p95 + a state machine; "helps 33 further" buys nothing
   measurable. Optional A3-split: if adopted later it subsumes the fallback seam — record, don't
   build both.
2. **Q24 = (b) chosen — flagged line in 27's Question, no `Blocked by: 22` edge.** Verdict:
   **SURVIVES.** Line drafted voice-matched to 27 and applied at lock (new bullet immediately
   after 27's documentLink bullet, carrying the re-evaluate-the-edge clause); precedents:
   inline "(ties to ticket 22)" at `18:13` and `21:13`, coordination prose at `27:10`/`15:19`/
   `21:49`, sibling vehicle already applied to 34. Timing fallback: if 27 is claimed/resolved
   before the line lands, restate it at 27's resolution (§3.2's edge-or-restatement rule).
   Steelman answered: unlike map.md:39's cross-artifact gap, the flag lives *inside* 27's
   Question — the artifact 27's worker reads at claim time. **Policy stated once for the Answer:
   edges = answerable-today-but-blocked inputs (24/25/26); flagged lines = optional/deferred
   inputs (27, 31, 34).**

Both answers, plus record-and-hand-off material (§5) and the grounding-supersession block
(Q7 dropped from the supersession — §5 own-Answer row), are destined for 22's `## Answer`.
