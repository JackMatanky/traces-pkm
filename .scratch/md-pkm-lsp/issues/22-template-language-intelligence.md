# Template-language intelligence

Type: grilling
Blocked by: 08
Status: resolved

## Question

Grounding — this is the sharpest constraint in the whole map: **templates cannot be safely executed for analysis purposes.** `TemplateEngine` owns one shared, `debug`-enabled minijinja `Environment` (`src/template/engine.rs:71,117`); helpers are registered as `Object`-trait globals (e.g. `ui` at `src/template/engine/ui.rs:61-63`); the `ui.*` namespace's "No-Declaration Format" means templates call interactive prompts *synchronously mid-render*, blocking on a live `DialogProvider` (`src/template/engine/ui.rs:82-87`) — attempting to render a template with no human present either hangs or immediately errors (`DialogError::NotInteractive`). **There is no existing static-analysis capability**: no code path extracts "which helpers/variables does this template reference" without a full execution pass, and there is no root-template AST caching today (every render re-parses via `env.template_from_named_str`, `src/template/engine.rs:172-175`). Minijinja's own parser (`env.parse()`) is available but unused for this purpose today — confirmed via rust-docs-mcp against the `minijinja` crate if deeper API confirmation is needed.

Decide:
- The static-analysis approach: build a new (LSP-only, or shared-and-reusable) code path using minijinja's `Environment::parse`/AST inspection to answer "what helper calls, variables, and includes does this template reference, and where (spans)" without executing `ui.*`/`query.*`/etc. side effects at all.
- Completion: helper-namespace and function-name completion (`ui.`, `file.`, `date.`, `query.`, `tasks.`, `schema.` → member completion) — does this require typed knowledge of each namespace's method signatures (hand-maintained metadata table, since minijinja `Object`s don't expose a reflectable schema) or can it be derived some other way?
- Hover: showing a helper function's signature/doc on hover — same "hand-maintained metadata" question.
- Diagnostics: minijinja's own parse errors already preserve spans (`env.set_debug(true)`, `TemplateError::Render` preserves the source `minijinja::Error`, `src/template/error.rs:89-96`) — decide whether these get surfaced live (on every keystroke, via the static-parse-only path) as LSP diagnostics, distinct from the CLI's render-time error diagnostics.
- Whether `{% include %}`-referenced templates get "go to definition"/completion for template names, reusing `TemplateLoader::find`'s existing local→global resolution precedence (`src/template/loader.rs:85-99`).

## Answer

24 decisions locked across four grilling rounds (Q1–Q6, Q7–Q16, Q17–Q22, Q23–Q24), each stress-tested by a 2–3-subagent adversarial/technical pass (14 source reports total, all merged into the research file and deleted).

### Grounding supersession (required by ticket graph MUST#5)

- **`env.parse()` → `machinery::parse`**: analysis uses minijinja's machinery API behind the `unstable_machinery` feature gate, exact-pinned `=2.24.0`; the free fn returns `ast::Stmt` (top node `Stmt::Template(Spanned<Template>)`, `parser.rs:1392-1399`), companion `parse_expr`. Every node is `Spanned<T>` (struct over a private boxed tuple, `.span()`/`Deref` — no pattern-match on `(node, span)`); offsets are UTF-8 bytes composing with ticket 11's `BytePos` ladder.
- **Names derived, docs hand-authored** (not reflected): minijinja `Object`s expose no reflectable schema; filter/test enumeration is Rank-1 derived via `env.state().iter_{filters,tests}()` probes, **filtered through `find`** (offered ⇒ find-resolvable, D5/S1), cross-checked in tests against hand-written docs — golden-span tests, exact version pin, `_`-match arms banned so a new minijinja variant breaks loudly.
- **Corrected line refs** recorded in research §0.
- **Supersession block**: ticket 08's "20's territory" attribution for schema-change rebuild is superseded (gap = un-owned open item, owner candidate 31 — see §3.5). **Q7's "eager = startup-only" is NOT superseded** — Q23(a′) reinstates it with a non-fatal qualifier; only the Q17-lazy knock-on claimed otherwise (withdrawn).

### Static analysis approach (Q1–Q6)

- **(a) `unstable_machinery` gate** — full-parser reuse over a hand-rolled scanner; hand-rolled full parser rejected for grammar divergence (minijinja's upstream COMPATIBILITY.md), hand-rolled *token scanner* remains a gate-failure fallback (no-AST tier).
- **Placement: shared, engine-adjacent — `src/template/analysis.rs`** (D2 restated as a protocol-free invariant: analysis depends on minijinja only, no LSP types); concrete placement is a ticket-34 input (workspace split / `pub(super)` widening / gate cost — inputs line applied to 34's Question).
- **Metadata = derived names + hand-authored docs + cross-check** (filters/tests included, per Rank-1).
- **Diagnostics live-debounced** (not on-save): recompute on `didChange` through the single-lane publisher (Q19 β), timing provisional to 25 (`map.md:31`).
- **D5 soundness invariant**: completion-offered ⇒ `find`-resolvable, scope S1 (exact-name forms).
- **No persistent AST cache**: parse per recompute; derived-analysis memo lives in 33's LRU tier within existing caps; caller-side invalidation via `clear_templates()`; env-handle/invalidation ownership decided in Q19.

### Completion & hover (Q11, Q12, Q21)

- **Q11/Q21 — tiered hover return `Doc(entry)` / `Identified(kind, name)` / `None`**: which tiers render = 26's call; `Identified` only for kind-verified entities (probe-derived kind or table entry), unknown-root attributes → `None` by construction; kind rule: non-empty `Str` enumerator ⇒ "namespace", empty enumerator + registered-in-globals ⇒ "function", else "value". Markdown safety = inline-code wrap (identifiers ASCII+`_`).
- **Q12 — narrow fix, SURVIVES**: drop ambiguous stems in the shared `list_available` via `retain(|stem| find(stem).is_ok())` **after the local/global merge** (naive per-dir filtering is a no-op and cross-dir unsound). Zero test changes + one new regression test; update doc comments (`loader.rs:237-246`, `service.rs:77-84`). **CLI delta: listing behavior changes — user sign-off recorded here (no CLI ticket exists).**

### Diagnostics & include navigation (Q9, Q10, Q13, Q14)

- **Q9(c) — phase-1 four classes**: syntax + name-resolution + typo (probe with close match) + undefined-variable, **iff** Q18's span mechanism. Conditions: `undeclared_variables(false)` (nested=true defeats subtraction), subtract `env.globals()` keys, empty-render-context soundness condition recorded (`engine.rs:195`), includer-context FP recorded as known limitation, severity proposed Information → 25.
- **Q18 — span mechanism**: scope-correct span-collecting mini-tracker re-walk (`nested=false` only, ~100–150 lines on top of D1's existing adapter walk); the `undeclared_variables` name set demoted to gate + `#[test]` oracle only, dropped from production (one parse per recompute).
- **Q10 — fallback range = span-precedence ladder** (owned by 22): (1) nearest AST-node span, (2) `Error::range()`, (3) `0..len` only as tripwire; all `BytePos`, boundary-mapped; cosmetic choice proposed to 25.
- **Q13 — buffer-authoritative content reading** whenever content reading lands (transitive analysis/preview): served by 14's `ContentResolver::text_of`, overlay handle injected at engine construction; phase-1 reads no include content today, so overlay-vs-disk is moot for 22 now.
- **Q14 — extends/import/from in discovery scope**: uniform across runtime/codegen/loader; dynamic targets (`{% extends layout %}`) honest false-negative, named so nobody "fixes" it with string-sniffing; `file.include(...)` named as excluded (different resolution path).

### Data source, cadence & delivery (Q7, Q8, Q15, Q16, Q19, Q20)

- **Q7 — one env per analysis host** (30); eager cost accepted as startup-only (confirmed under Q23(a′)).
- **Q8 — no edge to 34**: conditional re-open triggers recorded (analysis leaves `src/template/`; gate must ride an LSP-only feature; `derived_names()` visibility under chosen crate graph); inputs-pending line applied to 34's Question.
- **Q15(c) — data-source guarantee scoped as API contract**: env-derived tier total, doc-derived tier `Option`, diagnostics exempt (owned rule 2), cursor-context parse-dependent → 24's degrade call; **third explicit state** (construction-failure/intelligence-disabled) distinguishable from empty-by-success — keyed off the **stored `Err` record**, not engine shape (`Ok(full)` / `Ok(empty-registry, Err recorded)` / `Err(no env)`), threaded to 24 + 26.
- **Q16 — three confirmations**: (a) class+content 22's, severity → 25; (b) must-carry sentence into 25's inputs; (c) drift risk accepted with proximity forcing function.
- **Q19 — β confirmed**: compute-on-lane + long-lived background sleep/publish task holding owned payloads + `Client` clone, find-only analysis resolution, single-owner `&mut` env (N=1); FIFO messages `Compute/Clear/Opened` with coalesce-by-URI, publish-gate on open/version/no-newer-pending, per-URI reopen version reset (prevents ghost diagnostics); single-publisher-per-URI invariant proposed to 25. Amendment text: `Amends 12 §1` (cite `12:46`; §3 refresh exclusivity unaffected), `Amends 14 §2` scope-limited to the template-diagnostics publication path (14's 150ms sleep stands), 09 = one-line clarification, not an amends.
- **Q20(b) — overlay-aware discovery**: existence = set union of disk entries and open-buffer paths; virtual-child rule inside `find`'s per-directory two-phase check (precedence unchanged); **`find` and `list_available` adopt the seam together**; one code path two configs — `Option<Arc<RwLock<…>>>` overlay handle injected only by the LSP, CLI constructs `None` (disk-only, zero delta); `didClose` removes the name; untitled buffers never participate (stated limit).

### Construction policy (Q17 → Q23(a′) chosen)

- **Eager attempt at `initialize`, non-fatal**: store `Err`, degrade, record. Failure ladder, three rungs: **(0)** config absent/untrusted (`ConfigService::load` fails non-interactively) ⇒ no Config ⇒ no loader ⇒ parse-only tier, env off; **(1)** schema dir resolve/load fails ⇒ empty-schema-registry fallback engine (env alive, registry empty) + failure flag; **(2)** full. **State exposure mandatory**: the failure flag accompanies the fallback data ("degraded, schema registry unavailable, data = empty") — depth A = schema-tier-empty + flag; depth B = env-tier-off list (syntax/resolution/loader features survive; filter/test/global/hover/undef-var off).
- **Fallback needs two additive APIs**: `SchemaService::empty()` + engine-level seam (no `Option` plumbing through ops).
- **Surface**: `window/showMessage` + `window/logMessage` (spec's named pre-`InitializeResult` exception, framework-legal) **and** `tracing::warn!`; toast carries schema-dir path + disabled scope + "until restart". Optional v2 = range-less schema-file diagnostic, owed to **25's inputs row** (20 is resolved).
- **`reset_engine()`**: one named API sharing initialize's full construction path (Config → loader → engine), re-injects the same overlay `Arc` from state, called only by lane handlers; v1 = sticky-until-restart; schema-dir `didChangeWatchedFiles` trigger owed to 31 (flagged line, not edge).
- **Two-policy note + seam**: `ConstructionPolicy { FailClosed, DegradeEmptyRegistry }` at the shared constructor — CLI passes `FailClosed` (three fail-closed tests untouched), LSP passes `Degrade` (**zero existing test coverage — new tests required**). **CLI delta: CLI stays fail-closed; no CLI ticket/owner exists; out of scope to change — user sign-off recorded here.** If the A3-split (schema-free env build + registration step) is adopted later, it subsumes this fallback — record, don't build both.

### Wiring (Q22/Q24 + research §5)

- **Edges: exactly {24, 25, 26} ← 22** (their Questions contain items unanswerable without 22). **27 gets no edge — Q24(b)**: flagged line appended verbatim to 27's Question (include/extends spans as documentLink candidates + overlay-aware existence + re-evaluate-the-edge clause); **timing fallback: if 27 has been claimed/resolved before this lands, restate it at 27's resolution.** 34 gets the sibling inputs-pending line (Q8). 32←22 never added (flag rides 22→25→32).
- **Edge-vs-line policy (stated once):** edges = inputs answerable-today-but-blocked; flagged lines = optional/deferred inputs (27, 31, 34).
- **Downstream owed rows** (full text in research §5): 24 (data source + staging + trigger + ordering + degrade line incl. disabled state), 25 (one inputs list: class list, source/code set, fallback-ladder rule, severities incl. undef-var FP rationale, cadence proposal, single-publisher/β coordination, SchemaLoad class, construction-failure degradation note — env-free classes only, silence-not-ranges), 26 (hover content + tiered contract + degrade line incl. disabled state), 12/14/09 timer amendment texts, 13 (`Amends 13`), 33 (`Amends 33`: eager KB-scale construction on the cold-start path + failure-degrade axis), 20 (construction-failure surface: v1 vehicle named, two-SchemaService fact), 30/31/18 lines.
- **Verified clean**: 10's facade consistent with D2; 09's runtime/DialogProvider consistent with Q7/Q9; 19's hover/kind split intact; map standing constraints `:22/:23/:24/:29/:31` all respected; no ticket loses an edge.

### Research

Consolidated: `research/22-template-intelligence.md` (637 lines, §0–§9) — grounding corrections, four round-decision sections with stress verdicts, downstream wiring table, Round-4 answers. Fourteen source reports (4 grounding + 10 stress) merged here and deleted.
