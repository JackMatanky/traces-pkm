# 07: Remaining parity — shorthands, parse_with, file.day

**What to build:** The last parity odds and ends land: bucketing helpers (`sow`/`eow`/`soy`/`eoy` and start/end-of-month) arrive on both relevant seams — as query `FilterFunction` registry entries (spec story 4's start/end-of-week/month/year helpers) and as template shorthands — and `weekday(n)` (ISO Monday) works in templates; ISO-8601 duration offsets (`P1M`, `P-1M`) are accepted on date shorthands like `date.now` by adapter translation, not a second grammar; `reference`/`reference_format`-style parsing goes through one `parse_with(text, fmt)` with a `Result` contract; notes expose `file.day` for day-bucketed views.

**Blocked by:** 04, 05, 06 (spec parity ordering 05 → 06 → 07; shorthands delegate to the calendar owner — 03, reached via 04; `parse_with` builds on the recognition interface).

**Status:** ready-for-agent — X1(i)/item-12/item-15/file.day closed 2026-10-05 (see Comments); brief attached
**Category:** enhancement — all items are new parity surface; nothing cited is broken-today behavior.

Skills: `rust-integration-testing`, `rust-skills`. Rules: edge strategies named per workflow; `conv-tryfrom-fallible`; adapter translation (ISO-8601 offsets) at the shorthand seam, not in the grammar. Design record: `../review.md` §3 (B11, B12, B26/B30; T1, T2, T3), §9 (S9+S11+S12); spec stories 4, 6, 10–11, 34.

- [ ] Bucketing helpers available as query registry entries — **value-returning, not predicates** (X1(i) closed 2026-10-05): `sow`/`eow`/`soy`/`eoy` plus start/end-of-month (B11 — Dataview's query-side bucketing functions, L14960; spec story 4 requires start/end-of-week/month/year), reachable through spec's decided minimal value-expression surface (field refs, literals, value-returning registry calls whose args are expressions, `+`/`-` on date/duration incl. `date − date` → duration, `* number`, comparisons; left-to-right) so `sow(note.date) = …` compares; start of week is ISO Monday
- [ ] The same bucketing helpers exposed as template shorthands — **aliases only** (closed 2026-10-05): `sow`/`eow`/`soy`/`eoy` as short aliases over the long forms; the long month forms already exist, so no new month surface; no new function bodies beyond the alias definitions — ticket 07 owns both seams per spec story 4 (05 defers them here)
- [ ] `weekday(n)` works with ISO Monday convention (T3)
- [ ] Seam 3 rule: shorthands and `weekday(n)` are tested through the template seam only — never by invoking the engine adapter directly
- [ ] Local-clock display through the template seam — **consume/verify ticket 02's pin** (restated 2026-10-05): `now`/`today` render the local wall clock while storage stays UTC (spec Seam 3, story 34). 02 delivered the clock logic (local-clock reads, TZ-injected in-module engine tests) — this item authors no new clock code; its work is verifying the contract with an integration case at the template seam. Gap: 02's coverage is in-module engine tests, not a seam-3 integration assertion — if none exists, this item adds it
- [ ] ISO-8601 `P1M`/`P-1M` accepted on date shorthands via adapter translation; `P-1M` internal-sign handling deliberately not ported (T1)
- [ ] `parse_with(text, fmt)` returns `Result` with a documented contract — one implementation serving query `reference` and future LSP needs (B12/T2); no lenient guessing
- [ ] The `reference`/`reference_format` consumer is wired end-to-end through its seam — B12/T2 demonstrated by a user-facing case, not just the library function
- [ ] `file.day` computed at index time, exposed to queries (B26/B30) — **precedence decided 2026-10-05**: the filename-derived date wins over other candidates, with `created_at` as fallback (the Dataview rule; the index computes this)
- [ ] Integration cases cover each workflow and edge failure (bad format string, missing date field, out-of-range week number) or state an out-of-scope reason
- [ ] `sow`/`eow`/`soy`/`eoy` and `weekday(n)` are built from chrono primitives (`NaiveDate::iso_week()` / `from_isoywd_opt` / weekday accessors) — no epoch-day division for weeks (Dataview `.week` footgun)
- [ ] `mise run verify` green

**Review amendments (2026-10-05 adversarial rust-design pass; source: `../review.md` §11):**

- [ ] **(X1 cascade — closed 2026-10-05, option (i)).** The query-side bucketing helpers that *return* a date for comparison (`sow(note.date) = …`) land as value-returning registry entries reachable through spec's amended minimal value-expression surface — no predicate restatement, no Dataview-parity caveat; stories 1–3 stand. Nothing in this ticket treats the expression surface as undecided anymore. Template-side shorthands (item 12) unaffected — seam 3 already owns them.
- [ ] **(blocker shape)** `Blocked by: 04, 05, 06` is correct as a schedule, but the calendar-owner dependency is a seam, not just a wait: shorthands delegate to 03's `apply`/`shift` *through* 04's recognition interface. If 04 lands late, 07 cannot even stub the delegations.

## Comments

> *This was generated by AI during triage.*

## Triage Notes (2026-10-05)

**What we've established so far:**

- Category `enhancement`, state `needs-info` — not `ready-for-agent`: all three blockers (04/05/06) are unresolved, item 11 is contingent on 05's open X1 decision (the ticket itself says to read that decision first), and two checklist lines are stale or already delivered (below).
- Claims verified at current HEAD: `sow`/`eow`/`soy`/`eoy` absent repo-wide; `weekday(n)` arg form absent (existing `weekday` is no-arg); `iso_week()`/`from_isoywd_opt` unused; `parse_with` absent (nearest are crate-private fixed-pattern `DateFormat`/`DateTimeFormat::parse`); `file.day` absent (file accessors are path/name/folder/size/timestamps/tags only); `now`/`today` already render the local wall clock via ticket 02's `local_now()`.
- Redundancy check: partial overlaps found — template `start_of_month`/`end_of_month` filters already exist; ticket 02 already delivered local-clock display. Core asks (bucketing helpers, `weekday(n)`, `parse_with`, `file.day`) have no implementation anywhere.
- Prior rejection: no `.out-of-scope/` directory exists — not a previously rejected request.

**What we still need from you (@maintainer):**

- **X1 cascade (from 05 — surface only, do not decide here)** — do the query-side bucketing helpers (item 11) become value-returning registry entries (spec option i) or function-only predicates (option ii)? The Dataview parity claim (L14960) reads differently under (ii).
- **Item 12 scope** — template-side start/end-of-month already exists as filters (`start_of_month`/`end_of_month`); confirm item 12 means only the `sow`/`eow`/`soy`/`eoy` shorthand aliases, or is it already satisfied as written?
- **Item 15 status** — `now`/`today` local-clock rendering was delivered by ticket 02 (its checklist is `[x]`): drop the item, or restate it as a seam-3 integration pin only?
- **`file.day` derivation rule** — unspecified: filename/daily-note pattern, frontmatter date, or created/mtime date? With what fallback order?

> *This was generated by AI during triage.*

**2026-10-05 (decisions — maintainer):**

1. **X1 closed (option (i)).** Spec L77 now names the minimal value-expression surface: field references, literals, value-returning registry calls whose args are expressions, `+`/`-` on date/duration (incl. `date − date` → duration), `* number`, comparisons; left-to-right; no boolean algebra beyond the filter combinators. Stories 1–3 stand. Anything in this ticket that treated the expression surface as undecided is now settled: the bucketing helpers land as **value-returning registry entries** reachable through that surface. This answers the X1 cascade in Triage Notes above.
2. **Item 12 → aliases only.** The week/year boundary helpers ship as short aliases (`sow`/`eow`/`soy`/`eoy`) over the long forms; the long month forms already exist, so there is no new month surface. No new function bodies beyond the alias definitions.
3. **Item 15 → seam-3 pin delivered by ticket 02.** The pin this item asked for is delivered by `02-local-zone-clock.md` (status `resolved`, merged `90696f25`): local-clock doctrine + TZ-injected tests. Restated to consume/verify that pin rather than authoring a new one. Judgment note: 02's coverage is in-module engine tests, not a seam-3 integration assertion — the restated item keeps that need explicitly (verify, and add the integration case if absent; no new clock code).
4. **`file.day` precedence.** The filename-derived date wins over other candidates for `file.day`, with `created_at` as fallback (the Dataview rule — the index computes this).

## Agent Brief

**Category:** enhancement
**Summary:** Land the remaining temporal parity odds and ends — query-side bucketing helpers as value-returning registry entries, template shorthands as aliases, `weekday(n)` (ISO Monday), ISO-8601 duration offsets at the shorthand adapter, one `parse_with(text, fmt)` `Result` contract wired to its reference consumer, and `file.day` on notes with the decided filename-first precedence.

**Current behavior:**
The bucketing helpers (`sow`/`eow`/`soy`/`eoy`, start/end-of-month) exist on neither seam: the query registry exposes no such functions, and the template filter set has only the long month forms. `weekday` takes no argument. Date shorthands reject ISO-8601 duration offsets (`P1M`, `P-1M`) outright — there is no adapter translation. Reference-style parsing has no single `parse_with` entry with a `Result` contract; format parsing is pattern-specific and duplicated. Notes expose file path/name/folder/size/timestamps/tags accessors but no `file.day`. The local-clock display doctrine for `now`/`today` already exists (delivered by the local-zone clock ticket); what remains is verifying it through the template integration seam.

**Desired behavior:**
- Query side: bucketing helpers are value-returning registry entries reachable through the decided minimal value-expression surface (field references, literals, registry calls whose arguments are themselves expressions, `+`/`-` on date/duration incl. `date − date` → duration, `* number`, comparisons; left-to-right; no boolean algebra beyond the filter combinators). Comparisons like `sow(note.date) = …` work. Start of week is ISO Monday, computed from chrono ISO-week primitives — never epoch-day division.
- Template side: `sow`/`eow`/`soy`/`eoy` are short aliases over the long-form helpers — alias definitions only, no new function bodies on this seam; the long month forms already exist, so no new month surface.
- `weekday(n)` works in templates under the ISO Monday convention, tested through the template seam only (never by invoking the engine adapter directly).
- ISO-8601 `P1M`/`P-1M` are accepted on date shorthands by adapter translation, not a second grammar; `P-1M` internal-sign handling is deliberately not ported.
- `parse_with(text, fmt) -> Result` has a documented, strict contract (no lenient guessing) — one implementation serving the query reference-format consumer and future LSP needs — and that consumer is wired end-to-end through a user-facing case.
- `file.day` is computed at index time: the filename-derived date wins over other candidates, `created_at` is the fallback (Dataview rule), and the field is queryable.
- `now`/`today` local-clock rendering with UTC storage is consumed and verified through a template-seam integration case — no new clock code.

**Key interfaces:**
- Bucketing helper registry entries (query): value-returning, arguments are value expressions; built on chrono `NaiveDate::iso_week()` / `from_isoywd_opt` / weekday accessors
- Template shorthand aliases: `sow`/`eow`/`soy`/`eoy` delegate to the long forms; `weekday` gains an `n` argument
- `parse_with(text, fmt)`: the single `Result`-returning format-parsing entry with a documented error contract
- `file.day`: index-computed field with filename-first, `created_at`-fallback precedence
- Shorthand adapter: ISO-8601 duration-offset translation (`P1M`, `P-1M`)

**Acceptance criteria:**
- [ ] A filter expression calls each bucketing helper and compares its result (e.g. `sow(note.date) = …`) — value-returning through the decided expression surface, pinned by an integration test
- [ ] Start-of-week returns ISO Monday for a pinned input spanning a week boundary
- [ ] Each helper is reachable as a template shorthand alias `sow`/`eow`/`soy`/`eoy` delegating to the long form, with no new month surface and no function body on this seam beyond the alias definitions
- [ ] `weekday(n)` returns the ISO-Monday-based weekday in templates, pinned through the template seam (the engine adapter is never invoked directly)
- [ ] `P1M` and `P-1M` are accepted on date shorthands via adapter translation; no second duration grammar exists
- [ ] `parse_with` returns the parsed value for a matching format and `Err` for a bad format string or non-matching input — no lenient guessing — pinned by test
- [ ] The reference-format consumer works end-to-end through its seam (a user-facing case, not just a library call)
- [ ] `file.day` equals the filename-derived date when present, falls back to `created_at` otherwise, and is queryable — pinned by test
- [ ] `now`/`today` render the local wall clock while storage stays UTC, asserted by a template-seam integration case built on the existing clock logic (no new clock code)
- [ ] Week math comes from chrono ISO primitives — no epoch-day division for weeks — pinned
- [ ] Each workflow and edge failure (bad format string, missing date field, out-of-range week number) maps to an integration case or an explicit out-of-scope reason
- [ ] `mise run verify` green

**Out of scope:**
- The expression-surface implementation itself (ticket 05) — consume the decided surface, do not build it
- Authorship of the local-zone clock / seam-3 pin (ticket 02) — already delivered; this ticket only verifies it
- CONTEXT.md clauses, ADRs, and divergence-register records (ticket 08)
- A second duration grammar for ISO-8601 offsets — translation at the shorthand adapter only
- Boolean algebra or operator precedence beyond the decided value-expression surface
- New crate-public exports
