# Comprehensive Review v4 — `src/date.rs` + `src/duration.rs`

Companion to `spec.md` (the tracker feature `temporal-core`): the full findings/analysis/deepening record. Tickets cite IDs from here rather than re-deriving them.

**Status:** decisions A2′, B1+local-naive, D-b(scoped), and D11-doc-widening are **accepted**. This document is the full findings/analysis/deepening record.

**Implementation status (amended 2026-10-05 — the original line here read "No implementation performed", which stopped being true):** tickets **01** (`c990f6c9`, resolved), **02** (`90696f25`, resolved), and **03** (`57ea1b09` + remediation `bcc938b5`, resolved) have landed. Consequences for this document: §1's adapter counts, §2.3's dead-surface rows, §4's format-basis lines, §9's S1/S2/S6/S10 rows, and §10's steps 2–3/5–6 are stale where marked below; tickets **04–09** remain `ready-for-agent`. Findings that arrived from the 2026-10-05 adversarial pass are in §11.

**Post-v4 evolutions (settled at spec time, marked inline below):** DST policy = ambiguous → `.earliest()`, nonexistent → shift forward (was the single open item); null ordering = keep current behavior, ADR replaces the code comment; the three test seams confirmed (value types / `FilterFunction` registry / template filters); calendar frame = local wall clock for day/week/month/year application (chrono `checked_add_*` + local round-trip; Temporal/Luxon/PG parity), sub-hour units exact, incoherence extended to `1d` vs `24h` across DST (pinned by test); week joins day as a calendar application unit (≡ 7 wall-days; moment/Temporal parity — research items 24–25); one gap-verified local→UTC resolver (`MappedLocalTime` exhaustive match; tz-data errors must not shift; rustdoc cited to Temporal `'compatible'`/RFC 5545 and jiff `Compatible`; wasm caveat chrono #1701 out-of-scope); format grammar formally bound to `chrono::format::strftime` (intra-doc link, no parallel invalid-pattern validator, `%+` forbidden, interop `…Z` via `to_rfc3339_opts(Secs, use_z=true)`), week bucketing mandated to `iso_week()`/`from_isoywd_opt`; external prior-art research completed (see `research/`), backing the divergence register.

---

## 1. Seam landscape

| Seam                | Module                                  | Adapters today                                                        | Missing / broken                                                     |
| ------------------- | --------------------------------------- | --------------------------------------------------------------------- | -------------------------------------------------------------------- |
| Recognition/parsing | `date.rs`, `duration.rs`                    | note lexer/inline/list, `TextShape`, field coercion, engine `ParsedDate`  | derived `Deserialize` bypasses it (N12); guard protocol leaks (N15/N7) |
| Formatting          | `date.rs` (`format_with`, `Display`)          | engine `format_precise` + engine's own `format_with`                      | moment-dialect input (D-b → S8); `durationformat` (B14 → S4)           |
| Calendar arithmetic | **no owner**                                | engine `date_shift_unit`/`date_diff` (copy), dead `checked_add`/`checked_sub` | query-side arithmetic, `weekday`, shorthands — all blocked on S1       |
| Unit registry       | `duration.rs` `UNIT_MAP`/`DurationUnit`       | duration parse, engine kwarg + shift                                  | regime declaration (N2 → S2); **4th silent ratio copy** (N18)            |
| Clock               | **no doctrine**                             | `Local::now` ×4, `Utc::now` ×3 (engine)                                   | naive-zone = local (decided), display policy (D9 → S10)              |
| Precision           | engine-private `DatePrecision`/`ParsedDate` | arithmetic re-serialization only                                      | recognition result should own it (N16/D10 → S7)                      |

Deletion test: every value type earns its keep (grammar + registry + error taxonomy reappear across ≥6 call sites). Row 3 is the structural root of N2/D1/D9; row 6 is a seam an adapter had to invent because the public one lost information.

**Table status note (2026-10-05, verified at HEAD `5b7dc748`):** rows **2, 3, 5** drifted — **row 2:** core `format_with` was deleted (only the engine's private copy at `src/template/engine/date.rs:304` remains; `rg "fn format_with" src/date.rs` → 0 hits), so `date.rs` no longer hosts a format-with adapter; `durationformat` still missing (S4, ticket 06). **row 3:** the calendar owner **exists** — ticket 03 landed `shift`/`apply`/`diff` on the value types (`src/date.rs`, shift at `:1143` + folds at `:1176-1206`), and `checked_add`/`checked_sub` are **deleted** (`rg "fn checked_add" src/` → 0 hits); the engine now *delegates* — though it still owns the precision-match frame (§11 D1). **row 5:** clock doctrine landed with ticket 02 — `Local::now` has no production call (`rg "Local::now" src/` → doc mention only at `engine/date.rs:91`); counts in the row are pre-02. Rows 1, 4, 6 remain accurate; row 1's serde note (N12) is *also* stale — manual `Deserialize` impls now exist for both value types (`src/date.rs:337`, `:778`), pending confirmation against N12's full AC in ticket 08's docs pass.

---

## 2. Findings catalogue

### 2.1 Correctness

| ID   | Finding                                                                                                     | Evidence                        | Rule                    |
| ---- | ----------------------------------------------------------------------------------------------------------- | ------------------------------- | ----------------------- |
| **N1**   | `chrono::Duration::seconds` panics on out-of-range magnitude → `try_seconds`                                    | engine `date.rs:432`              | `err-result-over-panic`   |
| **N3′**  | `parse("-0m")` → `-0.0`, `total_cmp` orders strictly below `0.0` so zero can fail `>= 0`; plus `0.1+0.2` equality class | `duration.rs:111–121, 717–747`    | `num-float-compare`       |
| **N4**   | No `.with_source(DateError)` chaining; `ParsedDate::parse` discards via `.ok()`                                   | engine `date.rs:183–201`          | `err-source-chain`        |
| **N7**   | Missing `can_start` guard in coercion cascade → non-durations hit allocating path/misclassify                 | `note/field.rs:525`               | `api-parse-dont-validate` |
| **N14**  | `from_seconds(1e300)` → `"0s"`; same class at `0.0004` → `"0s"` (ms-round drops value) — Display lies about `seconds` | `duration.rs:224–269`             | `type-display-vs-debug`   |
| **N15b** | `"-1h 30m"` whole-duration sign rule documented but unpinned by tests                                         | `duration.rs:45–47`               | —                       |
| **N19**  | `DurationSeconds: Display` = raw `f64` — `1e300` renders 301 digits                                               | `duration.rs:737–741`             | `type-numeric-fmt`        |
| **D9**   | Clock mix, no doctrine                                                                                      | engine `date.rs:92–117, 699, 709` | —                       |
| **D3**   | Fractional truncation in `format_precise`                                                                     | engine `date.rs:282`              | —                       |

### 2.2 Interface coherence

| ID  | Finding                                                                                                                                                                           | Remedy                                                                           |
| --- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- |
| **N2**  | Dual unit semantics: fixed 30d/365d in `seconds()`/`TimeDelta` vs calendar in engine `match` — undeclared                                                                               | A2′ + S1 + S2                                                                    |
| **N8**  | `parse`/`parse_prefix` hand-copied loops; `"1h 1x"` divergence                                                                                                                          | S5                                                                               |
| **N12** | Derived `Deserialize` bypasses `parse_iso`: chrono `NaiveDate::from_str` skips 4-digit guard + rejects `YYYY-MM`; datetime RFC3339-only; serial `…Z` ≠ Display; `NoteFieldValue` carries both | S6 (manual impl, `duration.rs:561` precedent)                                      |
| **N15** | Guard-protocol leak (`can_start`/`is_iso_shape`/`has_four_digit_year` are protocol functions); `field.rs` forgot the protocol precisely because the interface demands hidden knowledge    | S5 — classify, guards go private                                                 |
| **N16** | `YYYY-MM` → day 1, shape forgotten; precision belongs to the recognition result                                                                                                     | S7 (hoist `Precision`)                                                             |
| **N17** | `DateValue` public, `DateTimeValue`/`DateError` `pub(crate)`; external `FromStr::Err` unnameable; no `#[non_exhaustive]` on either error                                                      | S6                                                                               |
| **N18** | `from_seconds` carries a private 4th copy of unit ratios (ms-table, `duration.rs:236–243`) omitting Month/Year undeclared                                                             | S2 (derive from registry; document omission)                                     |
| **N20** | Cascade loop copied between `DateValue::parse_iso` and `DateTimeValue::parse_iso`                                                                                                     | S5-era: `parse_any` on format enums                                                |
| **N21** | Invalid-pattern detection exists 3× (`date.rs:255`, `date.rs:463`, engine `date.rs:255`) — **premise retired 2026-10-05: all three cited copies are gone; one renderer remains** (engine `date.rs:304`, `DelayedFormat` refs now engine-only) | S4 re-scoped: confirm one site + pin, no `render_pattern` needed (ticket 06) |
| **N23** | `From<FieldValueRef>` embeds classification policy (null→link→duration→string) inside a `From` impl                                                                                   | S5: duration step → `classify`                                                     |
| **D11** | Doc ≠ behavior: `date_add` docs promise six units; `DurationUnit::parse` accepts `weeks`/`ms`/abbrevs                                                                                     | **decided: widen doc, bundle with S7** (the real trap is precision no-ops, S7 fixes) |

### 2.3 Structural / depth debt

| ID  | Finding                                                                                                                                                                                                                                      |
| --- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **D1**  | Format cascade duplicated ×4 (date/datetime × parse/engine)                                                                                                                                                                                  |
| **D6**  | ISO-prefix byte scan duplicated ×4                                                                                                                                                                                                           |
| **D7**  | `NaiveDate`/`DateTime<Utc>` leak via `into_inner`/`From` — **demoted**: `NaiveDate` is already validated, so this is surface-area, not a parse-don't-validate hole. Keep the `From` impls; shrink the *need* (S1/S7); re-run deletion test on `into_inner` after |
| **D10** | Engine-private `DatePrecision` + `ParsedDate` + `format_precise` = a recognition-result type an adapter had to build because the public seam lost precision — direct evidence for N16                                                              |
| **H3**  | Recognition is not one seam: `TextShape` + three guard fragments of one classifier                                                                                                                                                             |
| **H4**  | Engine is a fat adapter: owns calendar arithmetic, formatting, clock                                                                                                                                                                         |
| **—**   | ~~Dead bridge: `checked_add`/`checked_sub` unusable until A2′ makes them calendar-aware (or superseded by S1's `apply`)~~ **executed by ticket 03: superseded by `apply` and deleted** (2026-10-05)                                                                                                                              |
| **—**   | ~~8 methods behind `#[cfg_attr(not(test), expect(dead_code))]`~~ **stale count and composition (2026-10-05): the live census is six sites — `DateValue::{shift, apply}`, `DateTimeValue::apply`, `DurationValue::{from_seconds, parts, is_calendar}`** (`src/date.rs:165/192/585`, `src/duration.rs:353/434/454`) — Decision-C's named set (`to_time_string`/`start_of_day`/`cmp_date`/`to_offset_string`/`checked_add`-residue) was deleted by ticket 03; ticket 09's item 11 carries the corrected set. Wire-or-delete per method remains the rule (S4's `format_with` half of the original claim is void — see N21 above) |

---

## 3. Parity assessment (condensed)

### Dataview (22,124 lines, L-cited)
**At parity:** ISO recognition incl. `YYYY-MM`; multi-unit durations (superset with `y`/`ms` — keep); cross-spelling value equality (stronger than theirs, E4); template-side arithmetic; `file.ctime/cday/mtime/mdate`; task date fields; midnight coercion; null-placement superset.

**Gaps:** **B17** query `date ± duration`, `date − date` (their headline idiom L13501/L4807) → `FilterFunction` entries (`filter.rs:116–125`), never operators; **B18** duration arithmetic (follows); **B14** `durationformat` (spec L6803–6825); **B12** parse-with-format (L6075); **B11** `sow/eow/soy/eoy` (L14960); **B24** components minus `.week`; **B26/B30** `file.day` (L3652).

**Divergences to register:** UTC instants (tz-bug pedigree #1412/#601 — *strengthened by the local-naive decision: parse is now TZ-dependent by design, storage stays UTC*); null never satisfies an ordering (**ADR needed — currently a comment**; *decided at spec time: keep current behavior, ADR replaces the comment*); explicit `NoteFieldType` rank; canonical duration Eq; space-datetime acceptance (note `TextShape` reclassification side effect); strftime-dialect.

**Do-not-port:** `.week`=`floor(day/7)+1`; epoch-falsy; Luxon `YYYY` week-year; null-min comparisons; `today()`/`now()` parse-then-fail; alphabetical type ordering.

### Templater (13,045 lines, L-cited)
**At parity:** day offsets; `tomorrow`/`yesterday`; arbitrary format output; `start/end_of_month`.

**Gaps:** **T1** ISO-8601 `P1M`/`P-1M` offsets (L4973) → adapter translation at `date.now`, not a second grammar; **T2** `reference`/`reference_format` (L4968) → converges with B12, one `parse_with`; **T3** `weekday(n)` (L4992), pinned ISO Monday; **T4** moment tokens (L10978/11106) → D-b; **T5** 100% local clock (L4963) → B1.

**Do-not-port:** type-overloaded `offset`; `P-1M` internal sign; lenient reference guessing; locale weekday; `stat.ctime` lie; unvalidated format strings (we keep `InvalidPattern`).

---

## 4. Decisions (final)

### A2′ — accepted
- `mo/y` stay parseable; `DurationValue` retains `parts: Option<Box<[(f64, DurationUnit)]>>` alongside `seconds`.
- **Identity stays seconds-based** (`fixed_seconds()` ratios, renamed to carry the regime in the name); **application** to dates is calendar for Month/Year parts via one owner (S1), applied left-to-right in written order.
- The Luxon-style incoherence (`"1mo" == "30d"` but different date-shifts) is **accepted and declared**, not hidden — every reference implementation runs this dual regime; ours stops pretending otherwise. **Amendment:** prose alone won't tame it — add a pinning test that *demonstrates* the behavior (equal values, different shifts) so the contract is executable, plus the CONTEXT.md clause.
- A1′ (reject `mo/y`) breaks Dataview L4814 and Templater L4973; A3′ (shape identity) breaks pinned `"1h 30m" == "90m"`. Rejected.

### B1 + naive-input = **local → UTC on parse** — accepted (revised)
- Instants stored as `DateTime<Utc>`; **naive input (`2026-07-29 14:30`) is interpreted in the local zone and converted to UTC at parse** (parity with both plugins; avoids wrong-by-hours comparisons against `file.mtime` for non-UTC users — the prior "keep UTC" lean optimised for test convenience, not user correctness).
- Date-only values are civil dates — no zone applies; `DateValue` untouched.
- `now`/`today`/`tomorrow`/`weekday`/file-stat **formatting** from the local clock; storage always UTC.
- **DST ambiguity policy for local parse — SETTLED at spec time (was this review's single open item):** ambiguous fall-back → chrono `.earliest()`; nonexistent spring-forward → shifted forward by the gap; documented in the divergence register, pinned by a test. (Original options were `.single()` reject / `.earliest()` / `.latest()`.)
- Tests inject `TZ` to stay deterministic (the engineering cost, accepted).

### D-b — accepted, scoped
- moment→strftime translator **at the template seam only**. *(2026-10-05: "`DateValue::format_with` keeps exactly one grammar (strftime)" is now moot as written — ticket 03's remediation deleted core `format_with`; the sole strftime surface is the engine's private copy at `src/template/engine/date.rs:304`. The decision's intent — one grammar, one seam — survives and is now true by deletion.)*
- **Scope 1:** bracket literals `[Daily]`, `YYYY MM DD HH mm ss` family (pass-through), `Do`/`S` ordinals, `dddd`/`ddd`/`MMM`/`MMMM`. Everything else → `InvalidPattern`.
- **Error message is part of the design:** must name the offending token, identify the input as moment-dialect, and point at the supported list — a bare "invalid pattern" on `Do MMM` would be baffling.
- Strictness is deliberate: silently mistranslating an ambiguous token (`YYYY` week-year) would be *worse* than erroring. Full token coverage grows additively later (OCP).

### D11 — widen doc, bundle with S7
- `date_add` docs state "`unit` accepts any `DurationUnit` spelling" — never a second, smaller registry (that's N18 again).
- The real user trap isn't the vocabulary — it's **precision truncation**: sub-day units on date-only inputs format back through `%Y-%m-%d` and visibly do nothing. That's D3/N16, fixed by S7; the doc line and S7 land together.

### Also decided, carried (not yet executed — 2026-10-05 correction; this block was titled "Also fixed (carried)", which overclaimed)
- Start-of-week = ISO Monday, documented divergence.
- `DEFAULT_DATE_FORMAT` removal from `config/model.rs` (live const at **`:52`**, not the `:49` this doc originally cited; still present at HEAD `5b7dc748` with its two read sites at `:814`/`:837`) → substitute **`crate::date::DEFAULT_DATETIME_FORMAT`** (not `DEFAULT_DATE_FORMAT` — different value; `src/date.rs:48` vs `:51`). Executed by ticket 08's checklist item.

---

## 5. Type & pattern designs (Rust-specific)

### 5.1 `DurationParts` — A2′ without dual authority
```text
DurationValue { raw: Box<str>, seconds: DurationSeconds, parts: Option<Box<[(f64, DurationUnit)]>> }
```
- **Sole identity = `seconds`** (Eq/Ord/Hash unchanged). `parts` is the parsed *shape*, `None` for synthesized values (`from_seconds` can't know Month/Year — and its ms-round means exact-parts claims would be false anyway; N14 class stays honest as `None`).
- **Invariant by construction:** `parse` computes `seconds` by a single left-to-right fold over exactly the parts it stores — no state where they disagree; duration arithmetic preserves Σ by construction too (below). `api-parse-dont-validate` satisfied without runtime checks.
- **`parts` doubles as the regime witness:** `Some` containing `Day|Week|Month|Year` ⇒ calendar application; else fixed. `None` ⇒ always fixed (correct). No extra enum; the state is data, matched exhaustively.
- **Signed entries:** parse stores each entry with the whole-duration sign applied (N15b), so Σ is one left-to-right fold.
- **Exactly two filling paths:** `parse` and duration arithmetic. `Add` concatenates when both `Some`; `Sub` concatenates with each rhs entry negated (Σ survives); `Mul` scales entries; **either side `None` ⇒ `None`** — Σ forces it, and the regime then honestly reads `None` ⇒ fixed.
- Rejected: deriving `seconds` on demand (dies on `None`, O(k) Eq for nothing); always-`Some` with approximations (violates Σ invariant = representable invalid state).

### 5.2 Declared regimes
- `DurationUnit::seconds()` → **`fixed_seconds()`**, `seconds_i64()` → `fixed_seconds_i64()` — regime in the name.
- Calendar meaning gets **no** counterpart method on the unit — it exists only behind S1's `shift`, so exactly one place can define it (locality).

### 5.3 `classify` — both domains, one protocol each
```text
DateValue::classify(s)     -> Option<Result<DateValue,     DateError>>      // #[must_use]
DurationValue::classify(s) -> Option<Result<DurationValue, DurationError>>
```
- Trichotomy *is* the guard protocol: `None` = cheap shape gate (O(1), no allocation — preserves lexer hot path), `Some(Err)` = allocating detail, `Some(Ok)`.
- Replaces `can_start && parse` at `sort.rs:473`, fixes N7 at `field.rs:525`, kills N8's loop divergence, absorbs N23's protocol copy; `has_four_digit_year`/`is_iso_shape`/`can_start` go **private** (N15 closed, interface shrinks).
- `TextShape` becomes a *caller*, not a fourth fragment.
- Rejected: one combined date-or-duration mega-classifier — the date/duration/link/string *decision* is coercion-layer policy (cohesion: policy at coercion seam, recognition at value seams).

### 5.4 Precision hoist (N16/D10)
```text
pub(crate) enum Precision { YearMonth, Date, DateTime }
// parse_iso unchanged (clean value for Eq); precision-carrying entry for re-serialization
```
- Engine's `ParsedDate`/`DatePrecision`/`format_precise` delegate → shadow interface deleted; `YYYY-MM` precision survives end-to-end; D3 fixable once.
- `Precision` never enters `DateValue`'s `Eq` — recognition-layer fact, not value identity (`type-enum-states`, not two bools).

### 5.5 Serde repair (N12/N17)
- Manual `Deserialize` via `parse_iso` (`Cow<'de, str>` + `Error::custom`) — exact `duration.rs:561–569` house precedent over `#[serde(try_from)]` (consistency wins).
- One serial spelling per channel: serialize via `Display` (round-trips through `parse_iso` by construction); RFC3339 `…Z` reserved for explicit interop. *(2026-10-05: the interop channel named here as `to_offset_string` no longer exists — that method was deleted by ticket 03; the `…Z` producer is `Serialize for DateTimeValue` at `src/date.rs:771` (`to_rfc3339_opts(SecondsFormat::Secs, use_z=true)`), which matches spec's post-v4 serde note. `rg "to_rfc3339\(" src/` → 0 hits.)*
- `#[non_exhaustive]` on `DateError` + `DurationError` — within-crate exhaustive matches unaffected.

### 5.6 Smaller mechanisms
- **N18:** delete ms-table; derive greedy units from one registry `const`; document Month/Year omission (un-synthesizable from magnitude).
- **N21:** ~~private `render_pattern(display, pattern) -> Result<String, ()>` in `date.rs`; three call sites map its error to their own type~~ **moot (2026-10-05): the three copies were deleted rather than shared; one engine site remains (`engine/date.rs:304`), see ticket 06's rewritten item.**
- **N20/D1:** `DateFormat::parse_any` / `DateTimeFormat::parse_any` own the cascade loop → adding a shape = variant + `ALL` + `pattern()`, one file.
- **B18:** `Add`/`Sub` for `DurationValue` (seconds add; `Sub` = add of negated rhs; parts concat when both `Some`, rhs entries negated on `Sub`; either side `None` ⇒ `None` — Σ forces it; `-0.0` normalized; result `raw` re-synthesized via `from_seconds`); `Mul<f64>` scales both — non-finite scalar/overflow leaves `seconds` non-finite, declared in the type docs and rejected by consuming conversions (`NonFiniteSeconds`), never panics; Eq remains seconds-only.
- **N19:** choose a rendering dialect for `DurationSeconds` Display (short/general, exponent above threshold) — user-facing once `durationformat` surfaces it.

### 5.7 Considered and rejected (`anti-over-abstraction`)
`Clock`/`TimeZone` traits (one adapter = hypothetical seam — doctrine is data + docs); `Dialect` trait (D-b is a pure function at the template seam); typestate parse builders (interface growth, no state to protect); merged `DateError`+`DurationError` (distinct failure domains); `Cow<'a, str>` for `raw` (lifetime param on an index-stored value = interface bloat); locale first-day-of-week (pinned ISO Monday).

---

## 6. Principles audit (summary verdicts)

| Principle                | Verdict                                                                                         |
| ------------------------ | ------------------------------------------------------------------------------------------------ |
| Separation of concerns   | ❌ calendar arithmetic ownerless (S1); precision split across seam (D10/S7); Display mixing into raw `f64` rendering (N19) |
| Single responsibility    | ❌ `From<FieldValueRef>` classifies + converts (N23/S5); `DurationUnit`'s third job (calendar vocabulary) becomes legitimate only under A2′ + S1 |
| Cohesion & coupling      | ❌ engine reaches *through* registry and newtypes into chrono; target graph `note/query/template → date.rs\|duration.rs → chrono`; `date/` directory split only if S1+S7+S11 push size — don't preempt |
| DRY                      | ❌ ×4 cascades (D1/N20), ×4 scans (D6), ×4 ratio tables (N18), ×3 format-with (N21), ×2 grammars (N8), ×4 guard sites (N15) — all have single-point remedies |
| Modularity               | ❌ guard protocol changes rippled to callers (N15) — modularity's "change inside, not outside" failed; S5 restores it |
| Information hiding       | ❌ protocol functions `pub(crate)` → go private under S5; `into_inner` demoted to monitored surface (D7); hidden facts promoted to normative CONTEXT.md (§7) |
| Open/Closed              | ✅ `FilterFunction`, filter tables, format enums, `UNIT_MAP` — **parity features are almost entirely registry entries**; only A2′/B1/D-b + S1/S2/S5/S7 touch core. ❌ engine unit-match (per-unit arms + shortcuts) |
| General flexibility      | S1's `shift`/`diff`/`apply` serve N callers from one implementation; rigidity of `checked_add`'s fixed regime resolved by A2′; flexibility as *data* (pinned Monday, declared regimes), never single-adapter traits |
| Law of Demeter           | ❌ `can_start && parse` (protocol fiddling), chrono round-trips in `ParsedDate` — one call each after S5/S1 |
| Least astonishment       | N3′ `-0.0`, N14/D11 no-op shifts, N12 Display≠serde, strictness-by-seam invisible → all documented/pinned; **the local-naive flip was itself a least-astonishment correction** |

---

## 7. CONTEXT.md revisions

- **(a)** Funnel claim made normative **and true**: "every date-shaped string — including deserialization — funnels through `parse_iso`" (true only after S6).
- **(b)** Owner of anchored arithmetic named: "`date.rs` owns calendar shift/diff; template/query layers are thin adapters."
- **(c)** Duration regime clause per A2′: identity = magnitude (ratios declared); application = calendar for month/year parts, left-to-right; equal values may shift dates differently (matches Luxon) — backed by a pinning test.
- **(d)** Date interface facts: four-digit-year rule; `YYYY-MM` → day 1; precision at the recognition layer (S7); **naive datetime = local zone → stored UTC** (S10).
- **(e)** Editorial: entries use their own *Avoid* terms inside definitions — stop, or define *Avoid* once as "never as a concept name."
- **(f)** "Single owner of unit registry" true without caveat after `fixed_seconds()` + S1.
- **(g)** Strictness-by-seam rule (lenient coercion / strict template / strict serde) + pinned ISO Monday + DST ambiguity policy.

---

## 8. Rust-skills conformance

| Rule                                    | Status                                                    |
| --------------------------------------- | --------------------------------------------------------- |
| `api-parse-dont-validate`                 | ❌ N12, N15/N7 → S6/S5; ✅ §5.1 invariant-by-construction |
| `serde-try-from-validate`                 | ✅ DurationValue precedent; ❌ date.rs → S6               |
| `err-custom-type`/`err-source-chain`        | ✅ enums; ❌ N4                                           |
| `num-float-compare`/`num-overflow-explicit` | ✅ `total_cmp`/`try_from`; ⚠ N3′; ❌ N1                       |
| `type-enum-states`                        | ✅ proposed (`Precision`, parts-as-witness); ❌ today (D10) |
| `type-display-vs-debug`/`type-numeric-fmt`  | ❌ N14, N19                                               |
| `api-non-exhaustive`                      | ❌ N17                                                    |
| `conv-tryfrom-fallible`                   | ✅ `TryFrom<DurationValue> for TimeDelta`                   |
| `anti-over-abstraction`                   | ✅ §5.7 rejection list                                    |
| `proj-pub-crate-internal`                 | ⚠ N17                                                     |
| `num-float-compare` (S1/S2) | ➕ bit-exact Σ fold + `Mul` non-finite, pinned by ticket 03 |

---

## 9. Deepening plan (final, ranked)

| #   | Suggestion                                                                                                                                                                                                            | Seam / interface                     | Collapses                                       | Parity        |
| --- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------ | ----------------------------------------------- | ------------- |
| **S1**  | Calendar-application owner in `date.rs`: `shift(base, n, unit)`, `diff(a, b, unit)`, `apply(base, &DurationValue)` (parts, L→R) — **LANDED ticket 03** (2026-10-05; engine frame-dispatch residue = §11 D1)                                                                                       | interface on `DateValue`/`DateTimeValue` | engine copy, dead bridge, H4, future query impl | C10, L13506   |
| **S2**  | `parts: Option<…>` + `fixed_seconds()` rename + N18 registry merge — **LANDED ticket 03** (2026-10-05; `DurationSeconds::normalized`/`canonical_raw` at `duration.rs:361-415`, `SUB_YEAR_DECOMPOSITION_UNITS` at `:124-136` — confirm N18's ratio-copy is fully gone at implementation time rather than re-citing "×4")  | additive; single constructor         | N2 declared as data, N18                        | A2′           |
| **S3**  | Query temporal functions `date_add`/`date_diff`/`date_component` + duration arithmetic — **gated by §11 X1** (grammar lock vs stories 1–3, ticket 05)                                                                                                                                      | `FilterFunction` entries only          | —                                               | **B17, B18, B24** |
| **S4**  | `durationformat` + ~~wire `format_with`~~ **(wire half void — core `format_with` deleted; one engine site remains)** + ~~shared `render_pattern`~~ **(moot — see §2.2 N21) re-scoped to confirm-and-pin**                                                                                             | template filters                     | ~~N21,~~ dead-code attrs                            | B14           |
| **S5**  | `classify` for both domains; guards private; `parse_any` on format enums                                                                                                                                                  | one `#[must_use]` entry per type       | N8, N7-class, N15, N23, N20, D1                 | —             |
| **S6**  | Serde repair: manual `Deserialize`, Display serial spelling, `#[non_exhaustive]` — **LANDED ticket 01** (2026-10-05: manual impls `src/date.rs:337`/`:778`, `#[non_exhaustive]` `src/date.rs:1287` + `src/duration.rs:1172`; confirm N12's full funnel claim in 08's docs pass)                                                                          | `duration.rs:561` precedent            | N12, N17                                        | —             |
| **S7**  | Hoist `Precision`/`Recognized*`; engine delegates; D11 doc widened **lands here**                                                                                                                                             | recognition result owns precision    | D10, N16, D3, D11                               | —             |
| **S8**  | D-b translator (scope 1: brackets + common tokens + good errors) + `durationformat`                                                                                                                                     | pure functions, template seam        | T4                                              | T4            |
| **S9**  | Shorthands `sow/eow/soy/eoy`, `weekday`, ISO-8601 offset translation — **seam column corrected 2026-10-05 (§11 X-F): these helpers exist on *two* seams, engine shorthands *and* query `FilterFunction` registry entries (spec story 4 requires both — ticket 07 items 11–12), so "engine adapters over S1" undercounts the interface; the query half is additionally gated by §11 X1** | engine adapters over S1 **+ query registry entries**                     | —                                               | B11, T1, T3   |
| **S10** | Correctness pass: N1, N4, N3′, N14, N19, D9 + **local-naive parse + DST policy**, N15b pinning, A2′ incoherence pinning test — **LANDED across tickets 01 (`c990f6c9`: N1/N3′/N14/N19, error sources, serde) and 02 (`90696f25`: D9, local-naive, DST policy), pinning from 03** (2026-10-05)                                                                                              | —                                    | —                                               | T5            |
| **S11** | `parse_with(text, fmt)` with `Result` contract                                                                                                                                                                            | serves `reference` + future LSP        | —                                               | B12, T2       |
| **S12** | `file.day` at index time                                                                                                                                                                                                | index computes, query exposes        | —                                               | B26, B30      |
| **S13** | Docs: CONTEXT.md (§7), ADR null-ordering, divergence register (space-datetime, magnitude-vs-calendar, strftime-dialect, **local-naive + DST**, strictness-by-seam), config-const removal (`DEFAULT_DATETIME_FORMAT` caveat) | —                                    | —                                               | C-audit       |

---

## 10. Execution order
*(2026-10-05 status overlay: steps 1–3 partially executed by tickets 01/02/03; tickets 04–09 encode the remaining edges — see spec L141+ for the authoritative encoded chain.)*

1. ~~Decisions~~ — **done**: A2′, B1+local-naive, D-b(scoped), D11-widen, ISO Monday.
2. **Correctness:** S10 (N1, N4, N3′, N14, N19, D9, local-naive + DST policy, pinning tests) + S6 (N12/N17) — **done via tickets 01 + 02** (residual: N12's full funnel confirmation rides 08's docs pass; §1 row 1 note).
3. **Load-bearing seam:** S2 → S1 → S5 → S7 (parts → calendar owner → classifier → precision) — **S2 + S1 landed in ticket 03; S5 + S7 are ticket 04** (this sequence unblocks everything structural).
4. **Parity features:** S3 (biggest gap) → S4, S8, S9, S11, S12 — **all pending (05–07)**; S3 carries the §11 X1 grammar decision, S4's `format_with`/`render_pattern` halves are void per §9.
5. **Docs:** S13 — **pending (08)**; carries §11 X4 (register entry 17) and U1 (entry 18).
6. **Dead surface:** Decision-C deletions/wirings — **the named set is already gone** (ticket 03 deleted `to_time_string`, `start_of_day`, `cmp_date`, `to_offset_string`, `checked_add`/`checked_sub`; `from_seconds` kept). The live work is the six-site `expect(dead_code)` census: `DateValue::{shift, apply}`, `DateTimeValue::apply`, `DurationValue::{from_seconds, parts, is_calendar}` — wire-or-delete each after 04–08 (ticket 09 item 11 + §11 X7's apply question).

## 11. 2026-10-05 adversarial pass — post-v4 findings & dispositions

Three independent rust-design reviews (date, duration, corpus) were run against HEAD `5b7dc748` after tickets 01–03 landed. *(2026-10-05 integration: this pass began as four standalone draft files; the consolidated report now lives inline as §11.1–§11.10 below, and the three component reports are appended in full as §12.1 (date), §12.2 (duration), §12.3 (corpus).)*

**ID warning:** these findings are namespaced in the consolidated report as **D#** (date), **U#** (duration), **X#** (corpus/process) to avoid collisions — but §2 above *also* uses `D1`/`D3`/`D6`/`D7`/`D9`/`D10`/`D11` for its own findings. **A cite like "D1" is ambiguous without a location: cite `§11 D1` (the consolidated report below; its findings carry the tag **FR** in the table) vs `this doc §2.3 D1`.** Component reports (§12) keep their original IDs — `C#` (date), `F#` (duration), `G#`/`Q1x`/`Q3-C#` (corpus), `R#` (correction register) — and §11 cites them by those names. Disposition column shows where each was written into the tickets/spec (no code was changed in this pass).

| ID | Finding (one line) | Disposition |
| --- | --- | --- |
| **D1** (FR) | Shift-frame dispatch lives in the engine: `date_shift_unit`'s `match precision` (`engine/date.rs:469-491`, `shift_wall` call `:482`) while `shift_wall` is `pub(crate)` (`date.rs:1143`) solely for that caller; frame policy stated in 5 places | **unassigned — open ordering question** recorded in 04 + 05 (land the fold before extending the engine match for `YearMonth`) |
| **D2** (FR) | Consumed-length gap: `classify` returns no byte count, so date-side gates hardcode 10 in three places + two `parse_iso` scans; date needs its own prefix entry (duration has `parse_prefix`) | 04 amendments (items D2 ×2) |
| **D3** (FR) | `shift_calendar_months` (`date.rs:539`) / `shift_calendar_days` (`:556`) re-implement `shift_wall` (`:1143`) — ~28 lines of duplicate frame logic | **candidate, not ticketed** (folds naturally into D1's relocation) |
| **D4** (FR) | Two f64→`TimeDelta` conversion paths with different failure modes: `seconds_delta` (`date.rs:1120`) vs `TryFrom` (`duration.rs:1042`); `NonFiniteSeconds` mislabels its source; error source erased at `date.rs:213`/`:604` | **candidate, not ticketed** (belongs to an error-mapping pass) |
| **D5** (FR) | `apply`/`apply_part` fold extraction (~40-line span of two near-identical left-to-right loops) | **candidate, not ticketed** (mechanical; rides D1 or 07) |
| **D6** (FR) | `DatePoint::new(wall, instant, has_time: bool)` (`date.rs:937-947`) — stale-precision flag representable; both call sites pass literals (`engine/date.rs:218`/`:232`); contradicts 03's handoff L97-101 | 04 checklist (added) |
| **D7** (FR) | `DateTimeFormat::parse` returns `Result<DateTime<Utc>, …>` but attaches a UTC placeholder for naive input (`date.rs:905-914`), undone by the caller (`:421-427`) — documented, but the type lies | 04 checklist (added) |
| **D8** (FR) | `DateError::InvalidPattern` (`date.rs:1314`) has zero constructors (only a display arm at `engine/date.rs:747`) — a permanently unconstructible public variant | 06 checklist (added; ties to D-b) |
| **U1** (FR) | `,` has two grammar owners (duration separator `duration.rs:5`/`:183`/`:263` vs note-list delimiter `inline.rs:67,82`); precedence is an accident, unpinned, unregistered — outcomes traced but PREDICTED | 08 open decision + register entry 18 |
| **U2** (FR) | `UNIT_MAP` (35 keys) / `UNIT_HINT` (`:44`) / `SUB_YEAR_DECOMPOSITION_UNITS` (`:130`) / test mirror — sync is prose-enforced; the `unknown_unit_error` doc's "cannot drift" claim (`engine/date.rs:756-758`) has no test | 08 open question (test home: 04/08/09) |
| **U3** (FR) | `DurationSeconds(pub(crate) f64)` type-doc invariant vs `from_seconds`'s non-finite acceptance — one benign site (`duration.rs:417`, downgraded after tracing `canonical_raw`'s `== 0.0` early return) | **candidate, not ticketed** (doc-conformance pass) |
| **U4** (FR) | `apply_sign` duplicated in both parse loops | rides 04 item 15's loop merge |
| **U5** (FR) | `normalize_zero` byte-identical twice (`note/field.rs:468-474`, `query/sort.rs:445`), both from `21ae8b1d` — the commit that closed drift *duplicated* the policy; third consumer `field.rs:263` | 08 checklist (doctrine record) + open: who extracts the helper |
| **U6** (FR) | `parse` vs `parse_prefix` disagree on stored `raw` for `"1h,"` (`duration.rs:214` `"1h,"` vs `:281` `"1h"`); only acceptance is pinned (`:1307-1315`), not the spelling | 04 checklist (pins with the loop merge) |
| **X1** (FR) | Spec L75 grammar lock vs query stories 1–3: `FilterFunction` is a one-variant predicate enum (`filter.rs:110-121`), `dur(` absent from `src/query/` (0 hits), no value-expression node — items 1–3 of ticket 05 are undeliverable as written | **spec L75 amended (open) + 05 decision item + 07 cascade note** |
| **X3** (FR) | Corpus staleness root cause: 04 cites pinned in `07c631ac` without re-verifying against `bcc938b5` — drifted 92–256 lines (`has_time` `:1022→:922`, `parse_iso` `:343→:87`, `duration.rs:217→:353`) | 04 repinned at `5b7dc748` + re-verify-at-implementation rule (03 L136-139 pattern) |
| **X4** (FR) | `From<DateValue> for DateTimeValue` (`date.rs:741-749`) silently falls back to UTC midnight on zone failure; reachable from filter equality (`:681`), sort keys (`sort.rs:375/396`), ordering (`field.rs:269-272`) — violates spec D14's absoluteness | **spec D14 amended (open) + 08 decision item (register #17 vs `TryFrom`)** |
| **X7** (FR) | Spec decision 12 (multi-part `apply`) has no live consumer; 05 doesn't create one — 09's wire-or-delete can strand the decision | **05 + 09 decision items** |
| **X8** (FR) | No home named for the `YearMonth` strftime pattern (`%Y-%m`) post-precision-hoist, nor for `wall` recovery in `date_shift_unit` after `ParsedDate` deletion | **spec precision line amended (open) — ticket 04 must settle** |
| **X-A…X-G** (FR) | Process/consistency findings: core-`format_with` premise retired (spec + 06 + §5.5 fixed); Decision-C census corrected (09 item 11, §2.3, §10 step 6); blocker edges wrong (03/05 headers fixed, spec L141 note); `to_offset_string` premise retired (09 item 13); S9 seam column undercounted (§9 fixed); `is_calendar` fate unassigned (04 checklist); gate-layering note for `note/field.rs:531` (04 checklist) | **all written into spec/tickets/this doc** |

**Deliberately not re-proposed** (rejected candidates re-audited and confirmed): `DurationUnit::class()`, `DurationValue` split (reconfirmed with `api-parse-dont-validate` analysis — parse already the sole validated ctor; carried into spec's Rejected list), mega-classifier, `LocalWall` newtype, public `Shift` trait, `date/` directory split, `DateError`+`DurationError` merge, private `Frame` trait (D1 is the narrow-fold version instead), test-only `dead_code` surface (spec-mandated).

**Type:** focused review + audit (research only — no code changes; every candidate is an argument, not a patch).
**Baseline:** HEAD `5b7dc748` (`refactor(date,duration): move module consts to top`, 2026-10-04 20:16, clean worktree). chrono pinned `0.4.45`.
**Date:** 2026-10-05.
**Method authority:** `.agents/skills/rust-design/` (TOOLING/DISCOVERY/DEEPENING/MODELING/METRICS), `.claude/skills/codebase-design/SKILL.md` (Module/Interface/Seam/Depth/Leverage/Locality vocabulary), `rust-skills` consulted per-candidate (notably `api-parse-dont-validate`).

### 11.1 Evidence contract

1. Every claim carries `file:line` (verified at HEAD) plus caller evidence from `rg`/CodeGraph, or is marked **PREDICTED**.
2. Findings are tagged by probe — **expansion** (should a seam exist / where should this live?) or **compression** (can two become one?) — with a direction: relocate / collapse / narrow / construct-behind-seam.
3. Corpus status per candidate: `new` / `adjudicated` / `superseded-by-<ticket>` / `conflicts-with-<decision>` / `unowned`.
4. Negative findings (items that look like candidates and survive scrutiny) are recorded with the same rigor as positives.
5. Gauges: **measured** = span arithmetic or grep count on verified lines this session; **cited** = from 03/review.md, not re-run; **predicted** = projected effect of an unimplemented change.

**Namespacing for this consolidated report:** `D#` = date.rs findings, `U#` = duration.rs findings, `X#` = corpus-level findings. Original IDs in parentheses.

---

### 11.2 Current responsibility and seam map (step 1)

#### 11.2.1 `src/date.rs` (2388 lines: prod 1–1332, tests 1341–2388)

| Unit | Location | Responsibility | Interface / callers (verified) |
| --- | --- | --- | --- |
| `DateValue` | struct `:72`, impl `:74` | civil date value; parse, shift, apply, format | 13 callers (lib.rs, date.rs, note/field.rs, query/sort.rs +2); `shift`/`apply` `expect(dead_code)` at `:165`/`:192` — declared surface for ticket 05 |
| `DateTimeValue` | `:373`/`:375` | instant + local-wall value; parse, shift, apply, serialize | `apply` dead_code `:585`; `shift` live via engine:486 |
| `DatePoint` | `:922`, fields `:924`/`:926`/`:929` | recognition triple (wall, instant, has_time); **owns `diff`** (`:965`) | **1 caller** (engine) — CodeGraph; field reads at 7+ engine sites |
| `DateDiff` | `:1020` | `Whole(n)` / `Exact(f)` | translated to `Value` at engine:642-645 |
| Format enums | `DateFormat :792`, `DateTimeFormat :854` | cascade parse/format | 2 cascade loops (`:94`, `:407`) — 04 will own one (`parse_any`) |
| Zone helpers | `local_naive_to_utc :1051`, `resolve_gap_offset :1097` | local→UTC resolution, gap policy (D14) | free fns, internal callers only — correct shape |
| `seconds_delta` | `:1119` | whole/sub-second split for **both `apply_part`s** | callers `:259 :273 :650 :668` |
| `shift_wall` | `:1143`, `pub(crate)` | civil per-unit shift core, zone-free | callers `date.rs:176` (internal) + **engine:482 only** |
| `shift_months` `:1199`, `signed_years_since :1218`, `signed_months_since :1249` | private, single-caller chains | 03-relocated owners | correct |
| `DateError` | `:1288` (`#[non_exhaustive]` landed) | 1 recognition + arithmetic/zone variants | `InvalidPattern :1314` has **zero producers** (match at engine:747) |

**Policy state:** the calendar-vs-instant shift doctrine is stated in **five** places (engine:455-466, engine:475-479, engine:411-414, date.rs:1145-1148, date.rs:474-481).

#### 11.2.2 `src/duration.rs` (2299 lines: prod 1–1230, tests 1231–2299)

| Unit | Location | Responsibility | Interface / callers |
| --- | --- | --- | --- |
| `DurationValue` | `:117` `{raw, seconds, parts: Option<DurationParts>}` | parsed duration with calendar witness | `parse :169` (prod: note/field.rs:531, sort.rs:488, FromStr/serde); `parse_prefix :233` (prod: inline.rs:186 only) |
| `DurationUnit` registry | `:915–1003` | spelling table `UNIT_MAP` (35 keys), `fixed_seconds`, `parse` (`:946`, Option) | one ratio table (S2 landed; `fixed_seconds_i64` derived) |
| `DurationSeconds` | tuple `:1015`, `pub(crate) f64` | magnitude identity, `-0.0` normalization, total_cmp | constructed via `try_from` at all external sites; one direct site `:417` |
| `DurationError` | `:1155` | merged enum (settled) | — |
| Scanner helpers | 9 fns (`can_start :499`, `skip_separators :539`, `parse_number :570`, …) | pure `(bytes,pos)→span` transforms | stateless; **foreign to the value namespace** (LCOM4 root cause) |
| Synthesis | `from_seconds :359`, `canonical_raw :380` | seconds→text | test-only today; retained by 09:12 |

**Verified seed table:** all 16 no-self associated fns exist at cited lines (3 entry points + 1 ctor + 1 synthesis = 5 value-type ops; 11 scanner/witness helpers).

#### 11.2.3 `src/template/engine/date.rs` (sole external consumer of the date seam)

`ParsedDate::parse :210`, `precision() :238-244`, `format_precise :331`, `date_shift_unit :469-491` (matches `DatePrecision`, calls `shift_wall` at :482 / `DateTimeValue::shift` at :486), `date_diff :631` (delegates to `DatePoint::diff`), `date_error :730-751` (minijinja translation — correct seam), format_with (engine-private, `:304`, 6 callers).

#### 11.2.4 Boundary and funnel checks (observed)

| Check | Result |
| --- | --- |
| Production bypass of `DateValue::parse_iso` | CLEAN — only tests/docs outside date.rs |
| `Local::now` (D9/02 clock doctrine) | GONE — 0 hits; compares use `Utc::now()` (engine:96, :659, :669) |
| `f64→TimeDelta` algorithms | exactly **two** (date.rs `seconds_delta`, duration.rs `TryFrom` at :1043) |
| `trunc()/fract()` sites | only date.rs `:247 :253 :642 :648 :1120 :1121` + duration.rs `:1052 :1053` |
| `expect(dead_code)` census | **6** — date.rs `:165 :192 :585`, duration.rs `:353 :434 :454` (ticket 09 names only 1) |
| `UNIT_HINT` consumers | engine:764 (message) + lib.rs:101 re-export; **no test references it** |
| `normalize_zero` | two byte-identical copies: `query/sort.rs:445`, `note/field.rs:468` |
| `DurationSeconds(` direct constructions | definition `:1015` + `duration.rs:417` only |
| `shift_wall` external callers | engine:482 only |
| `FilterFunction` variants | exactly one, `Contains` (filter.rs:116); `grep "dur(\|date_add\|date_diff" src/query/` → **0 hits** |
| `format_with` in date.rs/duration.rs | **none** (deleted by 03; sole survivor engine:304) |
| Duration/date gate sites | sort.rs:487 (`can_start`+parse — sole guarded site), inline.rs:221 (`is_iso_shape`+boundary), lexer.rs:250 (`is_iso_shape`), note/field.rs:114 (`.get(..10)`), note/field.rs:531 (unguarded parse, Err→String fallback) |

---

### 11.3 Discovery probes and tool-finding dispositions (step 2)

**Expansion probes (should a seam exist / where should this live?):** run over date.rs (shift/diff frame ownership, prefix recognition, constructor ownership, error-seam shape), duration.rs (spelling-table ownership, invariant enforcement, guard placement), engine (bypass of typed-value interface), corpus (grammar surface for ticket 05, apply's consumer, YearMonth pattern home).

**Compression probes (can two become one?):** run over the `apply`/`apply_part` mirror pair, the two f64→TimeDelta algorithms, the `shift_wall`/`DateTimeValue::shift` twins, `normalize_zero` copies, UNIT_HINT/UNIT_MAP spellings, parse/parse_prefix loops (04-owned), guard sites.

**Material tool findings, dispositioned:**

| Tool finding | Location class | Disposition |
| --- | --- | --- |
| jscpd: 7 clones / 78 dup lines (date+duration) | production | investigated → mapped to D3/D4/D5/U5; none dismissed |
| jscpd (src, min-lines 10): 313 clones, **none crossing the date/duration boundary** | production | negative evidence — cross-module duplication is not the problem |
| messrust: `DurationValue` complexity 78, LCOM4=2 | production (cited, not re-run) | investigated → U4: structurally explained (11/16 fns foreign); no-split upheld; complexity lives in parser fns and moves wholesale under any split |
| messrust: `parse_number` CC 14 / NPath 360 | production (cited) | drives the R2 rejection (collapsing the delegation pair reverts 03's remediation) |
| CRAP: `DateTimeValue::shift` 17.4, `shift_wall` 15.1 (cited) | production | out of scope / inherent branchiness (03:186-190); no action |
| CodeGraph: `DatePoint`/`DateDiff`/`shift_wall` each 1 external caller | production | feeds D1/X6 (measured) |
| dead_code census = 6 sites | production | ticket 09 census incomplete → X3-docs fix |
| Orphans / dependency cycles | — | not applicable (single crate, `mod date`/`mod duration` private in lib.rs) |

---

### 11.4 Ranked findings

Ranking basis: knowledge-duplication × evidence confidence × corpus independence × leverage. `D2`/`X1` outrank raw depth because they catch in-flight tickets before they land wrong.

#### 11.4.1 date.rs findings (D-series)

##### D1 (C1+X6) — The frame dispatch for *shifting* lives in the engine; for *measuring* it lives in `DatePoint` — **top structural finding**
`new` · expansion · direction: relocate the `DatePrecision` match → `DatePoint::shift(n, unit)` (or route the date arm through `DateValue::shift`)

**Observed:** `DatePoint::diff` (date.rs:965) owns measurement; sole caller engine:640 only translates errors. Shifting has no owner: engine:469-491 matches `DatePrecision` itself, calling `shift_wall` at :482 and `DateTimeValue::shift` at :486. `shift_wall` is `pub(crate)` **solely** for engine:482. The policy is restated in 5 places (§11.2.1). Two entry shapes exist and the engine bypasses the one ticket 03 specified (review §9 S1: "interface on `DateValue`/`DateTimeValue`"; target graph `note/query/template → date.rs → chrono`; `DateValue::shift` has zero production callers while the wall-level core is reached directly).

**Behavior preservation:** the `DateTime` arm already discards the shifted instant (returns `local_wall()`, engine:488) — a wall-returning `DatePoint::shift` is observationally identical. The date-only arm touches no zone today (date.rs:1146 promises this), so returning `NaiveDateTime` (not `DatePoint`) avoids adding a resolver failure mode.

**After relocation (PREDICTED):** `date_shift_unit` shrinks 23+2 lines → ~8; engine loses the `shift_wall` import; `shift_wall` → private; the engine's identity round-trip (engine:211-216, see D6) disappears; 5 doc restatements → 1 owner; ticket 05's "no reimplementation in the query layer" AC gets a named home.

**Trade-offs:** the match is "only 10 lines" — the finding is the 5 restatements + the visibility leak + that 04 plans to *extend* this match in place (`YearMonth` arm). Coordination flag: **do D1 before ticket 04, or accept the match as 04's.**

##### D2 (C2+G3) — Date-domain consumed-length has no owner; ticket 04 can't deliver its own items 13+16 without one
`conflicts-with-04` (underspecified, not refuted) · expansion · direction: date-owned `parse_prefix(s) -> Option<(DateValue, usize)>`, mirroring duration

**Observed:** duration solved this (`DurationValue::parse_prefix :233`, prod caller inline.rs:186). Date has **four caller-side copies** of "take 10 bytes then validate": inline.rs:219-222 (hardcoded `10`; sibling `ISO_DATE_LEN` unused), lexer.rs:72/247/257/263 (the constant, in the wrong module for reuse), **note/field.rs:109-116 (prefix-lenient — no atom boundary, so `"2026-01-01junk"` parses, locked by test note/field.rs:833; absent from 04's inventory)**, date.rs:127-137 (validation half inside date.rs while consumption lives at 3 call sites).

**The 04 gap:** `spec.md:92` `classify` returns **no consumed length**, while 04 item 13 requires guards to go module-private and item 16 requires inline to keep its boundary rule (needs an `end` coordinate). These cannot all hold. **Recommended amendment:** `classify` = `parse_prefix`-backed (as duration's is); boundary/atomness stays at the parser seam (moving it would be the rejected mega-classifier in miniature).

**Related corpus item:** `note/field.rs:114` is a grammar gate + N15 guard site that 04 classifies as neither (audit G3/C10) — after 04 widens to 7-byte `YYYY-MM`, this path's `.get(..10)` still returns `None`, leaving story 15 half-applied for the task-date String path (prod caller note/parser/task.rs:212).

##### D3 (C3) — `DateTimeValue::shift`'s calendar arms are `shift_wall` re-coded
`new`, flagged `conflicts-with-03-in-wording` for adjudication · compression · direction: delegate; keep the exhaustive unit match intact

**Observed (line-by-line twins):** Year→months `checked_mul(12)` (:491-494 ↔ :1149-1151), month clamp via the same `shift_months` callee, week/day arithmetic (:496-500 ↔ :1152-1171), TimeDelta sub-day path, ms branch — all identical; the only genuine difference is the resolve envelope (wall→zone→wall). `DateValue::shift` **already delegates** (`:176`) — the pattern 03's remediation established. `shift_calendar_months`/`shift_calendar_days` are private with one caller each. `shift_wall`'s doc (:1145-1148) describes a wrapper relationship that is implemented as a copy.

**Explicitly NOT the rejected `DurationUnit::class()`:** classification stays exactly where it is (the five-site exhaustive gate untouched); only arithmetic *inside* already-grouped arms moves. No predicate introduced.

**Predicted:** ~28 lines deleted (span-measured `:538-565`); net resolver calls unchanged; `DateTimeValue::shift` becomes the 3-step wrapper its doc claims.

##### D4 (C4) — Two independent `f64 → TimeDelta` algorithms, two error vocabularies, one erased source chain
`new`, `conflicts-with-03` (intent, not letter — 03's wording is scoped to `apply_part`) · compression · direction: one conversion owner; map errors keeping the source

| | `date.rs::seconds_delta :1119-1129` | `duration.rs::TryFrom<DurationSeconds> :1043-1058` |
| --- | --- | --- |
| Split | `trunc().to_i64()` / `(fract()*1e9).round()` | `trunc`/`fract` with negative-fraction re-base |
| Error | `DateError::OutOfRange` | `DurationError::NonFiniteSeconds` |
| Callers | `:259 :273 :650 :668` (both `apply_part`s) | `:212` (`DateValue::apply`), `:603` (`DateTimeValue::apply`), duration.rs:771 |

**Observed problems:** (1) 03's "single owner" claim is narrowly-true/design-false; (2) **`NonFiniteSeconds` lies** for finite out-of-range input (doc at :1048 admits the conflation); (3) both date call sites erase diagnosis via `.map_err(|_| DateError::OutOfRange)` (`:213`, `:604`) against the house source-chain rule. **Equivalence checked** (spot-evaluated `total = -0.5` and `-1e-10` through both) — currently behavior-compatible; a maintenance trap, not a live bug.

##### D5 (C5) — `apply`/`apply_part` mirror pair: extract the decomposition+fold — *narrow helper, explicitly not a frame trait*
`new`, held inside the public-`Shift`-trait rejection boundary · compression

**Observed:** mirrored structure across `DateValue` (:197-280) and `DateTimeValue` (:590-676) — non-finite guard, whole→TimeDelta, trunc/fract decomposition, L→R fold, `OutOfRange` policy; only the frame differs (civil midnight + `.date()` vs local wall + resolver). Plus the midnight chain duplicated 4× inside `DateValue` (`:175 :211 :256-258 :272-274`) + a 5th at engine:226-229 (→ D9-a).

**Constraint:** extraction must not smuggle a shared `unit` predicate behind it — if it does, it *is* the rejected `is_calendar()`. If the fold helper can't be extracted while both exhaustive matches stay legible, the honest disposition is **adjudicated repetition** (dismiss). The earlier "private `Frame` trait" formulation is **rejected**: one behavior-varying pair per trait, and the corpus rejected the public `Shift` trait (review §5.7) — two adapters here are codebase-design's *real-seam* test, so the two impls stay.

##### D6 (C6+G5) — `DatePoint`'s invariant is built by its only caller, field-by-field
`new`, partially `superseded-by-04` (precision signal), residue unowned · expansion · direction: own construction; fields go read-only (package deal: constructor **and** encapsulation, or neither)

**Observed:** `DatePoint::new(wall, instant, has_time)` asserts nothing; engine supplies literals `true`/`false` (engine:218/232). A `DatePoint { wall: 10:30, has_time: false }` is constructible — 03's remediation claim "a stale precision flag is unrepresentable" (03 L97-101) **overclaims**; the invariant holds by convention. `precision()` re-derives `DatePrecision` from `has_time` (two names for one bit); the datetime branch recomputes its wall through an identity round-trip (`into_inner` + re-wrap, engine:211-216 ≡ `value.local_wall()`).

**04 residue:** unless `new` takes `Precision` (or a `from_recognized` constructor) *inside 04*, the overclaim survives 04. Frame explicitly as constructor-ownership, **not** as the rejected `LocalWall` newtype.

##### D7 (C7) — `DateTimeFormat::parse` returns a value typed as an instant it didn't parse
`new` · expansion · direction: discriminated parse output (wall-or-instant), or branch on `zone()` before parsing

**Observed:** naive patterns force the chrono `NaiveDateTime` through `and_utc()` into `DateTime<Utc>` (`:905-916`); `DateTimeValue::parse_iso` must undo the label with `if !matches!(matched, Rfc3339) { …naive_utc()… }` (`:421-427`) — correctness rests on an **identity law**, not a conversion. Blast radius: 1 producer, 1 undo, private representation. Coordination: 04's `parse_any` inherits the question — settle inside 04.

##### D8 (C8) — Recognition errors ride arithmetic seams; `diff` returns an unreachable error
`new` · expansion/narrow · direction: choose deliberately between (a) within-domain split `DateParseError`/`DateError` or (b) narrowing seams (`diff`'s only error is provably unreachable)

**Observed:** `InvalidPattern :1314` zero producers (match engine:747); `date_error` mostly translates *impossible* into *misleading* ("date arithmetic overflowed" for parse outcomes, doc admits they cannot occur); `diff`'s `OutOfRange` documented defensive-only yet callers pay `map_err`. **Not** the rejected Date+Duration merge — a within-domain split. Ranks below D1-D5: judgment call, and (a) costs two public error types exactly where 03 deliberately simplified. `InvalidPattern` liveness = `unowned` — check ticket 06 before removal.

##### D9 — Minor cluster (each independently small)

| ID | Finding | Evidence | Direction | Corpus |
| --- | --- | --- | --- | --- |
| a | Midnight construction ×4 in `DateValue` (+1 engine) | `:175 :211 :256-258 :272-274`; engine `:226-229` | private `DateValue::midnight_wall()` (associated fn — no `self` at 3 of 4 sites) | `new` |
| b | `TextShape` defined in *sort*, consumed by *grammar* | sort.rs:457-466; filter.rs:26 import | relocate to a query-level location (04 makes it a caller but doesn't settle where it lives) | `new` |
| c | `is_iso_shape` re-implements the 4-digit check inline | `:130` vs `has_four_digit_year :115-118` | delegate | `superseded-by-04` |
| d | `to_datetime_string` one-caller Display forwarder | def `:436`; prod caller engine/query.rs:606 | fold to `to_string()` | `new` (absent from 09's list) |
| e | `parse_year_month` double-parses to fabricate a chrono error source | `:824-848` | construct source once, or dismiss (house err-source-chain rule) | `new` |
| f | `DateError::InvalidPattern` dead variant | `:1314`; zero producers | remove or promote (see D8) | `unowned` |

Dismissed below threshold: `checked_neg` ×4 in `date_sub*` (one-line guards); `signed_*_since` ordering micro-pattern (2 sites, no divergence).

#### 11.4.2 duration.rs findings (U-series)

##### U1 (F1) — `','` has two grammar owners; precedence is an accident, unpinned, unregistered
`genuinely new` · duplicated policy across seams · OBSERVED collision, PREDICTED outcomes

**Evidence:** duration claims `,` as part separator (module doc `:5`, `skip_separators` admits `,` at 183/263); note-parser claims `,` as list delimiter (inline.rs:67,82, `parse_comma_list_from :91`); YAML assigns the same chars to list splitting *before* coercion (note/field.rs:656 test `list: [1h, 30m]` → `List([Duration, Duration])`).

**Traced outcomes (PREDICTED, hand-traced through `parse_atom_at`):** `1h, 30m` → **one** `Duration(5400s)` (lookahead eats the comma); `1h, 45` → `String("1h, 45")` (MissingUnit → whole prefix None); `1h,` → `List([Duration])`. Inline duration-first lists are effectively unusable; the YAML seam silently disagrees with the inline seam; **no divergence-register entry, ticket, or test covers this.** Direction: a note-parser precedence decision (pin duration-wins, or try list before duration when `","` follows a complete atom) + 5 asserts + register entry (08).

##### U2 (F2+G2) — Unit *spelling* vocabulary has four owners; sync is comment-enforced only
`genuinely new` (distinct from S2/N18 ratios) · policy with no owner

**Evidence:** `UNIT_MAP` 35 keys (`:53-89`); `UNIT_HINT :44-46` with sync stated **in prose only** (`:41-43` "a reminder to update both"); consumed by the user-facing message engine:764 whose doc *claims* "the message cannot drift" — **no test asserts it** (rg: no test references `UNIT_HINT`); `SUB_YEAR_DECOMPOSITION_UNITS :130` (literal `&str` suffix column); the `#[case::]` mirror in `parses_all_units_and_abbreviations` (35 vs 35, set-equal — but hand-maintained).

**Corpus precedent:** this exact failure class "hid weeks" (03 L117-119) — the fix removed the symptom, not the class. **Direction:** make it executable — derived test `UNIT_MAP`→`UNIT_HINT` coverage + iterate parse cases from `UNIT_MAP.entries()` (precedent: `duration.rs:2136-2139`).

##### U3 (F3) — `DurationSeconds` documents a type invariant its field visibility doesn't enforce
`genuinely new` · representable-but-invalid · OBSERVED

Type docs assert normalization happens "at construction" and holds "everywhere" (`:1007-1014`, `:1020-1021`), but `pub(crate) struct DurationSeconds(pub(crate) f64)` (`:1015`) lets ~50 in-file sites bypass `normalized`/`TryFrom`. **Actual violations today: one, benign** (`:417`, guarded by the `== 0.0` early-return at `:383`); zero external reach (only def + `:417` match `DurationSeconds(`). **Direction:** narrow the field to private — makes the doc true by construction, zero churn outside duration.rs. Deletion test: reverting re-opens the bypass the docs claim closed.

##### U4 (F4) — The namespace is mislabeled; the seed's `Scanner` struct is the wrong remedy
`new` (analysis) · the *type* rejected on corpus grounds; micro-move optional

11 of 16 no-self fns are foreign to the value namespace (9 stateless scanner helpers + 2 parts-witness helpers). But: the helpers are stateless (no struct needed); the only stateful code is the two loops' locals, which **04 collapses into one parameterized `Whole | Prefix` loop**; post-04 a `Scanner` wraps ~5 locals of one function with no second consumer (second-adapter test fails); and 03:171-173 already rejected the adjacent parse-machinery/value split. **What survives:** optional namespace partition (move the 9 pure helpers to module-private free fns) — explicitly labeled **navigability, not deepening**, only for free while 04 rewrites the loops, never standalone, never a struct. Related: `apply_sign :314` post-pass is removable during 04's merge (sign known at first push).

##### U5 (F5) — Signed-zero normalization has three owners; two are byte-identical copies
`genuinely new` · duplicated policy · OBSERVED

`fn normalize_zero` at `note/field.rs:468-474` and `query/sort.rs:445` — byte-identical; `git log -S` shows **both introduced by `21ae8b1d`, the commit that closed signed-zero drift** (the fix duplicated the policy it closed). Third consumer `note/field.rs:263`. `DurationSeconds::normalized :1022` is the type-owned construction invariant (different responsibility — keep). **Direction:** one crate-level helper for the two raw-f64 sites; record the doctrine once (ticket 08).

##### U6 (F6) — Trailing-separator `raw` spelling diverges by entry point
`genuinely new` · pin during 04's loop merge · OBSERVED

`parse` stores the trimmed whole input (`:214` → `"1h,"`); `parse_prefix` stores the consumed span (`:281` → `"1h"`). Same value, different `Display`, by constructor. Trailing whitespace is stripped; trailing comma is kept. Only trailing-separator test asserts seconds only. `raw` is display-only (spec:88), so nothing else constrains it — nobody decided. **Direction:** pick one (recommended: consumed-span, uniformly) at 04's merge.

##### U7 (F7) — Guard-fn visibility
`superseded-by-04` — `can_start :499` / `can_start_duration_segment :504` privacy tightening belongs in 04's residue checklist. Census note: exactly **one** guarded production site (sort.rs:487), not a four-seam protocol.

#### 11.4.3 Corpus findings (X-series)

##### X1 (G1 / Q3-C1) — The filter expression grammar cannot host ticket 05 — **single blocking unknown for the parity half**
`genuinely new` (unadjudicated contradiction) · OBSERVED

`FilterFunction` has exactly one variant, `Contains` (filter.rs:111-120); `FilterAtom` = comparison | function, **both predicates → bool; no value-producing expression node**. `parse_literal_arg` accepts only `Literal` → `dur("1 month")` is a syntax error; `grep "dur(|date_add|date_diff" src/query/` → 0 hits; no `Plus`/`Minus`/`Star` tokens in the grammar; no arithmetic ops in `CompareOp`.

**The contradiction:** ticket 05 L3 + spec L75 + review §3 B17 all say "registry entries, never new operators or grammar" — and ticket 05's demo contract requires `date − date` to yield a **value** compared against a date, plus nested `dur(…)`. A predicate-only enum cannot deliver that. **Either** the grammar constraint is amended (minimal expression surface named) **or** stories 1-3 and 05 items 1-3 are undeliverable as written. Decision required *before* 05 is scheduled.

*Scoping note:* as a **code-design candidate for duration/duration-parse**, "split parse to expose intermediates" was considered and **rejected** — there is no value-producing consumer in scope (speculative generality). X1 is carried as a **corpus contradiction**, which it is.

##### X2 — (folded into U2) four spelling owners, no executable sync.

##### X3 (Q1a/Q1c) — Line-cite drift is systemic; ticket 09's census is incomplete
`docs-fix` · OBSERVED (git-verified)

**Root cause:** ticket 04's cites were pinned in review pass `07c631ac` (Oct 4 16:03) against `bcc938b5` numbers **without re-verifying** — proven by `git show 07c631ac:src/date.rs`: symbols were already 3–10 lines off *at pin time*; now off by 92–256 (e.g. `has_time` cited :1022 → actual :922/929; `parse_iso` :343 → :87/:94; `date.rs:585` cite now collides with a `dead_code` attr). Ticket 03 followed the correct pattern (L136-139: "stale; these symbols are the current pointers").

**Ticket 09 census:** 5 of 6 named methods already deleted by 03; `duration.rs:217` → now `:353`; `to_rfc3339()` premise false (0 hits — the `…Z` channel's producer is `Serialize` at date.rs:771); the live `expect(dead_code)` set is **6 different items** (§11.2.4) of which 09 names one. Item 13's re-target mandate is already satisfied by a different route.

##### X4 (G4 / Q3-C7) — `From<DateValue> for DateTimeValue` silently shifts on tz failure; absent from the divergence register
`genuinely new` · spec contradiction · OBSERVED

`date.rs:741-749`: `unwrap_or_else(|_zone_failure| midnight.and_utc())` — documented in rustdoc, but **reachable from filter equality** (`is_equal_to_literal` → `is_equal_to_date :681` → `From`), so in a broken-tz environment `Date == DateTime` compares at **UTC** midnight while every other path returns `DateError::LocalZoneLookup`. Spec L98/D14 is absolute: "never a silent shift." Ticket 08's register (17 entries) does not include it. **Options (maintainer call):** register as entry #18, or replace with `TryFrom` so the equality path degrades explicitly. The rustdoc's "chrono does the same" is a parity argument, not a spec argument.

##### X5 (G5) — folded into D6.

##### X6 (G6) — folded into D1.

##### X7 (G7) — `apply`/`parts`' declared production consumer may never materialise
`needs a decision before 09` · OBSERVED

03 declares the consumers (04's classify, ticket 05); but 05's checklist never mentions multi-part durations or `apply` (Dataview's `date_add(date, amount, unit)` is single-unit), and `grep "\.apply(" template/ index/ query/` finds only unrelated types. If 05 lands single-unit only, `DateValue::apply`/`DateTimeValue::apply` stay dead and **09's deletion test will strike spec-mandated surface** (spec L123, 03's AC). Decide: 05 wires a multi-part path, or 09 is told they're exempt, or ACs amended.

##### X8 (G8) — Where the `YearMonth` strftime pattern lives after 04
`minor, unowned interface decision` · PREDICTED

04 deletes `DatePrecision`/`format_precise` and adds a `YearMonth` arm, but neither 04 nor spec names `"%Y-%m"` or its home (presumably the hoisted `Precision`). Also: post-deletion `date_shift_unit` must recover `wall` from the value — derivable (DateValue→midnight, DateTimeValue→`local_wall()`, YearMonth→day-1 midnight) but 04 never says so, which makes the deletion look riskier than it is.

##### X9 — Minor, filed for completeness: `DurationUnit::parse` returns `Option` vs `FromStr` `Result` (weak, may be deliberate); `as_date`'s `DateTime` arm silently truncates to date (no register entry); `is_calendar`'s consumer tension (X11 below).

#### 11.4.4 Corpus internal contradictions (Q3) — consolidated

| # | Contradiction | Resolution needed |
| --- | --- | --- |
| X1 | ticket 05 ↔ review B17 ↔ spec L75 ↔ the grammar | decide expression surface (see X1) |
| X-A | spec L102-103/L111 + review §9 S4/§10 step 6 + ticket 06 items 2/15 promise `format_with` wiring that **03 deleted** (sole survivor: engine:304) | rewrite 06 items 2/15, spec L102-103, review S4; re-scope 06 to `durationformat` + moment translator + format-binding docs |
| X-B | ticket 09's six-method set vs review's "8 methods" vs §10 step 6 vs the actual 6 live items | rewrite 09's set (add `parts`, `is_calendar`, `DateValue::shift`/`apply`, `DateTimeValue::apply`; drop 5 deleted names) |
| X-C | review.md §1/§9/§10/§4 in future tense vs resolved 01/02/03; L5 "No implementation performed" contradicted by `c990f6c9`/`90696f25`/`57ea1b09` | retense; §4's `DEFAULT_DATE_FORMAT` bullet is still open (`config/model.rs:52` live — values identical to `date::DEFAULT_DATETIME_FORMAT`) |
| X-D | spec L141 "tickets will encode blocking edges" vs 03's "Blocked by: None" vs 01's "ticket 03 blocked by 01" | fix 03's header to `01, 02` |
| X-E | 05's blocker note misstates direction ("calendar owner is 03, reached via 04") | correct to: needs both 03 and 04; chain 03→04→05 |
| X-F | review §9 S9 seam column (engine only) vs 05/07 split (both seams) | correct S9's seam scope |
| X-G | 03 internal tension: `is_calendar` warned-against-as-shared-implementation vs declared keepable surface with speculative consumer; 09 doesn't list it | decide `is_calendar`'s fate (09 deletion test vs 03 handoff) — currently point opposite ways |
| X-H | 04 pin-pass certified stale numbers (process contradiction) | see X3 |

---

### 11.5 Candidates rejected or dismissed (consolidated)

| Candidate | Probe result | Disposition |
| --- | --- | --- |
| **`DurationValue` split** (by class or parse/value) | grammar mixes (`1d 2h` pinned in tests); D13 seconds-equality (`1d == 24h`) and B18 cross-class `Add` span any boundary; `class()` predicate rejected one level down; parser complexity (78) moves wholesale. **`api-parse-dont-validate` supports the design as-is**: `parse` is already the sole validated constructor, fields private — the rule's boundary half is satisfied; its invalid-state half fails because the *data* has no invariant partition, only the *interpretation* does (the five-site exhaustive match, deliberately chosen). | **rejected, reconfirmed with rule analysis** — record rationale in review.md so it isn't re-litigated |
| `Scanner<'a>` struct (seed's flagship) | stateless helpers; 04 collapses the only stateful code; 03 rejected the adjacent split; second-adapter test fails | **rejected** (U4); optional namespace micro-move only |
| Private `Frame` trait for `apply`/`shift` | one behavior-varying pair; public `Shift` trait rejected (§5.7); two impls are the *real-seam* test | **rejected** — D5's narrow fold extraction is the legal residue |
| `DurationUnit::class()` / shared `is_calendar()` predicate | would bypass the exhaustiveness gate at 5 intentional sites | **rejected by corpus** — D3/U-findings explicitly preserve classification |
| `parsed_number`/`parsed_unit` collapse | they're the scan-vs-validate halves; collapsing reverts 03's remediation (CC 17→14, NPath 648→360) | **rejected** (adverse remediation evidence) |
| `LocalWall` newtype; `Clock`/`TimeZone` traits; `date/` directory split; merged `DateError`+`DurationError`; `Cow<'raw>`; typestate builders; mega-classifier | corpus-settled | **not re-proposed** |
| Delete test-only `dead_code` surface (6 sites) | 03:164-169 declares all six spec-mandated; consumers = 04 classify + 05 | **adjudicated — not dead code** |
| `to_date_string` as dead forwarding wrapper | live (Serialize :330 + engine/query.rs:604); 09 scopes it live | **seed correction R1 — withdrawn** |
| Group zone helpers into `mod zone`; `skip_separators`/`skip_whitespace` merge; `DurationParts` struct; registry→`unit.rs`; `TryFrom` delegation chain; `SortKey::Duration` leak; Σ-fold dispersion | depth-propagation / second-adapter / knowledge-relocation tests fail or no finding | **dismissed with reasons** (§12.2 duration §5, §12.1 date §3) |
| Split `parse` to expose intermediates for query values | no value-producing consumer exists (X1 scoping) | **rejected as code candidate**; carried as corpus contradiction instead |
| `field.rs:531` unguarded parse as correctness bug | `Err` falls back to `String` — safe-by-fallback; it's *performance knowledge with no owner* | **downgraded** — attach as a note to 06's gate-layering item, not a redesign |

---

### 11.6 Negative findings — checked and correctly shaped

1. `local_naive_to_utc`/`resolve_gap_offset` as free fns — zone policy, no natural `self`; depth-propagation test fails to justify grouping. 03 scope: unchanged.
2. `seconds_delta`, `shift_months`, `signed_*_since` — 03's declared owners: private, single-caller, one job (with the D4 caveat about the *other* conversion).
3. Five-site exhaustive `DurationUnit` classification — intentional (compiler as policy owner); `rg "class("` confirms no predicate exists.
4. Engine `date_error` + `DateDiff → Value` translation — minijinja must not leak into date.rs; this *is* 03's remediation.
5. Clock doctrine landed (no `Local::now`, 0 hits); parse funnel clean (no production `from_ymd_opt` outside date.rs).
6. `From<NaiveDate> for DateValue` — D7 adjudicated: `NaiveDate` is already valid; surface-area, not a parse-don't-validate hole.
7. `DateTimeFormat::parse`'s `matched` loop — verified correct (assign-before-attempt, break-on-success); D7 targets the return type only.
8. `filter.rs` cascade, `UNIT_MAP`/ratio single-table (S2), manual `Deserialize` (N12, landed 01), `#[non_exhaustive]` (N17, landed) — verified.
9. `is_calendar` dead-code attribute — deliberate per 03:164-169 (not a census gap — withdrawn as a finding).
10. Duration/date cross-boundary clone count = 0 (jscpd) — the seam is not leaky.

---

### 11.7 Prior review cross-check (the original 10 findings)

| Prior finding | Disposition |
| --- | --- |
| 1. `DatePoint` as sum type (`Civil(Date)`) | **flawed + superseded**: `Civil(DateValue)` can't express `YYYY-MM` (DateValue wraps NaiveDate); precision rides the recognition result (04). Retire. |
| 2. Calendar arithmetic ownerless | superseded by 03 (owner landed) — residue = D1/X6 (entry-shape bypass) |
| 3. `DurationUnit::class()` predicate | rejected by corpus — do not reopen |
| 4. `DurationValue` split | rejected — reconfirmed in §11.5 with `api-parse-dont-validate` analysis |
| 5. Combined mega-classifier | rejected (per-domain classify + TextShape as caller, 04) |
| 6. `shift` re-implements `shift_wall` | **carried as D3** (still open — 03 only deleted `shift_date_months`) |
| 7. Guard protocol ripple (N15) | covered by 04 (classify + guards private); residue = D2's length dimension + field.rs:531 note |
| 8. Two f64→TimeDelta paths | **carried as D4** — genuinely new, measured |
| 9. `apply`/`apply_part` clones | **carried as D5** in narrowed form (fold extraction; frame trait rejected) |
| 10. `LocalWall` newtype | rejected — constructor question separated into D6 |

Net: 4 already decided, 4 ticket-covered, 3 genuinely new (D3/D4/D5) + the earlier "Frame trait" formulation corrected to D5.

---

### 11.8 Gauge dispositions (measured vs cited vs predicted)

| Gauge | Raw inputs / counting basis | Status |
| --- | --- | --- |
| `date_shift_unit` size | 23 lines (engine:469-491) + 2-line prelude; post-change ~8 | span **measured**; after-state **predicted** |
| `shift_wall` visibility | 1 external call site (engine:482); → private | **measured** (rg) |
| Doc restatement count (D1) | 5 sites listed §11.2.1 → 2 owners | **measured** (read) |
| Consumption copies (D2) | 4 sites → 1 owned entry | **measured** (grep) |
| `shift_calendar_*` deletion (D3) | span `:538-565` = 28 lines | **measured (span)**; delegation delta predicted |
| Twin algorithm spans (D4) | 11 lines vs 16 lines; one becomes thin wrapper | **measured (span)**; direction pending |
| D5 duplicated span | ~40 lines across two impls | **estimated, not line-audited** (open gauge work) |
| D6 construction/read sites | 2 construction, 7+ field reads | **measured** |
| dead_code census | 6 sites (date 3, duration 3) | **measured** |
| messrust LCOM4=2 / complexity 78 | cited from 03:171 | **cited, not re-run** — structural explanation measured (11/16 fns) |
| messrust `parse_number` CC 14/NPath 360 | cited from 03 remediation | **cited** — basis for rejecting the R2 collapse |
| jscpd 7 clones/78 lines; 313 src clones, 0 cross-boundary | this session's runs | **measured** |
| `UNIT_MAP`↔`UNIT_HINT` set equality | 35 vs 35 case mirror | **measured programmatically** (audit G2) |
| F1 comma outcomes (5 rows) | hand-traced through `parse_atom_at` | **PREDICTED** (no tests executed) |
| IKL / PS / KC | — | **unmeasured**: no calibrated counting basis defined for these units in METRICS.md at this scope; not attempted |
| DD (S/R/C/D) | inventory exists (seam map §11.2) | **predicted only** — labels: D1 +1 seam ownership, D2 +1 entry, U3 +1 enforced invariant |
| ISR | N/A — `DatePoint` is concrete, no variant split proposed | not applicable |
| MA / coverage % / crap / allocation benches | — | **not measured**: no date/duration Criterion bench exists (03:190-193 names this a deferral); coverage not re-run |
| Dynamic verification (3109/3113/3117 tests) | accepted from tickets' recorded gates | **not re-executed** this session |

---

### 11.9 Coverage gaps

**date.rs review:** tests (`:1341-2388`) read diagonally, not line-by-line; engine beyond line 900 (8 `date_shift_unit` callers confirmed by CodeGraph, not individually read); `note/field.rs` outside `as_date` and `filter.rs` in full; tickets 01/02/06/07/08 bodies not read in full (**D8's `InvalidPattern` must be checked against 06 before removal**); bench impact for D3/D5 unmeasured; D5's span not line-audited.

**duration.rs review:** messrust/clippy/coverage not re-run (citations only); no benches exist; all F1 outcomes predicted; `UNIT_MAP` table span not individually enumerated; `src/date.rs` read at apply/diff/shift sites only; serde/Display impl bodies read for structure, not line-audited.

**corpus audit:** no dynamic verification (`mise run verify/lint` not executed); `research/` (3 prior-art files) not read; `schema/`, `index/**` spot-checked only; ticket 02 (255 L) read in part; cross-crate consumer surface not surveyed; bench-absence claim unverified; audit-brief commit `cdc3f566` does not exist (pin commit is `07c631ac`).

**Cross-cutting:** all post-change effects are predictions until implementation; the three reports are single-pass adversarial runs — conflicts *between* component findings (none found beyond ID collisions resolved here) were resolved by the consolidator, not re-adjudicated.

---

### 11.10 Prioritized shortlist

#### A. Docs-fix only (no code) — before any remaining ticket is picked up

1. **Re-verify and repin every line cite in tickets 04/09 and review.md §2**, recording the verifying commit (as 03 did). *(X3 — highest mechanical leverage: every agent pickup re-derives these.)*
2. **Rewrite ticket 06 items 2/15, spec L102-103, review §9 S4/§10 step 6** — `format_with` is deleted; residue is engine:304 alone; re-scope 06. *(X-A)*
3. **Rewrite ticket 09's method set** to the 6 live `expect(dead_code)` items; fix `duration.rs:217`→`353`; retire item 13's `to_rfc3339()` premise. *(X-B)*
4. **Retense review.md §1 (rows 1-3), §9 (S1/S2/S6/S10), §10 (steps 2-3, 5-6), §4**; fix `config/model.rs:52` (08 item 14). *(X-C)*
5. **Fix stale doc comments:** `filter.rs:217-218` "midnight UTC" → local midnight; `engine:462-463` date-only sub-day caveat. *(Q1f)*
6. **Fix blocker headers:** 03 → `01, 02`; 05's dependency-direction note; review S9's seam column. *(X-D/E/F)*
7. **Record the `DurationValue`-split rejection rationale** (+ F2/F6/F8-style stable IDs for D3/D4/D5) in review.md so this pass isn't repeated. *(§11.5, §11.7)*

#### B. Code-work — decisions needed, not just edits

1. **Decide the expression surface for B17/B18 before ticket 05 is scheduled** — minimal expression node vs function-only restatement. **Single blocking unknown for the parity half.** *(X1)*
2. **Do D1 (shift-frame relocation) before ticket 04**, or explicitly accept the engine match as 04's extension point; route date-only through `DateValue::shift`/`DatePoint::shift` and demote `shift_wall` to private (also add to 09's `proj-pub-crate-internal` review). *(D1+X6)*
3. **Amend ticket 04 before implementation:** `classify` = `parse_prefix`-backed (consumed length); add `note/field.rs:114` to item 16's gate inventory; pin the trailing-separator `raw` spelling (U6); decide `DateTimeFormat::parse`'s return shape (D7); make `DatePoint::new` take `Precision` (D6/G5). *(D2, D6, D7, U6, X3-G3)*
4. **Pre-decide `apply`'s production consumer** before 09's deletion test: does 05 wire multi-part durations, or are the six `dead_code` sites exempt? *(X7)*
5. **Record or narrow the `From<DateValue>` UTC fallback** — register entry #18 or `TryFrom`. *(X4)*
6. **Give unit spellings an executable owner:** `UNIT_MAP`→`UNIT_HINT` derived test + case-list generation. *(U2)*
7. **Compression candidates (can pair with 05/09):** D3 delegate calendar arms to `shift_wall`; D4 unify f64→TimeDelta on one owner with source-preserving error map (also fixes the `NonFiniteSeconds` mislabel); D5 fold extraction (dismiss if exhaustive matches lose legibility); U3 narrow `DurationSeconds` field to private; U5 one crate-level `normalize_zero`. *(D3, D4, D5, U3, U5)*
8. **Land 08's owed items:** config const removal, null-ordering ADR, divergence register incl. X4, comma-precedence entry (U1), signed-zero doctrine. *(U1, U5, X4, X-C)*
9. **Decide `is_calendar`'s fate** (09 deletion test vs 03 handoff point opposite ways). *(X-G)*
10. **Attach to 06's gate-layering:** `field.rs:531`'s fall-through-on-Err is safe but unowned (performance knowledge). *(§11.5 last row)*

#### C. Ordering rationale

D1 is the deepest structural asymmetry and cheapest dividend (a `pub(crate)` disappears, 5→1 policy statements). D2/U-04-pins outrank raw depth because they prevent in-flight tickets landing wrong — the highest-leverage intervention available to a review. X1 blocks an entire ticket. D3/D4 are pure compression with measured twins. D5 is the highest-volume duplication but the most legally fragile (dismiss on legibility, don't force).

---

*Consolidated 2026-10-05 from three component reports; all claims verified against HEAD `5b7dc748` or explicitly marked cited/predicted. Read-only: no source files were modified.*

## 12. Component reports (2026-10-05 pass — integrated verbatim)

The three component reviews behind §11, retained in full as evidence records. Section numbers and finding IDs are the originals, so §11's citations ("date §3", "duration §5", "audit G2", "Q1a", "G3") resolve here. **Omitted from each report:** gauge dispositions, coverage gaps, and top-5 shortlists — §11.8, §11.9, and §11.10 consolidate them. **Kept in full:** method/contract, seed verification, caller/gate censuses, the expanded finding arguments, rejected-candidate probes, negative findings, correction registers, and the corpus stale-claim audit.

### 12.1 Component report: `src/date.rs`

**Reviewer posture:** Rust design reviewer. Research only — no edits, no fixes, no implementation. Every candidate below is an argument, not a patch.
**HEAD:** `5b7dc748` · chrono pinned `0.4.45` · `.codegraph` present (CodeGraph v1.6.0, CLI + MCP).

---

#### 0. Method, contract, and corpus

##### 0.1 Method docs used (read first)

- `.claude/skills/codebase-design/SKILL.md` — vocabulary: Module / Interface / Seam / Depth / Leverage / Locality; the "two adapters = real seam" test; the depth-propagation test for whether a grouping earns a module.
- `.agents/skills/rust-design/DISCOVERY.md` — seed questions (deep modules, misplaced fns/methods, type boundaries, seam anatomy, variant vs type).
- `.agents/skills/rust-design/DEEPENING.md` — the two probes: **expansion** ("should a seam exist / where should this live?") and **compression** ("can two things become one?"), plus move-direction vocabulary.

##### 0.2 Evidence contract (as applied)

1. Every finding carries `file:line` + caller evidence (who calls it and how often, verified by grep/CodeGraph, not asserted).
2. Every finding names which probe it came from (**expansion** / **compression**) and a **direction** (relocate / collapse / narrow / construct-behind-seam).
3. **OBSERVED** = verified in source this session. **PREDICTED** = follows from the change; not measured unless a gauge says so.
4. Corpus status is stated per candidate: `new` / `adjudicated` / `superseded-by-*` / `conflicts-with-*` / `unowned-by-any-ticket`.
5. Negative findings (correctly-shaped items) are recorded with the same rigor as positives — a review that only finds problems is not adversarial, it is biased.

##### 0.3 Scope

| File | Lines | Read |
| --- | --- | --- |
| `src/date.rs` | 2388 (prod 1–1332, tests 1341–2388) | prod fully; tests diagonally + greps |
| `src/template/engine/date.rs` | 1913 | 1–900 fully (module doc, parse/format, shortcuts, shift/diff, `date_error`) |
| `src/field.rs` | 1825 | 780–890 (classify/guards) |
| `src/query/sort.rs` | 1139 | 430–520 (`TextShape`, guards) |
| `src/note/parser/lexer.rs` | 810 | 210–290 (date lexing) |
| `src/note/parser/inline.rs` | 494 | 130–290 (duration/date recognition) |
| `src/note/field.rs` | — | 95–130 (`as_date`) |
| `src/lib.rs` | — | 63 (`mod date`), 92–93 (re-exports) |
| `src/duration.rs` | — | conversion impls (1000–1075), `parse_prefix`, `to_seconds` |
| `src/template/engine/query.rs` | — | 604–606 (`to_date_string` / `to_datetime_string`) |

##### 0.4 Corpus consulted

- `.scratch/temporal-core/review.md`, `spec.md`
- Issues: `03-duration-semantics-owner.md` (RESOLVED), `04-recognize-with-precision.md` (READY), `05-query-temporal-functions.md` (in flight), `09-dead-surface-cleanup.md` (checklist audited below).

**Settled decisions this review must not re-propose:**

| # | Settled | Meaning here |
| --- | --- | --- |
| 1 | **Ticket 03** — `DatePoint` owns `diff`; `signed_*_since` relocated + private; `seconds_delta` is the whole/sub-second owner *for both `apply_part`s*; `DateValue::shift` delegates to `shift_wall` at midnight; `shift_date_months` deleted; `DateError::OutOfRange` is the exact/trunc boundary; engine `DateError`→render-error translation lives in `date_error` (engine:730); `local_zone` resolution (`local_naive_to_utc`, `resolve_gap_offset`) unchanged. | Fns behind this line are owners, not findings — unless the ownership is *incomplete* (see C4). |
| 2 | **Ticket 04** — `classify` trichotomy on both domains; `Precision { YearMonth, Date, DateTime }` hoisted into a recognition result `{value, precision}`; format enums own the cascade loop once (`parse_any`); guards (`can_start`, `is_iso_shape`, `has_four_digit_year`) go private; engine `ParsedDate` / `DatePrecision` / `format_precise` delegate then deleted (D10); `TextShape` becomes a caller (H3); `DatePoint::has_time` derives from `Precision`. | Candidates that 04 already covers are labelled `superseded-by-04`, not counted as new findings. |
| 3 | **Explicitly rejected (do not re-propose):** `DurationUnit::class()` / `is_calendar()` shared predicate (the exhaustive match at 5 sites is intentional: `is_calendar`, `shift_wall`, `DateTimeValue::shift`, both `apply_part`s, `DatePoint::diff`); `DurationValue` type split; combined date-or-duration mega-classifier; `LocalWall` newtype; **public `Shift` trait** (review §5.7); `date/` directory split; `Clock`/`TimeZone` traits (one adapter = hypothetical seam); merged `DateError`+`DurationError`. `DateError` `#[non_exhaustive]` already landed (N17/S6). | C5 and C3 explicitly argue they do *not* re-propose these. |
| 4 | **Ticket 05** — `DateValue::{shift, apply}` and `DateTimeValue::apply` are declared production surface for query temporal functions; AC: "engine and query paths agree on the same calendar owner (no reimplementation in query layer)". | Their `dead_code` attributes are not findings. |
| 5 | **Ticket 09** — dead-surface cleanup; its audit lists `DateTimeValue::to_date_string` as removed and `DateValue::to_date_string` as **live at field resolution** (out of scope). | Used below to *correct* the seed's forwarding-wrapper candidate. |
| 6 | **D7** — `From<NaiveDate> for DateValue` is surface-area, not a parse-don't-validate hole (`NaiveDate` is already a valid domain value). | Negative finding. |
| 7 | **D9 / ticket 02** — clock doctrine: no `Local::now`; instant compares via `Utc::now()`, local clock for display only. | Verified landed (negative finding). |

---

#### 1. Seed verification

The seed inventory (questions carried into this review from the corpus and the previous session) was re-verified line-by-line this session. Tables below record what was **confirmed**, what was **corrected**, and what was **newly measured**.

##### 1.1 Seed inventory — line numbers (all confirmed)

| Item | Expected | Verified at | Status |
| --- | --- | --- | --- |
| `local_naive_to_utc` (free fn) | 1051 | `src/date.rs:1051` | CONFIRMED |
| `resolve_gap_offset` (free fn) | 1097 | `src/date.rs:1097` | CONFIRMED |
| `seconds_delta` (free fn) | 1119 | `src/date.rs:1119` (body 1119–1129) | CONFIRMED |
| `shift_wall` (free fn) | 1143 | `src/date.rs:1143` (body 1143–1189) | CONFIRMED |
| `shift_months` (free fn) | 1199 | `src/date.rs:1199` | CONFIRMED |
| `signed_years_since` (free fn) | 1218 | `src/date.rs:1218` | CONFIRMED |
| `signed_months_since` (free fn) | 1249 | `src/date.rs:1249` | CONFIRMED |
| `DateValue` struct / impl | 72 / 74 | `src/date.rs:72` / `:74` | CONFIRMED |
| `DateTimeValue` struct / impl | 373 / 375 | `src/date.rs:373` / `:375` | CONFIRMED |
| `DateFormat` | 792 | `src/date.rs:792` (ALL 801, pattern 806, `parse` 824, `parse_year_month` 840) | CONFIRMED |
| `DateTimeFormat` | 854 | `src/date.rs:854` (ALL 872, pattern 884, `parse` 905) | CONFIRMED |
| `DatePoint` | 922 (`pub(crate)` fields 924/926/929) | `src/date.rs:922` | CONFIRMED |
| `DateDiff` | 1020 | `src/date.rs:1020` | CONFIRMED |
| `DateError` | 1288 | `src/date.rs:1288` (variants through 1332) | CONFIRMED |
| `DateValue::shift` | 170 | `src/date.rs:170` (delegates at `:176`) | CONFIRMED |
| `DateValue::apply` | 197 | `src/date.rs:197` (`TimeDelta::try_from` at `:212`) | CONFIRMED |
| `DateValue::apply_part` | 233 | `src/date.rs:233` | CONFIRMED |
| `DateTimeValue::shift` | 485 | `src/date.rs:485` | CONFIRMED |
| `DateTimeValue::shift_calendar_months` | 538 | `src/date.rs:538` | CONFIRMED |
| `DateTimeValue::shift_calendar_days` | 554 | `src/date.rs:554` | CONFIRMED |
| `DateTimeValue::apply` | 590 | `src/date.rs:590` (`TimeDelta::try_from` at `:603`) | CONFIRMED |
| `DateTimeValue::apply_part` | 628 | `src/date.rs:628` | CONFIRMED |
| `dead_code` attributes | 165, 192, 585 | `src/date.rs:165`, `:192`, `:585` | CONFIRMED |

##### 1.2 Caller evidence (all measured this session)

| Symbol | Call sites (verified) | Reading |
| --- | --- | --- |
| `shift_wall` | `src/date.rs:176` (same module), `src/template/engine/date.rs:482` (external) | `pub(crate)` exists **solely** for the engine — C1 dividend. |
| `DateTimeValue::shift` | live production: `engine/date.rs:486` | Not dead; calendar-arms twin of `shift_wall` — C3. |
| `DateValue::shift` / `DateValue::apply` / `DateTimeValue::apply` | tests only (`date.rs` tests; `dead_code` 165/192/585) | Declared surface for ticket 05 — **not** findings. |
| `DatePoint::new` | `engine/date.rs:218` (literal `true`), `:232` (literal `false`) | Caller-built invariant — C6. |
| `DatePoint` field reads | engine:214, 246, 348, 379, 395, 482–485, 640, 659, 669 (7+ read sites) | Fields are the engine's data source. |
| `DatePoint::diff` | `engine/date.rs:640` (`from.point.diff(to.point, unit).map_err(date_error)`) | The correct seam — negative finding + contrast for C1. |
| `signed_years_since` / `signed_months_since` | only from `DatePoint::diff` | 03-relocated, private, single-caller — correct. |
| `seconds_delta` | `date.rs:259`, `:273`, `:650`, `:668` (both `apply_part`s) | Matches 03's scope — but see C4 for the *other* split. |
| `has_four_digit_year` | `date.rs:89`, `date.rs:402`, `src/field.rs:830`, `src/query/sort.rs:479` | 4 callers (CodeGraph agrees: "4 callers in src/field.rs, src/query/sort.rs, src/date.rs"). |
| `is_iso_shape` | `src/note/parser/lexer.rs:250`, `src/note/parser/inline.rs:221` | 2 callers. |
| `shift_calendar_months` / `shift_calendar_days` | `fn`-private; callers only `DateTimeValue::shift` at `:493`, `:495`, `:498`, `:500` | Single-caller private helpers — the twins C3 deletes. |
| `DurationValue::parse_prefix` | def `src/duration.rs:233`; prod caller `src/note/parser/inline.rs:186` (+ tests 1458/1473/1811) | The prefix entry the **date** domain lacks — C2. |
| `TimeDelta::try_from(...)` | `date.rs:212`, `date.rs:603`, `duration.rs:771` (+ duration tests 1995/2001/2024/2265/2283/2295) | The second f64→`TimeDelta` algorithm — C4. |
| `to_date_string` | `date.rs:330` (its own `Serialize`), `engine/query.rs:604`, test `date.rs:1510` | Live — **corrects** the seed's "forwarding wrapper" candidate. |
| `to_datetime_string` | 1 prod caller `engine/query.rs:606`; tests 1468/1482/1522 | One-caller forwarder (C9-d). |
| `DateError::InvalidPattern` | declaration `date.rs:1314`; match `engine/date.rs:747`; **zero producers** | Dead variant — C8/C9-f; corpus mentions it only as "we keep `InvalidPattern`" (review:89/110), no ticket owns it. |
| `TextShape` | def `src/query/sort.rs:457-466`; import `src/query/grammar/filter.rs:26` (`sort::TextShape`); use `:190–194` | Cross-module import from a sibling concern — C9-b. |
| `and_hms_opt(0, 0, 0)` midnight chains (prod) | `date.rs:175`, `:211`, `:257`, `:272` (+ engine `:226-229`) | Four copies inside `DateValue` — C9-a. |

##### 1.3 Funnel, bypass, and hygiene checks

| Check | Method | Result |
| --- | --- | --- |
| Production bypass of `parse_iso`? | grepped every `NaiveDate::from_ymd_opt` outside `date.rs` | CLEAN — only tests/doc examples (plus engine's `is_leap_year` probe). The parse funnel claimed in `engine/date.rs:19-21` holds. |
| `Local::now` still present? | repo grep | GONE — clock doctrine (D9/02) landed. Compares read `Utc::now()` (`engine/date.rs:95-97`, `local_now` for display only). |
| Third f64→`TimeDelta` conversion anywhere? | grepped `TimeDelta`, `chrono::Duration`, `try_seconds`, `nanoseconds(` outside `date.rs`/`duration.rs` | NONE — exactly two algorithms exist (C4). |
| Which code does `trunc()/fract()`? | `trunc()\|fract()` grep across `src/` | `date.rs:247, 253, 642, 648, 1120, 1121` and `duration.rs:1052, 1053` — nothing else. |
| `DateTimeFormat::parse` placeholder invertible? | read `date.rs:905-916` + `:421-427` | `and_utc().naive_utc()` is an identity round-trip — correct *today*, but a correctness hinge, not a conversion (C7). |
| Guards reachable from tests? | caller table above | All four guard callers are production (field classify, sort, lexer, inline) — no test-only reachability to hide behind. |
| `DateError` `#[non_exhaustive]` landed? | read `date.rs:1286+` | Landed (N17/S6) — not a candidate. |
| Engine error translation preserves sources? | read `engine/date.rs:730-751` | Yes for parse errors (source chain kept); but `date.rs:213` and `:604` do `.map_err(\|_\| DateError::OutOfRange)`, erasing `DurationError` — part of C4. |

##### 1.4 CodeGraph blast radius (read-only run)

| Symbol | Blast radius reported |
| --- | --- |
| `date_shift_unit` (`engine/date.rs:469`) | 8 callers (all shortcut filters in the same file); no tests within 3 hops |
| `date_diff` (`engine/date.rs:631`) | 1 caller in-file; tested via `tests/e2e/support.rs` |
| `DatePoint` (`date.rs:922`) | **1 caller** — `src/template/engine/date.rs` only |
| `shift_date` (`engine/date.rs:390`) | 2 callers in-file |
| `DateValue` (`date.rs:72`) | 13 callers across `lib.rs`, `date.rs`, `note/field.rs`, `query/sort.rs` +2 |
| `shift_wall` (`date.rs:1143`) | 3 references in `engine/date.rs`, `date.rs` (rg resolves to 2 call sites: engine:482, date.rs:176) |
| `has_four_digit_year` (`date.rs:115`) | 4 callers |

*Correction:* CodeGraph's reference count for `shift_wall` (3) includes the engine import line; the rg-verified **call-site** count is 2 (one external). Reporting uses rg numbers.

##### 1.5 Corrections to seed assumptions

| # | Seed assumption | Correction | Consequence |
| --- | --- | --- | --- |
| R1 | "`to_date_string` is a dead forwarding wrapper" | **Wrong.** Live at `date.rs:330` (`Serialize`) + `engine/query.rs:604`; ticket 09 explicitly adjudicates it "live at field resolution — out of scope". | Moved to negative findings. |
| R2 | "`seconds_delta` is the single owner of the whole/sub-second split" | **True only for `apply_part`.** `DateValue::apply` (`:212`) and `DateTimeValue::apply` (`:603`) use a *second* algorithm: `TryFrom<DurationSeconds> for TimeDelta` (`duration.rs:1043-1058`), with its own `trunc/fract` handling and a different error type. | Becomes C4. |
| R3 | "The date domain's guarded entry points are known (inline, lexer)" | **Undercounted: 4 sites.** `src/note/field.rs:114-115` does its own `get(..10)` + `DateValue::parse_iso(prefix)` — a fourth copy, prefix-lenient (no atom-boundary rule), not in ticket 04's inventory. | Folded into C2. |
| R4 | "Ticket 04's `classify` will cover recognition cleanly" | `spec.md:92` signature `classify(s) -> Option<Result<Value, Error>>` returns **no consumed length**, while 04 items 13/16 require the guards to become module-private *and* inline to keep its boundary rule. | C2 flagged as `conflicts-with-04` (underspecified, not refuted). |
| R5 | "`DatePoint::has_time` is 04's problem" | 04 covers the **precision signal** only. The *construction seam* (caller-built triple at engine:218/232, `pub(crate)` fields) and wall/instant pairing are unaddressed. | Split into C6 (residue) + 04-covered part. |
| R6 | "`LocalWall` newtype rejected ⇒ nothing to do about wall/instant pairing" | Rejection stands (no newtype); the *constructor* question is separate from the newtype question. | C6 framed as constructor-ownership, explicitly not as `LocalWall`. |
| R7 | "`checked_neg` repeat in `date_sub`/`date_sub_days`/… is a candidate" | Verified: the repetition is 4 one-line `checked_neg` guards around a shared `date_shift_unit` — below the complexity threshold; also pre-04 surface. | Dismissed (§4). |
| R8 | "09's checklist can be trusted as a live inventory" | **Stale:** 03's remediation already deleted `DateTimeValue::{start_of_day, to_offset_string, to_time_string, to_date_string, cmp_date, format_with}` and `DateValue::format_with`. `to_datetime_string` (one prod caller) is absent from 09's list entirely. | 09 flagged as undercount; `to_datetime_string` raised as C9-d. |
| R9 | "`shift_wall`'s doc describes a wrapper relationship" | The doc at `date.rs:1145-1148` says "callers wanting DST-aware local-wall semantics route through `DateTimeValue::shift`" — but `DateTimeValue::shift`'s calendar arms **do not call** `shift_wall`; they re-implement it. The documented relationship is implemented as a copy. | Evidence for C3. |
| R10 | "`ParsedDate::parse` needs the double wrap to compute the wall" | `let instant = value.into_inner(); let wall = DateTimeValue::from(instant).local_wall()` (`engine:211-216`) ≡ `value.local_wall()` — an identity round-trip. | Micro-evidence for C6. |

---

#### 2. Ranked findings

Ranking basis: (knowledge duplication × evidence confidence × corpus independence). C2 outranks its raw "depth" because it catches an in-flight ticket before it lands wrong.

---

##### C1 — The frame dispatch for *shifting* lives in the engine; the frame dispatch for *measuring* lives in `DatePoint`

**Kind:** misplaced fn / decomposition · **Probe:** expansion · **Direction:** relocate `date_shift_unit`'s match → `DatePoint::shift(n, unit) -> Result<NaiveDateTime, DateError>`

**OBSERVED**

- `src/date.rs:965` — `DatePoint::diff` owns measurement: one typed value in, one answer out. Sole caller `engine/date.rs:640` (`from.point.diff(to.point, unit)`) only translates the error and the `DateDiff` enum (`Whole(n)`/`Exact(f)` → `Value`, engine:642-645).
- `src/template/engine/date.rs:469-491` — shift has no such owner. The engine matches on `DatePrecision` itself:

  ```rust
  let wall = match precision {
      DatePrecision::Date => shift_wall(parsed.point.wall, n, unit).map_err(date_error)?,          // :482
      DatePrecision::DateTime => {
          let dt = DateTimeValue::from(parsed.point.instant);
          let shifted = dt.shift(n, unit).map_err(date_error)?;                                    // :486-487
          shifted.local_wall().ok_or_else(date_out_of_range_error)?                                 // :488
      }
  };
  format_precise(wall, precision)                                                                   // :490
  ```

- The calendar-vs-instant policy this match depends on is stated in **five** places:
  1. `engine/date.rs:455-466` — `date_shift_unit`'s rustdoc.
  2. `engine/date.rs:475-479` — an inline comment 10 lines below, restating the same sentence.
  3. `engine/date.rs:411-414` — `date_add`'s rustdoc (cross-links to `date_shift_unit`, lighter).
  4. `src/date.rs:1145-1148` — `shift_wall`'s rustdoc.
  5. `src/date.rs:474-481` — `DateTimeValue::shift`'s rustdoc (the same doctrine from the other side).
- `src/date.rs:1143` — `shift_wall` is `pub(crate)` **solely** for engine:482 (the only external call site; `date.rs:176` is same-module).
- CodeGraph: `DatePoint` has exactly 1 caller (the engine); `date_shift_unit` has 8 (the shortcut filters).
- Behavior preservation: the `DateTime` arm already **discards the shifted instant** — it returns `local_wall()` (engine:488) and formats the wall (engine:490). A `DatePoint::shift` returning the shifted wall is observationally identical for every current caller.

**Analysis (knowledge moved behind the seam):**

"Which frame does a shift operate in?" is split across the seam: `diff`'s frame knowledge sits behind the interface (owner: `DatePoint`), `shift`'s sits in front (owner: engine). After relocation:

- `date_shift_unit` shrinks from 23 lines + a 2-line precision prelude to "parse → `point.shift(n, unit)` → `format_precise`" (gauge: C1-G1, §6).
- The `shift_wall` import (engine:47) drops; `shift_wall` narrows `pub(crate)` → private.
- The engine stops reconstructing `DateTimeValue` from an instant it already held (the C6-R10 identity round-trip disappears with it).
- Five doc restatements collapse to one owner + cross-links.
- Ticket 05's "no reimplementation in the query layer" AC gets a named home to call instead of re-deriving.

**Trade-offs / counter-arguments:**

- Returns `NaiveDateTime`, not `DatePoint` — a `DatePoint`-returning version would re-derive an instant for date-only inputs, adding a resolver call (and a `LocalZoneLookup` failure mode) to a path that today touches no zone (`date.rs:1146` promises exactly that). The asymmetry with `diff → DateDiff` is real but each return type is what its consumer consumes.
- The match is "only 10 lines". The finding is not the line count — it's that (a) the policy has 5 restatements, (b) `pub(crate)` visibility exists because of it, and (c) 04 is about to *extend* this match in place.
- Preserves the 03 exhaustive-match gate: classification by unit still happens inside the owner (`shift_wall` / `DateTimeValue::shift`), unchanged.

**Corpus:** `new`, consistent with ticket 03's constraint ("the owner takes and returns typed values … never strings, never formatting").

> **Coordination flag (04):** ticket 04 plans to *add a `YearMonth` arm to the engine match*. C1 relocates that extension point first. Not a contradiction — an ordering decision: **do C1 before 04, or accept the match as 04's.**

---

##### C2 — Date-domain prefix recognition has no owner — and ticket 04 makes that worse

**Kind:** decomposition + corpus contradiction · **Probe:** expansion (mirror an entry the sibling domain already has) · **Direction:** date-owned `parse_prefix(s) -> Option<(DateValue, usize)>` in `date.rs`

**OBSERVED**

Duration already solved this: `DurationValue::parse_prefix(&str) -> Option<(Self, usize)>` (`src/duration.rs:233`, docs at :221 "…and the number of consumed bytes"), exactly one prod caller — `src/note/parser/inline.rs:186` (`let (dv, consumed) = DurationValue::parse_prefix(tail)?;`).

Date has **four caller-side copies** of "take 10 bytes, then validate":

| # | Site | Code | Note |
| --- | --- | --- | --- |
| 1 | `src/note/parser/inline.rs:219-222` | `let end = self.source.advance(pos, 10);` + `if !(DateValue::is_iso_shape(date) && self.is_atom_boundary(end))` | Hardcoded `10`; sibling constant `ISO_DATE_LEN` unused. |
| 2 | `src/note/parser/lexer.rs:72, 247, 257, 263` | `const ISO_DATE_LEN: usize = 10;` + `get(..ISO_DATE_LEN)` + `get(ISO_DATE_LEN..)` + `lex.bump(ws_end.saturating_add(ISO_DATE_LEN))` | The constant exists — in the wrong module for reuse. |
| 3 | `src/note/field.rs:109-116` | `s.get(..10).and_then(\|prefix\| DateValue::parse_iso(prefix).ok())` | **Prefix-lenient**: no `is_atom_boundary`, so `"2026-01-01junk"` parses; locked in by test `note/field.rs:833` ("parses the leading ten bytes"). **Not in 04's inventory** (04 lists this file only as an `Option`-discard site). |
| 4 | `src/date.rs:127-137` | `is_iso_shape`'s `len >= 10` + an inline 4-digit check at :130 that re-derives `has_four_digit_year` | The validation half of the rule, inside `date.rs`, while the *consumption* half lives at three call sites. |

Guard caller inventory (for the "guards go private" claim): `has_four_digit_year` — `date.rs:89`, `date.rs:402`, `src/field.rs:830`, `src/query/sort.rs:479`; `is_iso_shape` — `lexer.rs:250`, `inline.rs:221`.

**The gap in ticket 04:**

- `spec.md:92` — `classify(s) -> Option<Result<Value, Error>>`: **no consumed length.**
- 04 item 13 — the guards become "unreachable outside their module".
- 04 item 16 — inline must still "accept 7-byte `YYYY-MM` through classify" *and* keep its atom-boundary rule.
- 04 H3 — `TextShape` becomes a caller of `classify`.

These cannot all hold: if `classify` returns no length, callers must keep their own consumption knowledge (violating item 13), or item 16's boundary check loses its `end` coordinate. The duration domain got "one scanner, two wrappers"; the date domain got no equivalent. Copy #3 shows the ad-hoc sites already outnumber the inventory.

**Analysis:** the knowledge "what is a date token's extent?" is unowned — neither the parser (it hardcodes 10) nor `date.rs` (it only validates shape) nor `field.rs` (it re-slices and re-parses). One owned entry point deletes copies #2's constant-in-the-wrong-place, #4's inline digit check (delegates to `has_four_digit_year`), and gives #1/#3 a `consumed` value instead of a literal.

**Trade-offs:** `is_atom_boundary` stays at the inline seam — atomness is a parser concern and must *not* move into `date.rs` (that would be the rejected combined-mega-classifier in miniature). `note/field.rs`'s leniency is test-locked; changing it is a behavior decision to be made explicitly, not silently.

**Corpus:** `conflicts-with-04` — underspecified, not refuted. **Recommended amendment:** date `parse_prefix` owned by `date.rs`; `classify` = `parse_prefix`-backed (mirroring duration); guards retire as 04 intends; boundary rule stays in the parser.

---

##### C3 — `DateTimeValue::shift`'s calendar arms are a second copy of `shift_wall`

**Kind:** duplicated policy across impls · **Probe:** compression · **Direction:** calendar arms delegate to `shift_wall`; keep the exhaustive unit match intact

**OBSERVED**

Arithmetic twins (verified line-by-line):

| Operation | `DateTimeValue::shift` (`date.rs:485-527`) | `shift_wall` (`date.rs:1143-1189`) | Identical? |
| --- | --- | --- | --- |
| Year → months (`n*12`, overflow check) | :491-494 (`checked_mul(12)`) | :1149-1151 (`checked_mul(12)`) | yes |
| Month clamp (Jan 31 + 1mo → Feb 28/29) | :495 → `shift_calendar_months:538-543` → `shift_months:1199` | :1150 → `shift_months:1199` | same callee |
| Week/Day → days (`n*7`, `checked_add_days`) | :496-500 → `shift_calendar_days:554-565` | :1152-1171 | same arithmetic, re-coded |
| Hour/Minute/Second via `TimeDelta` | :501-516 | :1172-1181 | same |
| Millisecond branch | :517-525 | :1182-1188 | same |
| Resolve envelope (wall in → zone → wall out) | `local_wall()` → … → `local_naive_to_utc` | none (zone-free by contract) | the *only* genuine difference |

Also verified:

- `DateValue::shift` **already delegates** (`date.rs:170-177`, delegation at `:176`: `shift_wall(wall, n, unit).map(|shifted| Self(shifted.date()))`) — the delegation pattern ticket 03's remediation established for exactly this shape.
- `shift_calendar_months` / `shift_calendar_days` are `fn`-private with a single caller each (`DateTimeValue::shift`, :493/:495/:498/:500).
- `shift_wall`'s doc (`date.rs:1145-1148`): "No zone is consulted: callers wanting DST-aware local-wall semantics route through `DateTimeValue::shift`" — describing a relationship implemented as a copy (correction R9).

**Analysis:** `DateTimeValue::shift` ≡ *(resolve local wall → apply civil rule → re-resolve)*. Only the first and last steps are zone-specific; the middle is `shift_wall` verbatim. Delegating removes ~28 lines of twin code (`shift_calendar_*`, gauge C3-G1, §6) and makes the documented relationship true.

**Why this is NOT the rejected `DurationUnit::class()` predicate (explicit):**

- The **classification** (`match unit { Year|Month => …, Week|Day => …, … }`) stays exactly where it is — at both `shift_wall` (:1147-1188 region) and `DateTimeValue::shift` (:490-526). Those exhaustive group-matches are the five-site adjudication (03/review): `is_calendar`, `shift_wall`, `DateTimeValue::shift`, both `apply_part`s, `DatePoint::diff`.
- What moves is the arithmetic *inside* the already-grouped arms — the same delegation `DateValue::shift` performs today.
- No shared predicate, no `is_calendar()`, no `class()` is introduced or needed.

**Trade-offs:** one extra `local_naive_to_utc` boundary *inside* the call chain — but that boundary exists today (shift → `shift_calendar_*` → resolver); net resolver calls unchanged. `DateTimeValue::shift` becomes a 3-step wrapper: resolve → `shift_wall` → re-resolve, which is precisely what its doc claims it already is.

**Corpus:** `new`, but it edits a function ticket 03 lists among "intentional repetitions" → flag as `conflicts-with-03-in-wording` for adjudication. The rejected decision is the *predicate*; this is the *delegation* 03 itself established.

---

##### C4 — Two independent `f64 → TimeDelta` algorithms, with different error labels

**Kind:** duplicated policy / mislabeled error · **Probe:** compression · **Direction:** one conversion owner; date maps the error (keeping the source)

**OBSERVED**

| | `date.rs::seconds_delta` | `duration.rs::TryFrom<DurationSeconds> for TimeDelta` |
| --- | --- | --- |
| Location | `src/date.rs:1119-1129` | `src/duration.rs:1043-1058` |
| Split | `trunc().to_i64()` / `(fract()*1e9).round()` | `trunc()` / `fract()` with explicit negative-fraction re-base (`frac < 0 → (whole-1, (frac+1)*1e9)`) |
| Assemble | `try_seconds(whole)?.checked_add(&nanoseconds(sub))` | `secs.to_i64().zip(nanos.round().to_u32()).and_then(TimeDelta::new)` |
| Error | `DateError::OutOfRange` | `DurationError::NonFiniteSeconds` |
| Callers | `date.rs:259, 273, 650, 668` (both `apply_part`s) | `date.rs:212` (`DateValue::apply`), `date.rs:603` (`DateTimeValue::apply`), `duration.rs:771` (+ duration tests 1995/2001/2024/2265/2283/2295) |

Verified by grep: these are the **only** two `trunc/fract` f64→duration algorithms in the crate (`src/duration.rs:1052-1053` and the six `date.rs` sites; nothing else).

**OBSERVED problems:**

1. Ticket 03's remediation declares "`seconds_delta`: the single whole/sub-second split for `DateValue::apply_part` and `DateTimeValue::apply_part`." The claim is **narrowly true, design-false**: the whole-duration path (`apply`) uses a *different* split owned by `duration.rs`.
2. Mislabeled error at `duration.rs:1054-1057`: a finite-but-out-of-`TimeDelta`-range `f64` returns `NonFiniteSeconds` (the type doc at :1048 admits the conflation: "non-finite **or outside TimeDelta's representable range**"). The variant name lies for the range case.
3. Both date call sites then erase the diagnosis: `.map_err(|_| DateError::OutOfRange)` (`date.rs:213`, `date.rs:604`) — against the house source-chain rule that motivated 03's engine remediation (which deliberately preserved `LocalZoneLookup` chains).

**Equivalence check (performed):** spot-evaluated `total = -0.5` and `total = -1e-10` through both algorithms — same normalized result (`-0.5s`, `0s` respectively; `TimeDelta::new(-1, 1_000_000_000)` normalizes to `0`). So the duplication is *currently behavior-compatible*, which is exactly why it has survived — a maintenance trap, not a live bug.

**Analysis:** one concept ("put f64 seconds on a timeline") has two owners, two normalizations, two error vocabularies, and one documented source-erasure. Consolidation: `apply`'s fixed branch calls the same conversion the parts path uses (`seconds_delta(duration.to_seconds().0)` — `DurationSeconds.0` is `pub(crate)` and readable from `date.rs`), deleting `map_err` entirely *and* improving the source chain.

**Trade-offs:** `duration.rs`'s `TryFrom` impl would remain for `TryFrom<DurationValue>` (`duration.rs:771`) — so "one owner" means *one algorithm*, either by having `date.rs` call the duration impl (and map `DurationError → DateError` once, preserving the source) or by having `duration.rs`'s impl call a shared helper. The `NonFiniteSeconds`-for-range mislabel in `duration.rs` is adjacent-scope: flagged here, owned by whoever does C4.

**Corpus:** `conflicts-with-03` (the single-owner *intent*, not the letter — 03's wording is scoped to `apply_part`). New as a finding.

---

##### C5 — `apply` / `apply_part` clone pair across the two frame impls

**Kind:** duplicated policy across impls · **Probe:** compression · **Direction:** extract the **part decomposition + fold** — explicitly *not* a frame trait

**OBSERVED (mirrored structure)**

| Step | `DateValue` | `DateTimeValue` |
| --- | --- | --- |
| `apply` entry | `date.rs:197-218` | `date.rs:590-611` |
| `dead_code` attr | :192 | :585 |
| non-finite guard | :210 | :601 |
| whole→`TimeDelta` | :212 (`TryFrom`) | :603 (`TryFrom`) |
| `parts=None` fixed branch | :214-217 | :605-610 |
| `apply_part` | `date.rs:233-280` | `date.rs:628-676` |
| `mag.trunc().to_i64()` | :247 | :642 |
| `mag.fract() * unit.fixed_seconds()` | :253 | :648 |
| left-to-right fold, `unit`-vs-`Ms` ordering policy | :236-278 | :631-674 |
| `OutOfRange` on overflow | both | both |
| frame-specific: civil midnight + `.date()` | :256-258, :271-274 | local wall + resolver + instant :654-660 |

Additional intra-impl duplication: the midnight chain `self.0.and_hms_opt(0,0,0).ok_or(DateError::OutOfRange)` appears **four times** in `DateValue` alone (`:175`, `:211`, `:256-258`, `:272-274`) plus a fifth at `engine:226-229`.

**What genuinely differs:** the frame. Civil `NaiveDate::and_hms(0,0,0)` + `.date()` vs local wall + `local_naive_to_utc` + instant. That difference is real and must stay in the two impls — two adapters per behavior is codebase-design's *real-seam* test, and the corpus rejected the public `Shift` trait (review §5.7).

**Analysis:** ~40 lines of identical decomposition/fold can live in one free fn generic over `NaiveDateTime` (both frames hold a `NaiveDateTime` at that point), leaving each `apply_part` owning only resolve/unresolve. The extraction is the *same kind* as 03's `seconds_delta` — a narrow helper with today's exact surface, no trait, no `dyn`, no 4-method interface.

**Constraints (self-imposed to stay legal):**

- The exhaustive unit-classification matches stay side-by-side and untouched (03's gate). The extraction must not smuggle a shared `unit` predicate behind it — if it does, it *is* the rejected `is_calendar()`.
- The public `Shift` trait stays rejected: no trait object, no public seam, no `LocalWall` newtype.

**Trade-offs:** this is the candidate most at risk of collapsing back into a rejected proposal if the extraction is done greedily. If a fold helper can't be extracted while keeping both classification matches legible and exhaustive, the honest disposition is **adjudicated repetition** (dismiss), not a forced merge.

**Corpus:** `new`, consciously inside the 03/review boundary; flagged for adjudication.

---

##### C6 — `DatePoint`'s invariant is built by its only caller, field-by-field

**Kind:** representable-invalid / missing constructor · **Probe:** expansion · **Direction:** own construction from the parse result; fields go read-only

**OBSERVED**

- `date.rs:924-929` — `pub(crate) wall`, `instant`, `has_time`.
- `date.rs:937-947` — `DatePoint::new(wall, instant, has_time)` asserts nothing; the caller supplies the third boolean.
- Engine supplies literals: `DatePoint::new(wall, instant, true)` (`engine:218`), `DatePoint::new(wall, instant, false)` (`engine:232`).
- Field reads at 7+ sites: engine 214, 246, 348, 379, 395, 482-485, 640, 659, 669.
- Forced derivations the bool creates:
  - `ParsedDate::precision()` re-derives `DatePrecision` from `has_time` (`engine:238-244`) — two names for one bit.
  - The datetime branch recomputes its own wall through an identity round-trip: `let instant = value.into_inner(); let wall = DateTimeValue::from(instant).local_wall()` (`engine:211-216`) ≡ `value.local_wall()` (correction R10).
- Unenforced invariants: `wall` needn't be `instant`'s local wall; `has_time == true` needn't mean `wall` carries a time-of-day.

**Analysis:** the triple is a *derived* representation (one parsed input + zone → three projections). Today the engine owns the derivation, which is why it can drift and why `pub(crate)` field reads are the API. Owning construction (e.g. `DatePoint::from_parts(...)` or two constructors `at_wall` / `at_instant` keyed by the recognition result) makes the projection one door, and lets fields become read-only in practice.

**Trade-offs:** adding constructors to a `pub(crate)`-field struct is only worth it if the fields actually go private — otherwise it's ceremony. That makes C6 a package deal: constructor **and** field encapsulation, or neither.

**Corpus:** partially `superseded-by-04` — 04 hoists `Precision` and makes `has_time` derive from it. **Residue 04 does not cover:** the wall/instant pairing, the literal-bool construction seam, field privacy. **PREDICTED (post-04):** the engine holds two representations of one input — the recognition result `{value, precision}` *and* the `DatePoint` — unless construction sits behind one door. Flag as a note for 04's design section, not a competing proposal.

---

##### C7 — `DateTimeFormat::parse` returns a value typed as an instant it didn't parse

**Kind:** representable-invalid / type-lies · **Probe:** expansion · **Direction:** return a discriminated parse output (wall-or-instant), or branch on `zone()` before parsing

**OBSERVED**

- `date.rs:905-916` — for naive (no-offset) patterns, the chrono `NaiveDateTime` is forced through `and_utc()` into a `DateTime<Utc>` — an instant the source never declared.
- `date.rs:421-427` (`DateTimeValue::parse_iso`) must undo the label:

  ```rust
  if !matches!(matched, DateTimeFormat::Rfc3339) { … parsed.naive_utc() … }
  ```

  with a comment admitting the round-trip exists only to recover the wall from the mis-typed value. `and_utc().naive_utc()` is an **identity** — correctness today rests on an inversion, not a conversion.
- Doc at `date.rs:897-899` describes the contract without naming the placeholder.
- `matched` tracking (`date.rs:407-416`) is correct on inspection (assigned before each attempt; breaks on first success; on total failure `matched` is unused because the `Err` propagates) — the bug is the *return type*, not the loop.

**Analysis:** the cascade's real output is two-valued — offset-bearing → instant, naive → wall. Encoding "always instant" pushes a recovery step onto every non-`Rfc3339` success path and makes the correct branch depend on an identity law. The clean shape is the same as C2's: return what was recognized.

**Trade-offs:** small blast radius (one consumer, `DateTimeValue::parse_iso`), private representation choice, no public API movement. But it is a *correctness hinge*: any future edit to the placeholder path (e.g. a real offset) silently changes semantics.

**Corpus:** `new` — repo-wide grep for "placeholder" hits only D12's wall-clock round-trip text; no ticket mentions it. **Coordination flag:** 04's `parse_any` inherits the question — its spec says what `classify` *accepts* but not what the datetime cascade *returns* for naive vs offset inputs. Settle inside 04 rather than after it.

---

##### C8 — `DateError` carries recognition-only variants into arithmetic seams; `DatePoint::diff` returns an unreachable error

**Kind:** interface narrowing · **Probe:** expansion (or narrow) · **Direction:** two options, choose deliberately

**OBSERVED**

- `date.rs:1309-1317` — `DateError::InvalidPattern` has **zero producers** (repo grep: declaration at :1314, match at `engine/date.rs:747`, nothing else). Corpus references it only as "we keep `InvalidPattern`" (review:89/110); no ticket owns it.
- Engine `date_error` (`engine:730-751`) maps `Unparseable | InvalidYearDigits | InvalidPattern` → `"date arithmetic overflowed"`, with a doc line admitting they "cannot occur here" — a translation function that mostly translates *impossible* into *misleading*.
- `DatePoint::diff` (`date.rs:960-964`): "`OutOfRange` … defensive only — no input produces it"; `engine:640` still pays `map_err(date_error)`.

**Analysis:** three *recognition* outcomes (`Unparseable`, `InvalidYearDigits`, `InvalidPattern`) ride the same enum as four *arithmetic/zone* outcomes (`OutOfRange`, `LocalZoneLookup`, …), and every arithmetic seam re-filters them. Two coherent designs:

- **(a)** Split recognition vs arithmetic errors inside `date.rs` (`DateParseError` / `DateError`) — this is **not** the rejected Date+Duration merge; it is a within-domain split, and it makes `date_error` lossless.
- **(b)** Keep one enum and narrow the *seams* — e.g. `diff` returning a non-`Result` (or a `#[derive]`-narrow error) once its only error is provably unreachable.

**Trade-off:** (a) costs two public error types and two conversion points in minijinja's error path — exactly the simplification ticket 03 deliberately made. That simplification is why this ranks below C1–C5: it is a judgment call, not a defect.

**Corpus:** `new` (neither 03 nor 04 adjudicates a within-domain split). `InvalidPattern`'s liveness is `unowned-by-any-ticket` — check ticket 06 before proposing removal (coverage gap #4).

---

##### C9 — Minor cluster (each independently small)

| ID | Finding | Evidence | Probe / direction | Corpus |
| --- | --- | --- | --- | --- |
| **a** | Midnight construction ×4 inside `DateValue` (×5 with engine) | `date.rs:175`, `:211`, `:256-258`, `:272-274`; engine `:226-229` — identical `and_hms_opt(0,0,0).ok_or(DateError::OutOfRange)` | expansion → private `DateValue::midnight_wall()` (an associated fn, not a method — no `self` in three of four sites) | `new` |
| **b** | `TextShape` sits in the *sort* module, consumed by the *grammar* | def `src/query/sort.rs:457-466`; import `src/query/grammar/filter.rs:26` (`sort::TextShape`); use `:190-194` | expansion → relocate to a query-level location (04 makes it a caller but does not settle where it lives) | `new` |
| **c** | `is_iso_shape` re-implements the 4-digit check inline | `date.rs:130` vs `date.rs:115-118` | compression → delegate | `superseded-by-04` (guards retire) |
| **d** | `to_datetime_string` is a one-caller Display forwarder | def `date.rs:436-438`; prod caller `engine/query.rs:606`; tests 1464-1482, 1516-1522 | compression → `to_string()` | `new` (09's audit lists `to_date_string` as live, never mentions this one — correction R8) |
| **e** | `parse_year_month` double-parses to fabricate a chrono error | `date.rs:824-848` — second pass exists only so `DateError::Unparseable.source` can hold a chrono error | compression → construct the source once | `new`; driven by the `err-source-chain` house rule — fix carefully or dismiss |
| **f** | `DateError::InvalidPattern` dead variant | `date.rs:1314`; match `engine:747`; zero producers | compression → remove (or promote to a real parse outcome) | `unowned-by-any-ticket` (see C8) |

Not carried as findings (below threshold, recorded for the audit trail): `checked_neg` repetition in `date_sub`/`date_sub_days`/`date_sub_months`/`date_sub_years` (R7); the shared `(earlier, later, sign)` ordering micro-pattern between `signed_years_since` (:1218) and `signed_months_since` (:1249) — same shape, two call sites, no policy divergence.

---

#### 3. Seed candidates that did NOT survive (rejected / dismissed with reasons)

| Seed candidate | Probe outcome | Disposition |
| --- | --- | --- |
| Group `local_naive_to_utc` + `resolve_gap_offset` into a `mod zone` | expansion — fails the depth-propagation test: all callers are internal, the interface does not change, no knowledge is deduplicated | **Dismissed.** Corpus additionally requires these stay untouched (03 scope: "unchanged"). |
| `Clock` / `TimeZone` traits for zone lookup | one adapter = hypothetical seam (review corpus) | **Rejected by corpus** — not re-proposed. |
| `DurationUnit::class()` / `is_calendar()` shared predicate | compression — would collapse the 5 intentional exhaustive sites into a hidden classification | **Rejected by corpus** — C3 explicitly preserves those sites. |
| Public `Shift` trait (frame trait for `apply`/`shift`) | rejected in review §5.7; C5 is the narrow non-trait extraction that survives | **Rejected by corpus** — C5 is the legal residue. |
| `DateError` + `DurationError` merge | rejected by corpus | **Not re-proposed** (C8 splits *within* `date.rs` instead, and says so). |
| `DateError` `#[non_exhaustive]` | verified already landed (`date.rs:1286+`, N17/S6) | **Already done.** |
| `LocalWall` newtype to make wall/instant pairing explicit | rejected by corpus; C6 pursues only the *constructor* | **Not re-proposed.** |
| Combined date-or-duration mega-classifier | rejected by corpus | **Not re-proposed** — C2 deliberately keeps atomness at the parser seam to avoid a miniature version of it. |
| `DurationValue` type split | rejected by corpus | **Not re-proposed.** |
| `date/` directory split / module-tree reorg | rejected ("don't preempt"); depth test fails for mechanical grouping | **Not re-proposed.** |
| Delete test-only `DateValue::{shift, apply}` / `DateTimeValue::apply` (`dead_code` 165/192/585) | adjudicated as declared surface for ticket 05 | **Adjudicated — not dead code.** |
| Delete `DateValue::to_date_string` | live at `date.rs:330` (`Serialize`) + `engine/query.rs:604`; ticket 09 explicitly marks it live | **Rejected (correction R1).** |
| Fold `ParsedDate`/`DatePrecision`/`format_precise` duplication | ticket 04 deletes all three (D10) | `superseded-by-04`. |
| Retire `is_iso_shape` / `has_four_digit_year` publicity | ticket 04 item 13 (guards go private) | `superseded-by-04` — except its *length* dimension, which survives as C2. |
| Collapse the three datetime→date parse cascades | 04's `parse_any` (format enums own one loop) | `superseded-by-04`. |
| Unify `parse_iso` cascades | 04 `parse_any` | `superseded-by-04`. |
| `From<NaiveDate> for DateValue` unvalidated wrapper (`date.rs:297`) | D7: `NaiveDate` is already a valid domain value — surface-area, not a parse-don't-validate hole | **Adjudicated — negative finding.** |
| Engine `checked_neg` ×4 (R7) | one-line guards, below threshold | **Dismissed.** |
| `signed_*_since` ordering micro-duplication | two sites, no divergence | **Dismissed.** |
| Engine `date_error` / `DateDiff → Value` translation as a misplaced seam | minijinja must not leak into `date.rs`; this *is* 03's remediation | **Correct seam — negative finding.** |
| Replace `local_now` / wall-vs-instant compare split | verified landed (D9/02): `Utc::now()` for compares, local clock for display | **Already done.** |
| `DatePoint::has_time` vs `Precision` duplication | 04 hoists `Precision`, derives `has_time` | `superseded-by-04` (residue → C6). |

---

#### 4. Negative findings — checked and correctly shaped

Recording these is part of the contract: an item that *looks* like a candidate and survives scrutiny is evidence the review actually ran the probes.

1. **`local_naive_to_utc` (`date.rs:1051`) / `resolve_gap_offset` (`:1097`) as free fns** — zone policy with no natural `self`; grouping them fails the depth-propagation test (internal callers only, interface unchanged). Correctly free.
2. **`seconds_delta` (`:1119`), `shift_months` (`:1199`), `signed_*_since` (`:1218`/`:1249`)** — ticket 03's declared owners: private, single-caller chains, one job each. Correct as-is (with the C4 caveat about the *other* split).
3. **`DurationUnit` exhaustive classification at 5 sites** — adjudicated intentional; C3 does not touch classification.
4. **Dead test-only `DateValue::{shift, apply}` / `DateTimeValue::apply`** — declared production surface for ticket 05. Their `dead_code` attrs are a feature (they document the intention), not a finding.
5. **`ParsedDate` / `DatePrecision` / `format_precise`** — look like single-purpose wrappers in isolation; 04 deletes them.
6. **Engine `date_error` translation + `DateDiff → Value` mapping** — the correct place for minijinja concerns; owned by 03's remediation.
7. **Clock doctrine** — verified: no `Local::now`; compares through `Utc::now()` (`engine:95-97`); `date.now()/today()/tomorrow()/yesterday()/from_timestamp()` read the local clock for display only. Landed.
8. **Parse funnel** — verified clean: every production `NaiveDate::from_ymd_opt` outside `date.rs` is a test/doc example. The module-doc claim (`engine:19-21`) holds.
9. **`From<NaiveDate> for DateValue` (`:297`)** — adjudicated surface-area (D7).
10. **`is_iso_shape` / `has_four_digit_year` reaching production from 4+2 sites** — all reachability is production; there is no test-only reachability that would let 04's privacy change hide a break.
11. **`DateTimeFormat::parse`'s `matched` loop** — verified correct (assign-before-attempt, break-on-success, `matched` unused on failure); C7 targets the return type only.
12. **`DateDiff`/`DatePoint`/`DateError` export shape** — `mod date` is private (`lib.rs:63`) with selective `pub(crate)`/`pub` re-exports (`lib.rs:92-93`); `FromStr::Err = DateError` is public while `parse_iso` is `pub(crate)` — coherent with N17.

---

#### 5. Correction register (seed → verified)

Flattened from §1.5 for grep-ability:

| ID | Correction |
| --- | --- |
| R1 | `to_date_string` is **live** (`date.rs:330` Serialize + `engine/query.rs:604`); 09 adjudicates it live — seed candidate withdrawn. |
| R2 | `seconds_delta` is the single split owner **only for `apply_part`**; `apply` uses `duration.rs:1043` — becomes C4. |
| R3 | Date-domain consumption sites: **4, not 3** — `note/field.rs:114` added, absent from 04's inventory. |
| R4 | 04's `classify` returns **no consumed length** while requiring guards to go private — becomes C2 (`conflicts-with-04`). |
| R5 | 04 covers `has_time`→`Precision` only; the construction seam is residue — becomes C6. |
| R6 | `LocalWall` rejection ≠ constructor question; C6 framed as construction only. |
| R7 | `checked_neg` ×4 below threshold — dismissed. |
| R8 | 09's checklist is **stale/undercounted**: 03 already deleted the `DateTimeValue` formatting cluster; `to_datetime_string` never listed. |
| R9 | `shift_wall`'s doc claims a wrapper relationship that `DateTimeValue::shift` implements as a **copy** — C3's core evidence. |
| R10 | `engine:211-216` is an identity round-trip (`into_inner` + re-wrap) — micro-evidence for C6. |
| R11 | CodeGraph's `shift_wall` reference count (3) includes the import line; rg call-site count is **2** (one external). Reporting uses rg. |
| R12 | `DurationError::NonFiniteSeconds` is returned for **out-of-range finite** input (`duration.rs:1054-1057`) — a name that lies, documented at :1048. Owned by C4's execution. |

---

### 12.2 Component report: `src/duration.rs` and its callers

**Review type:** adversarial design review (research only — no code changes, no fixers executed).
**Primary scope:** `src/duration.rs` (~1230 prod lines + ~1068 test lines); production callers as needed.
**Session date:** 2026-10-05.
**Method authority:** `.claude/skills/codebase-design/SKILL.md`, `.agents/skills/rust-design/DISCOVERY.md`, `.agents/skills/rust-design/DEEPENING.md`.
**Corpus authority:** `.scratch/temporal-core/review.md`, `spec.md`, `issues/01,03,04,05,09`.

---

#### 0. Scope, method, evidence rules

##### 0.1 What was read

- `src/duration.rs` in full for production (lines 1–1230) and the test modules that matter for the findings (`mod prefix`, `mod parts`, `mod arithmetic`, `mod regime`, `mod error`, Display/serde tests).
- All listed caller files: `src/note/parser/inline.rs`, `src/note/parser/lexer.rs`, `src/note/field.rs`, `src/field.rs`, `src/query/sort.rs`, `src/query/grammar/filter.rs`, `src/template/engine/date.rs`, `src/date.rs`, `src/lib.rs`.
- Corpus: `review.md`, `spec.md`, `issues/03-duration-semantics-owner.md` (full handoff incl. "Deliberate remaining state"), `issues/04-recognize-with-precision.md`, `issues/05-*`, `issues/09-dead-surface-cleanup.md`.
- Git history probes: `git log -S "fn normalize_zero"` for the signed-zero duplication.

##### 0.2 Evidence rules applied

- Every finding carries `file:line` (or `rg`-verified absence), caller evidence, and a deletion/expansion probe where one applies.
- **OBSERVED** = read directly in source this session. **PREDICTED** = traced through source by hand, not executed (no edits were allowed).
- Each candidate is tagged: `genuinely new` / `conflicts-with-corpus-decision` / `superseded-by-ticket-04`.
- Negative findings (probes that failed) are recorded in §5 rather than dropped.

##### 0.3 Corpus decisions honored (do not re-propose)

| Decision | Status |
| --- | --- |
| A2′ — `parts` as witness, `None` ⇒ fixed | SETTLED |
| Bit-exact Σ as single L→R fold over signed entries | SETTLED (spec:88, 03:13) |
| `fixed_seconds()` rename; one ratio table (`fixed_seconds_i64` derives) | SETTLED (03:17) |
| Calendar owner lives in the date module | SETTLED |
| Calendar-vs-fixed classification repeated as exhaustive arm shape at ~5 sites (`is_calendar`, `shift_wall`, `DateTimeValue::shift`, both `apply_part`s, `DatePoint::diff`) | INTENTIONAL — 03:174–180; **REJECTED** shared predicate (`DurationUnit::class()` / replacing matches with `is_calendar()`) |
| `DurationValue` type split (parse-machinery vs value) | **REJECTED** — 03:171–173: "no split — the module is the spec-mandated single registry; splitting would fragment the owner without relocating knowledge" |
| `Cow<'raw>`, merged error enums, `DurationValue` value/object split, typestate builders | REJECTED per handoff |
| `from_seconds` | RETAINED — 09:12 (synthesis constructor; no removal-list entry may name it) |
| Guard protocol / `parse`↔`parse_prefix` loop merge | **DECIDED in ticket 04** (classify trichotomy; "one scanner, two wrappers") |
| `DurationValue::parts`, `is_calendar`, `from_seconds` `expect(dead_code)` | DELIBERATE — 03:164–169; declared production consumers = 04's `classify` / query #05 (ticket 05) |
| No duration/date Criterion bench exists; perf notes are hypotheses | NAMED DEFERRAL — 03:190–193 |

---

#### 1. File map (verified)

`src/duration.rs` = **2299 lines**: production 1–1230, `#[cfg(test)]` from 1231.

| Region | Lines | Contents |
| --- | --- | --- |
| Crate-level constants | 36–89 | `DISPLAY_EXPONENT_LOWER` (36–37), `UNIT_HINT` (docs 39–43, const 44–46), `UNIT_MAP` (docs 48–50, table follows) |
| `impl DurationValue` | 123–752 | value core, parsing, synthesis, probes (see §2 table) |
| `SUB_YEAR_DECOMPOSITION_UNITS` | 130 | const; ratios derived via `fixed_seconds()` (N18 settled) |
| `parse` | 169–~230 | loop 182–203; `raw: trimmed` store 214; `Ok(Self{…})` 215–220 |
| `parse_prefix` | 233–~296 | loop 246–271; `raw: input[..last_end].trim()` store 281 |
| Trait impls | 754–910 | `TryFrom<DurationValue> for TimeDelta` (763), `Add`/`Sub`/`Mul`, serde |
| `type DurationParts = Box<[(f64, DurationUnit)]>` | 913 | single-use alias (used once, line 120: `parts: Option<DurationParts>`) |
| `DurationUnit` registry | 915–1003 | `parse`, `fixed_seconds`, `fixed_seconds_i64`, display spellings |
| `DurationSeconds` | 1005–1153 | tuple struct (1015), `normalized` (1022), `TryFrom` (~1038–1050), `PartialEq`/`Ord` via `total_cmp` (1066–1088), arithmetic via `normalized` (1123/1132/1141/1151) |
| `DurationError` | 1155–1229 | single merged enum |
| Tests | 1231–2299 | `mod prefix` ~1437, `mod parts` ~1776, `mod arithmetic` ~1851, `mod regime` ~2030 |

---

#### 2. Seed verification

##### 2.1 Complete table — all 16 no-self associated fns in `impl DurationValue` (123–752)

Seed claimed 16 functions without a `self` receiver: 13 "machinery" + 3 entry points. **All 16 exist at the cited lines.**

| # | Function | Line (verified) | Category | Callers / notes (verified this session) | Verdict |
| --- | --- | --- | --- | --- | --- |
| 1 | `parse` | 169 | **entry point** (whole-input ctor) | prod: `note/field.rs:531` (unguarded, safe-by-fallback), `query/sort.rs:488` (guarded at 487), `FromStr` + serde | ✅ seed correct |
| 2 | `parse_prefix` | 233 | **entry point** (prefix ctor) | prod: `note/parser/inline.rs:186` (sole production caller) | ✅ seed correct |
| 3 | `can_start` | 499 | **entry point** (O(1) gate) | prod: `query/sort.rs:487` **only**; delegates to #4 | ✅ seed correct |
| 4 | `can_start_duration_segment` | 504 | scanner/cursor | probed by `can_start`; sign+digit grammar for the lookahead | ✅ seed correct |
| 5 | `digits_after_sign` | 530 | scanner/cursor (shared grammar rule) | shared by probe (#4) and number scan (#8) — one owner for the sign rule (03 remediation) | ✅ seed correct |
| 6 | `skip_separators` | 539 | scanner/cursor | both loops: 183 (parse), 263 (parse_prefix); allows `','` + whitespace | ✅ seed correct |
| 7 | `skip_whitespace` | 553 | scanner/cursor | whitespace-only; used inside part scanning (`scan_part`) — near-clone of #6 | ✅ seed correct |
| 8 | `parse_number` | 570 | scanner/cursor (scan half) | called from the per-part scan; delegates span→value to #9 | ✅ seed correct |
| 9 | `parsed_number` | 624 | scanner/cursor (validate/convert half) | **sole caller ~620 inside `parse_number`** — one-to-one delegation pair (see §5 rejection) | ✅ seed correct |
| 10 | `parse_unit` | 656 | scanner/cursor (scan half) | alphabetic-run scan, then delegates to #11 | ✅ seed correct |
| 11 | `parsed_unit` | 675 | scanner/cursor (validate/convert half) | **sole caller ~671 inside `parse_unit`**; owns `MissingUnit`/`UnknownUnit` error construction (see §5) | ✅ seed correct |
| 12 | `scan_part` | 297 | scanner/cursor (composite step) | per-part scan extracted by the 03 audit remediation ("per-part scan extracted (`scan_part`)") | ✅ seed correct |
| 13 | `apply_sign` | 314 | parts-witness | exactly 2 prod sites: 209 (parse), 277 (parse_prefix) — post-pass over `raw_parts` | ✅ seed correct |
| 14 | `fold_parts` | 333 | parts-witness | **4 prod sites: 211, 279, 727 (`combine`), 854 (`Mul`)** + tests 1833/1894/1957 — the Σ owner | ✅ seed correct |
| 15 | `canonical_raw` | 380 | synthesis | sole caller 361 (`from_seconds`); greedy sub-year decomposition, fallback `format!("{}s", DurationSeconds(total_secs))` at 417 | ✅ seed correct |
| 16 | `from_seconds` | 359 | constructor (synthesis) | test-only prod today; **retained by 09:12**; `Add`/`Sub` re-synthesize through it (spec:88) | ✅ seed correct |

**Category roll-up of the 16:** 3 entry points + 1 ctor + 1 synthesis helper = 5 that genuinely belong to `DurationValue`'s public-ish contract; **9 scanner/cursor helpers + 2 parts-witness helpers = 11 that are foreign to the value type's namespace.** That ratio is the LCOM4=2 root cause — but see finding **F4** for why the seed's remedy (`Scanner` struct) is rejected.

##### 2.2 Seed claims that failed or needed correction

| Seed claim | Verdict | Evidence |
| --- | --- | --- |
| "`can_start` guard in `src/note/parser/lexer.rs`" | **FALSE** | `rg can_start src/note/parser/lexer.rs` → no match. The lexer's gate is `DateValue::is_iso_shape` at `lexer.rs:250` (ISO length const at `lexer.rs:247`, per 04:16). The seed appears to have conflated the date gate with the duration gate. |
| "`can_start` guard" as the recognition protocol | **Census corrected** — one guard site, not a protocol | Production `can_start` callers: `query/sort.rs:487` only (verified). `query/grammar/filter.rs:186–193` shares `TextShape::classify` with sort — **one** query classifier, not two independent guards. `note/parser/inline.rs:186` needs no guard (prefix contract consumes what it recognizes). |
| `note/field.rs:531` flagged as "N7 live" (unguarded) | **Live but benign** — severity downgraded | `note/field.rs:509–536`: `else if let Ok(dv) = DurationValue::parse(trimmed) { Self::Duration(dv) } else { Self::String(s.into_owned()) }`. An `Err` falls back to `String`, so the missing guard costs one failed parse, never correctness. The guard protocol is therefore *performance knowledge with no owner*, not a correctness requirement (feeds 04's residue notes). |
| "`dur(\"1h 1x\")` vs `parse_prefix(\"1h 1x\")` divergence (N8)" | **Not a divergence** | Both reject: `parse` → `Err(UnknownUnit)`, `parse_prefix` → `None` → inline falls back to text. 04:15 pins this as "one grammar with two shapes", both to be pinned. |
| Line-count / structure claims | **Confirmed with correction** | File = 2299 lines, prod 1–1230 (seed's "1230" refers to prod only). |
| messrust gauges (`DurationValue` complexity 78, LCOM4=2) | **Cited, not re-run** | From 03:171 handoff; no mess run executed this session (§8). |

##### 2.3 Gate / recognition census (final, corrected)

| Seam | File:line | Mechanism | Status |
| --- | --- | --- | --- |
| Query sort | `src/query/sort.rs:477–488` | `TextShape::classify`: `can_start(&s)` then `DurationValue::parse` → `SortKey::Duration` | OBSERVED — the only guarded site |
| Query filter | `src/query/grammar/filter.rs:186–193` | shares `TextShape::classify` | OBSERVED — same classifier, no second protocol |
| Field coercion | `src/note/field.rs:531` | unguarded `parse`, `Err` → `String` fallback | OBSERVED — safe-by-fallback |
| Inline atom | `src/note/parser/inline.rs:186` | `parse_prefix`, returns consumed-byte count | OBSERVED — contract pinned by 04:15 |
| Inline date gate | `src/note/parser/lexer.rs:250` | `DateValue::is_iso_shape` (not duration-related) | OBSERVED — seed's cited location, different type |
| Duration gate fn itself | `src/duration.rs:499–528` | `can_start` → `can_start_duration_segment` + `digits_after_sign` | OBSERVED — `pub(crate)`, one caller (superseded-by-04 privacy item) |

---

#### 3. Production caller census (verified)

| Surface | Production callers (file:line) | Test-only |
| --- | --- | --- |
| `DurationValue::parse` | `note/field.rs:531`, `query/sort.rs:488`, `FromStr`/serde | — |
| `DurationValue::parse_prefix` | `note/parser/inline.rs:186` | inline tests |
| `DurationValue::can_start` | `query/sort.rs:487` | — |
| `to_seconds` | `query/sort.rs:377,398`, `date.rs:212,603` | — |
| `parts()` | `date.rs:201,594` | — |
| `as_str` | `note/field.rs:85,304,338`, `template/engine/query.rs:603` | inline tests |
| `is_calendar` | **none** (03:164–169 deliberate `expect(dead_code)`; declared consumer = 04 `classify` / query #05) | `duration.rs:2036–2055` |
| `from_seconds` | none today (09:12 retains as synthesis ctor; `Add`/`Sub` re-synthesis via spec:88) | Display/serde tests |
| `Add`/`Sub`/`Mul` | none today — **ticket 05** is the declared consumer (query `date + duration`) | `mod arithmetic` |

Absence probes: no duration benches exist in `benches/` (03:192); `UNIT_HINT` is re-exported only for `template/engine/date.rs` (`lib.rs:101`).

---

#### 4. Findings, ranked

##### F1 — `','` has two grammar owners; the precedence is an accident, unpinned, unregistered
`genuinely new` · kind: duplicated policy / policy with no owner (cross-seam) · OBSERVED (collision), PREDICTED (outcomes)

**Evidence**
- Duration claims `,` as a part separator: module doc `duration.rs:5`, `parse` docs `duration.rs:141–142` ("separated by whitespace or commas"), lookahead in both loops skips separators (`skip_separators` at 183/263, which admits `,`).
- Note-parser claims `,` as the list delimiter: `inline.rs:67,82` (list rule), `parse_comma_list_from` at `inline.rs:91`, boundary test `treats_comma_as_atom_boundary` (mod boundary, ~375).
- YAML assigns the same characters to list splitting *before* coercion: `note/field.rs:656` test `list: [1h, 30m]` → `List([Duration(1h), Duration(30m)])`.

**What actually happens (traced through `parse_atom_at` ordering, not executed):** duration is tried before the list path, and `parse_prefix`'s continuation lookahead swallows `,` as a separator — so the first-atom priority decides everything:

| Inline input | PREDICTED result | Why |
| --- | --- | --- |
| `key:: 1h, 30m` | **one** `Duration(5400s)` | lookahead consumes `", "` → third part |
| `key:: 1h, 2h` | **one** `Duration(10800s)` | same |
| `key:: 1h, 45` | **`String("1h, 45")`** | part 2 lacks a unit → `MissingUnit` → whole prefix `None` → no atom → raw string |
| `key:: 1h, x` | **`String("1h, x")`** | lookahead sees `x` cannot start a part → break; then list path sees `x` unparseable → fallback to string |
| `key:: 1h,` | **`List([Duration(1h)])`** | atom consumed, comma at end → one-element list |

**Analysis:** a duration-shaped first item either *eats* the inline list or *breaks* it into a raw `String`. Inline duration-first lists are effectively unusable, the YAML seam silently disagrees with the inline seam for identical characters, and — unlike `YYYY-MM` (04:16 explicitly records it) — **no divergence-register entry, ticket, or test covers this** (rg for comma/precedence across `.scratch/temporal-core/` finds nothing; spec.md:117 register list has no entry).

**Trade-offs / direction:** the fix is not in `duration.rs` alone — it is a note-parser precedence decision (keep duration-wins and pin it, or try list before duration when a `","` follows a *complete* atom with remaining content). Probe cost: 5 asserts in `inline.rs` tests + one register entry.

**Corpus status:** `genuinely new`; touches but is not superseded by 04 (04 owns the loop and `classify`, not list precedence).

---

##### F2 — `UNIT_HINT`↔`UNIT_MAP` sync is comment-enforced only
`genuinely new` · kind: policy with no owner (no executable check) · OBSERVED

**Evidence**
- `UNIT_HINT` (`duration.rs:44–46`) with its sync contract stated in prose at `duration.rs:39–43`: "Kept beside `UNIT_MAP` as a reminder to update both when a unit or spelling family is added."
- Consumed by the user-facing message: `template/engine/date.rs:764` inside `unknown_unit_error` (756–766), whose doc **claims** "the message cannot drift from what `unit_kwarg` actually accepts" (756–758). No test asserts that claim (rg: no test references `UNIT_HINT`).
- Corpus precedent for this exact failure class: review **N2** — registry drift "hid weeks."

**Analysis:** two representations of "what units are accepted" (table + prose list), one human-reminder sync, one doc comment asserting a guarantee nothing enforces. Unlike the settled S2 fix (ratios — now derived), the *message* copy of the registry is still hand-maintained.

**Direction:** make it executable — a test that every hint spelling resolves via `DurationUnit::parse` and every `UNIT_MAP` family appears in the hint. Expansion probe: adding a unit family currently requires reading the comment to notice the second site.

**Corpus status:** `genuinely new` (distinct from S2/N18, which concern ratios).

---

##### F3 — `DurationSeconds` documents a type invariant its field visibility does not enforce
`genuinely new` · kind: representable-but-invalid state · OBSERVED

**Evidence**
- Type docs assert: "A signed zero normalizes to positive zero **at construction**, so `\"-0m\"` and `\"0m\"` compare, order, and hash identically" and `normalized`'s doc: "so the type's invariant (see the type docs) holds **everywhere**, not just at parse time" (`duration.rs:1007–1014`, `1020–1021`).
- The field is `pub(crate)`: `pub(crate) struct DurationSeconds(pub(crate) f64)` (`duration.rs:1015`). Any of duration.rs's ~50 construction sites can write the tuple and bypass both `normalized` (1022) and `TryFrom`'s finiteness check.
- **Actual violations today: one, and it is benign.** `DurationSeconds(total_secs)` at `duration.rs:417` (the `canonical_raw` fallback when the greedy decomposition writes nothing) — benign because `canonical_raw` returns early on `total_secs == 0.0` (line 383, which is also true for `-0.0`), and upstream construction guarantees finiteness. OBSERVED.
- **Zero external reach:** `rg "DurationSeconds\("` across `src/` matches only the definition (1015) and site 417. `query/sort.rs` constructs via `DurationSeconds::try_from` (756/758/934).

**Analysis:** the invariant is a *convention inside one file*, while the type docs phrase it as a type-level fact. Narrowing the field to private (`struct DurationSeconds(f64)`) forces every site through `normalized`/`TryFrom`, makes the doc true by construction, and costs zero churn outside duration.rs (site 417 becomes `normalized(total_secs)` or stays an explicit, reviewed exception).

**Probe:** expansion test — adding a duration source (e.g. serde path) today must *remember* the rule; after narrowing it cannot forget. Deletion test: deleting the change re-opens exactly the bypass the docs claim closed.

**Corpus status:** `genuinely new`; complements N11/N3′ (signed-zero work) without reopening it — arithmetic paths already route through `normalized` (1123/1132/1141/1151, 728/855).

---

##### F4 — The namespace is mislabeled, but the seed's `Scanner` struct is the wrong remedy
`genuinely new` (disposition: **reject the type, keep only an optional micro-move**) · kind: namespace abuse analyzed and downsized · OBSERVED + corpus-adjudicated

**Evidence (from §2.1):** 11 of the 16 no-self fns are not `DurationValue` knowledge — 9 scanner/cursor helpers that are pure `(bytes, pos, input) → span` transforms, 2 parts-witness helpers that operate on `DurationParts`, plus `SUB_YEAR_DECOMPOSITION_UNITS` (130) and `canonical_raw` (380) that belong to the seconds→text synthesis policy. Only `parse`/`parse_prefix`/`can_start`/`from_seconds`/accessors are value-type operations. This is the structural fact behind messrust's LCOM4=2 / complexity 78 (03:171).

**Why a `Scanner<'a>` type is rejected (adversarial pass over the seed's own hypothesis):**
1. The 9 scanner helpers are **stateless** — they take `(bytes, pos)` and return spans; they need no struct.
2. The only stateful parts are the two loops' locals (`pos`, `parsed_any`, `is_negative`, `raw_parts`, `last_end`) — exactly what ticket 04 collapses into **one** parameterized `Whole | Prefix` loop (04:15).
3. Post-04, a `Scanner` struct would wrap ~5 locals used by a single function, with no second consumer and no test interface (the interface discipline forbids exposing it). DEEPENING's second-adapter test fails: one consumer = hypothetical structure, not a seam.
4. The corpus's own disposition is adjacent and must be engaged: 03:171–173 **already rejected the parse-machinery/value type split** ("splitting would fragment the owner without relocating knowledge"). A cursor type relocates *state*, not knowledge — and after 04 there is barely state left to relocate.

**What survives:** an optional **namespace partition** only — moving the 9 pure helpers (and possibly `apply_sign`/`fold_parts`) out of `impl DurationValue` to module-private free fns or an inline `mod scan`, so `DurationValue::` lists only value operations. Explicitly labeled **not deepening** per DEEPENING ("moving functions into one file without changing responsibility or seams is not deepening"): navigability only, zero caller change, zero test change. Verdict: do it only for free while 04 rewrites the loops; never standalone, never as a struct.

**Related residue that folds into 04's merge:** `apply_sign` (314) as a post-pass is removable — the sign is known when the first part is pushed (both loops call it at 209/277 after the loop; a push-with-sign deletes the pass). Marked `superseded-by-ticket-04`.

**Corpus status:** `genuinely new` as an analysis; the *type* is rejected partly on corpus grounds (03 no-split), the micro-move is optional.

---

##### F5 — Signed-zero normalization has three owners; two are identical copies
`genuinely new` · kind: duplicated policy across seams · OBSERVED

**Evidence**
- `fn normalize_zero` exists twice, byte-identical: `src/note/field.rs:468–474` and `src/query/sort.rs:445` (`if n == 0.0 { 0.0 } else { n }`).
- `git log -S` shows **both** introduced by `21ae8b1d fix(query): close signed-zero drift and doc/test gaps from review` — the commit that closed drift *duplicated the policy it was closing*. A third consumer sits at `note/field.rs:263` (`NoteFieldValueRef::compare` uses `normalize_zero` for `Number` arms).
- `DurationSeconds::normalized` (`duration.rs:1022`) is the third implementation — but it is a *construction invariant* for the newtype, a different responsibility from the raw-`f64` comparison idiom.

**Analysis:** the doctrine "total_cmp distinguishes `-0.0`; the domain treats zeros equal" now lives in three places, only one of which (`DurationSeconds`) is type-owned. N3′/N11 covered durations; the raw-`f64` copies in note/query are unowned.

**Direction:** one crate-level `normalize_zero` (or `core` helper) for the two raw-`f64` sites; keep `DurationSeconds::normalized` (construction ≠ comparison idiom). Record the doctrine once (08's CONTEXT/ADR surface already carries "canonical duration Eq"; this is its numeric sibling).

**Trade-off:** module-local tiny helpers are a defensible style; the counter is that this exact duplication was introduced *by a drift fix*.

**Corpus status:** `genuinely new`; outside duration.rs for the two copies, in-scope because `query/sort.rs` is a listed caller.

---

##### F6 — Trailing-separator `raw` spelling diverges by entry point
`genuinely new` · kind: policy with no owner (pin during 04) · OBSERVED

**Evidence**
- `parse` stores the trimmed *whole input*: `raw: trimmed.into()` at `duration.rs:214` → `parse("1h,")` ⇒ `raw = "1h,"`.
- `parse_prefix` stores the *consumed span*, trimmed: `raw = input[..last_end].trim().into()` at `duration.rs:281` → `parse_prefix("1h,")` ⇒ `raw = "1h"`.
- Same value, different `Display`, by constructor. Intra-`parse` asymmetry as well: trailing whitespace is stripped (`parse("1h ")` → `"1h"`, pinned by test ~1298) but trailing comma is kept.
- The only trailing-separator test, `accepts_trailing_separator` (`duration.rs:1307–1315`), asserts seconds only — `Display`/`raw` is unpinned.

**Analysis:** nobody decided what the canonical stored spelling of trailing separators is; two code paths give two answers, and since `raw` is display-only (spec:88 — witness never encoded in `raw`) nothing else constrains it. The loop merge in 04 is the moment to pick one (recommended: consumed-span, uniformly) and pin it.

**Corpus status:** `genuinely new`; attach as a pin to ticket 04 (which owns the merge), not a redesign.

---

##### F7 — Guard-fn visibility
`superseded-by-ticket-04` · OBSERVED

`can_start` (`duration.rs:499`) and `can_start_duration_segment` (504) are `pub(crate)` with one production caller each (sort.rs:487, and the internal probe). After 04's `classify` lands, both shrink to whatever protocol remains; privacy tightening belongs in 04's residue checklist. Noted so it is not lost — and the corrected census (§2.2) means the residue is *smaller* than the seed assumed: there is exactly one guarded site, not a guard protocol spread across lexer/sort/filter.

---

#### 5. Rejected candidates and negative findings (probes that failed)

| # | Candidate | Probe | Result |
| --- | --- | --- | --- |
| R1 | **Σ-fold dispersion** — struct-literal `Self { raw, seconds, parts }` duplicated at 215/280 looks like knowledge duplication | Count production fold sites | **No finding.** All four production paths call `fold_parts` (211, 279, 727, 854); the literals are construction, the fold is single-owned. |
| R2 | **Collapse `parsed_number`/`parsed_unit`** (one-to-one delegation, sole callers 620/671) | Deletion test | **Rejected.** They are the *scan vs validate/convert* halves: the error policy (`MissingUnit`, `UnknownUnit`) lives in the second half, and 03 records `parse_number` CC 17→14 / NPath 648→360 as an explicit audit remediation *achieved by* this extraction. Collapsing reverts the remediation. |
| R3 | **Merge `skip_separators`/`skip_whitespace`** (clone pair, differ by the `,` predicate) | Deletion + readability | **Rejected (micro).** A shared form hides the grammar predicate behind a bool (`skip(…, allow_comma)`) or a closure at call sites; ~20 lines; one call site each post-04 (183→merged loop, scan_part). No knowledge relocation. |
| R4 | **Wrap `DurationParts` in a struct** (alias at 913, ops at 314/333/727/854/461) | Second-adapter test | **Rejected.** Alias used once (line 120); no cross-module operations; a struct without a second consumer is structure, not a seam. |
| R5 | **`Scanner<'a>` extraction** | See F4 | **Rejected** — stateless helpers need no type; 04 collapses the only stateful code; 03 already rejected the adjacent type split; post-04 the struct would wrap one function's locals. |
| R6 | **Split registry into `unit.rs`** | Knowledge-relocation test | **Rejected.** Parse table + ratios + hint + docs are cohesive; callers already import one path (`lib.rs:101` re-exports); a file split is CONTEXT-map churn with zero seam change (echoes the corpus rule "directory splits only if size pushes — don't preempt"). |
| R7 | **Split/merge error enums** | Corpus check | Settled: merged `DurationError` stands; splitting re-fragments the owner. No new proposal. |
| R8 | **`Cow<'raw>` for the stored `raw`** | Corpus check | Settled rejected; `raw` is re-derivable from `seconds` where consumers need it (display-only, spec:88). |
| R9 | **`duration.rs:417` as a signed-zero violation** | Trace `total_secs` derivation | **Downgraded.** `canonical_raw` early-returns on `== 0.0` (383), so `-0.0` cannot reach 417; upstream finiteness holds. Survives only as the *visibility* finding F3. |
| R10 | **`TryFrom` delegation chain** (`TryFrom<DurationValue>` at 763 → one line into `TryFrom<DurationSeconds>` at ~1043) | Deletion test | **No finding.** One-line delegation to the finiteness owner; deleting it duplicates the error mapping. |
| R11 | **`SortKey::Duration(DurationSeconds)` as a leak** | Caller analysis | **No finding.** Sort holding the numeric key while `DurationValue` holds the witness is leverage, not leakage; `date.rs` peels via `to_seconds()` (212/603) because it holds `&DurationValue` and `DurationSeconds` is `Copy`. |
| R12 | **Exhaustive calendar matches at ~5 sites as duplication** | Corpus check | Settled intentional (03:174–180): `pat-exhaustive-enum` makes the compiler the policy owner; a shared predicate would bypass the gate. `rg class(` confirms no `DurationUnit::class()` exists. |
| R13 | **`"1h 1x"` parse-vs-prefix divergence (N8)** | Read both paths | **Not a divergence** — both reject; 04:15 pins both shapes. |
| R14 | **Inline date boundary asymmetry** (`is_atom_boundary` at `inline.rs:270` treats date boundaries differently from duration) | Trace fallback | **Low-confidence negative.** Outcome-equivalent because failed atoms fall back to the whole-source `String` path (`inline.rs:71–85`). Not promoted to a finding. |
| R15 | **Recursion / stack-depth probe on `parse`** | Trace call graph | **Not applicable** — `parse`, `parse_prefix`, `parse_atom_at` are iterative; no recursive descent. Gap closed. |
| R16 | **`is_calendar` as dead surface / census gap** | rg + corpus read | **Self-correction — withdrawn.** It is *not* a census gap: 03:164–169 lists it under "Deliberate remaining state (do not 'fix')" with `expect(dead_code)`, tests as first consumers, and 04's `classify`/query #05 as declared production consumers; 03:174–176 keeps it as one of the five intentional exhaustive matches. 09's removal set correctly omits anything pinned by 01–08. Residue reduced to a verification item: at 04, confirm `classify` actually consumes it (see §7). |

---

#### 7. Cross-ticket notes (items this review does not own)

1. **→ Ticket 04 (recognize-with-precision):**
   - Attach **F6** (pin trailing-separator `raw` spelling) and **F7** (guard-fn privacy) to the loop merge; **R5/F4's `apply_sign` post-pass removal** rides the same rewrite.
   - Verify **`classify` consumes `is_calendar`** (declared at 03:167–169) — if the trichotomy uses unit-kind matches instead, `is_calendar`'s status must be revisited *at that point*, not now.
   - Post-04 residue, correctly scoped: one guarded site (sort.rs:487) plus the field-coercion performance note (field.rs:531) — not a four-seam protocol as the seed framed it.
2. **→ Ticket 05 (query temporal functions):** declared consumer for `Add`/`Sub`/`Mul`, `apply`, `parts` — until it lands, those remain test-only (§3).
3. **→ Ticket 09 (dead-surface cleanup):** no change requested by this review; `from_seconds` retention honored (09:12), `is_calendar` correctly absent from its removal set (see R16).
4. **→ Ticket 08 (docs/register):** register candidates from this review — **F1** (comma precedence, if the note-parser decision chooses to record it as a divergence), **F5** (signed-zero as one cross-module doctrine).
5. **→ Ticket 03 (done):** adjudications honored; the only clarification is that its "Deliberate remaining state" list is doing real work in this review (it killed R16 outright).

---

### 12.3 Component report: corpus audit (`spec.md`, `review.md`, `issues/01–09`)

**Type:** research-only, adversarial cross-check of the design corpus against the current code.
**Baseline:** HEAD `5b7dc748` — `refactor(date,duration): move module consts to top`, 2026-10-04 20:16:13 +0300, clean worktree (`git status --porcelain` empty).
**Artifacts audited:** `spec.md` (150 L), `review.md` (264 L), `issues/01-hardening-pass.md` (99 L), `02-local-zone-clock.md` (255 L), `03-duration-semantics-owner.md` (189 L), `04-recognize-with-precision.md` (207 L), `05-query-temporal-functions.md` (17 L), `06-template-format-dialects.md` (18 L), `07-remaining-parity.md` (22 L), `08-docs-and-decisions.md` (17 L), `09-dead-surface-cleanup.md` (16 L).
**Code touched:** `src/date.rs`, `src/duration.rs`, `src/template/engine/date.rs`, `src/field.rs`, `src/query/sort.rs`, `src/query/value.rs`, `src/query/grammar/filter.rs`, `src/note/field.rs`, `src/note/parser/inline.rs`, `src/note/parser/lexer.rs`, `src/note/parser/task.rs`, `src/lib.rs`, `src/config/model.rs`.

**Evidence rules honoured:**
- Every line cite is marked **OBSERVED** (read at HEAD `5b7dc748`) or **PREDICTED**.
- Decisions A2′, B1+local-naive, D-b, D11, D12–D14, exhaustive-match repetition, no-`DurationUnit::class()`, no-`DurationValue`-split were **not** re-litigated. None was contradicted by code, so none is promoted to a Q3 finding.
- No file in the corpus or the codebase was modified by this audit.

---

#### 0. Line-drift root cause

Ticket 04's and ticket 09's line cites were **exact** at commit `bcc938b5` (2026-10-02 23:03). The drift has three stages:

| Commit      | Date                | Effect                                                                              |
| ----------- | ------------------- | ----------------------------------------------------------------------------------- |
| `bcc938b5`  | Oct 2, 23:03        | `refactor(temporal): apply design-audit remediation` — the state ticket 04 cites from |
| `1e3b439f`  | Oct 4, 15:48        | `chore: doc comment format` — shifted `date.rs` lines by ~3–9                         |
| `07c631ac`  | Oct 4, 16:03        | `docs(temporal): review and pin ticket 04 shape` — **the review pass; reuses `bcc938b5` numbers without re-verifying** |
| `0b87d374`  | Oct 4, 18:38        | further reorder                                                                      |
| `df8f99e1`  | Oct 4, 19:39        | `refactor` — bulk reorder of `date.rs`/`duration.rs`; bottom-half cites off by ~680   |
| `5b7dc748`  | Oct 4, 20:16        | `move module consts to top` — HEAD                                                   |

**Definitive proof the pin pass did not re-verify** (`git show 07c631ac:src/date.rs | grep -n …`):

| Symbol                        | At `07c631ac` | Ticket 04 cite | Delta at pin time |
| ----------------------------- | ------------- | -------------- | ----------------- |
| `has_four_digit_year`         | 302           | 305            | +3                |
| `is_iso_shape`                | 315           | 319            | +4                |
| `DateValue::parse_iso`        | 335           | 343            | +8                |
| `DateTimeValue::parse_iso`    | 575           | 585            | +10               |
| `to_datetime_string`          | 612           | 615            | +3                |
| `DatePoint` struct            | 1006          | (has_time 1022) | ~+9              |
| `duration.rs::from_seconds`   | 269           | (09: 217)      | −52               |

So ticket 04's cites were **already 3–10 lines wrong the moment they were "pinned"**, and are now wrong by 92–256 lines (table in §Q1a). Ticket 09's `duration.rs:217` was correct at `22ee6743` (Sep 27, review era) and has never been correct since `2e4304d2`.

**Contrast:** ticket 03's own comment (03 L136–139) explicitly says *"checklist line numbers above predate the remediations and are stale; these symbols are the current pointers"*. That is the pattern tickets 04/09 and `review.md` failed to follow.

**Secondary note:** commit `cdc3f566` referenced in the audit brief does not exist — `git log --all | grep cdc3f566` exits 1.

---

#### Q1 — Stale or false code-fact claims

##### Q1a. Ticket 04 — line cites (all stale; all underlying facts hold)

| #   | Cite in ticket 04                                     | OBSERVED at HEAD `5b7dc748`                                                                                                            | Verdict                                                                                       |
| --- | ----------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| 1   | `DatePoint::has_time` `src/date.rs:1022` (item 17)        | struct `date.rs:922`, field `date.rs:929`                                                                                              | **stale −93**                                                                                    |
| 2   | `diff`'s instant/civil split `src/date.rs:1062` (item 17) | `diff` fn `date.rs:965`, `let both_datetimes = self.has_time && to.has_time;` `date.rs:970`                                            | **stale −92**                                                                                    |
| 3   | `DateValue::parse_iso` cascade `src/date.rs:343` (item 14) | fn `date.rs:87`; cascade loop `date.rs:94` — `let [first, rest @ ..] = DateFormat::ALL;`                                              | **stale −256**                                                                                   |
| 4   | `DateTimeValue::parse_iso` cascade `src/date.rs:585` (item 14) | fn `date.rs:400`; cascade loop `date.rs:407` — `let [first, rest @ ..] = DateTimeFormat::ALL;` | **stale −178**; `date.rs:585` is now a `#[cfg_attr(not(test), expect(dead_code))]` on `DateTimeValue::apply` — a misleading collision |
| 5   | `has_four_digit_year` `src/date.rs:305` (item 14)        | fn `date.rs:115`; scan `date.rs:117` — `bytes.get(0..4).is_some_and(\|b\| b.iter().all(u8::is_ascii_digit))`                          | **stale −190**; the "two inline 4-digit copies" *fact holds*                                     |
| 6   | `is_iso_shape` `src/date.rs:319` (item 14)               | fn `date.rs:127`; scan `date.rs:130`                                                                                                   | **stale −192**; same                                                                           |
| 7   | `to_datetime_string` `src/date.rs:615` (item 18)         | `date.rs:436`                                                                                                                          | **stale −179**                                                                                 |

**Ticket 04 claims that HOLD at HEAD (verified):**

- `src/field.rs:825` — `DateTimeCoercion` classify cascade (body `829–837`; enum defined `field.rs:790`).
- `src/query/sort.rs:477` — `TextShape::classify` (call to `can_start` at `sort.rs:487`).
- `src/template/engine/date.rs:210` — `ParsedDate::parse`.
- `engine:235` — chains `DateError` as source via `invalid_operation(format!("invalid date {s:?}"), datetime_source)` (`engine:235–238`); N4 closed by ticket 02, as 04 item 12 states. (`Err(_date_source)` at `engine:235` is a deliberate, comment-documented discard of the date-specific source.)
- `engine:480` — `match precision` on `DatePrecision`, two arms, **no wildcard** ✓ (`engine:469–491`).
- `date_add` docs `engine:399–417` — week/calendar-vs-instant clause present (observed within `405–408`) ✓.
- `src/note/parser/inline.rs:219` — `self.source.advance(pos, 10)` ✓; `inline.rs:224` — `DateValue::parse_iso(date).ok()?` ✓; `inline.rs:221` — `DateValue::is_iso_shape(date)` (an N15 `guard && parse` site the AC covers).
- `src/note/parser/lexer.rs:247` — `after_ws.get(..ISO_DATE_LEN)`; `ISO_DATE_LEN: usize = 10` at `lexer.rs:72`; `lexer.rs:250` `is_iso_shape`, `lexer.rs:253` `parse_iso` (another `guard && parse` site).
- `src/note/field.rs:115` — `.and_then(|prefix| DateValue::parse_iso(prefix).ok())` ✓.
- **Exactly 3** datetime→date coercion cascades: `field.rs:832/835`, `sort.rs:480/483`, `engine:211/224` ✓.
- **Exactly 2** format-cascade loops: `date.rs:94`, `date.rs:407` ✓ (review §2.2 said four; 04's amendment is correct).
- **Exactly 2** inline 4-digit scans: `date.rs:117`, `date.rs:130` ✓.
- "The engine calls `parse_iso` rather than looping" ✓ — `engine:211`, `engine:224`, no `for … in …Format::ALL` in the engine.
- `date.rs:89` / `date.rs:402` — the two cross-domain four-digit guards inside `DateValue::parse_iso` / `DateTimeValue::parse_iso`, each delegating to `DateValue::has_four_digit_year`.

**Ticket 04 claims that are now stale:** every line cite above (items 9/14/17/18), and by implication item 21's "intra-doc links resolve after the engine deletions" is unaffected.

##### Q1b. Ticket 06 — premise is FALSE (highest-severity stale fact)

- `DateValue::format_with` and `DateTimeValue::format_with` were **deleted** by ticket 03's `bcc938b5` remediation. Ticket 03's own comment (03 L105–108): *"Deleted the seven test-only dead methods (`DateTimeValue::{start_of_day, to_offset_string, to_time_string, to_date_string, cmp_date, format_with}`, `DateValue::format_with`) and their tests: seven `expect(dead_code)` suppressions eliminated."*
- OBSERVED: the **only** `format_with` in the crate is the engine-private `src/template/engine/date.rs:304` (six live callers: `format_precise` `engine:335`, `date_format` `engine:365`, and the remaining filter entry points). `grep -n "fn format_with" src/date.rs` → no match.
- Consequences:
  - review **N21** ("invalid-pattern detection exists 3× at `date.rs:255`, `date.rs:463`, engine `date.rs:255`") → now **1×**. (Those exact lines are confirmed deleted: at `22ee6743` they were `255` and `463` — so N21's cites *were* correct then.)
  - ticket 06 item 2 — *"`format_with` has a live consumer (first Decision-C wiring)"* — **moot**; there is no `format_with` left to wire.
  - ticket 06 item 15 — *"core `format_with` keeps strftime as its only grammar"* — **moot**.
  - spec **L102** — *"each call site maps it to its own type (template vs core)"* — only the template call site remains.
  - spec **L103** — *"Core `format_with` keeps strftime as its only grammar"* — **false today**.
  - spec **L111** / review **§9 S4** / **§10 step 6** all name a surface that no longer exists.
  - review **§5.6** — *"N21: private `render_pattern(display, pattern) -> Result<String, ()>` in `date.rs`; three call sites map its error to their own type"* — the deduplication was achieved by **deletion**, not by the specified shared renderer.
- **Only live residue:** `engine/date.rs:304` is now the single invalid-pattern renderer. Ticket 06's real remaining work is `durationformat` (item 1) + the moment translator (items 15/16) + the format-binding docs (items 13–14).

##### Q1c. Ticket 09 — 5 of 6 named methods are already gone; census incomplete

Ticket 09 item 1's "the set" (09 L11) vs OBSERVED at HEAD:

| Named in ticket 09                                  | State at HEAD                                                                                                                                                                    |
| --------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `to_time_string`                                    | **deleted** by 03                                                                                                                                                                |
| `start_of_day`                                      | **deleted** by 03                                                                                                                                                                |
| `cmp_date`                                          | **deleted** by 03                                                                                                                                                                |
| `checked_add` / `checked_sub` residue               | **deleted** by 03 ("superseded by `apply`"); `grep -n "fn checked_add\|fn checked_sub" src/` → **0 hits**                                                                            |
| `to_date_string` on `DateTimeValue`                 | **deleted** by 03 (`DateValue::to_date_string` survives at `date.rs:142`, live caller `src/template/engine/query.rs:604` — 09's scoping of it as out-of-scope is correct)           |
| `to_offset_string` (09 item 13)                     | **deleted** by 03. `grep -rn "to_rfc3339()" src/` → **0 hits**, so *"today the only `to_rfc3339()` site is inside `to_offset_string`"* is false. The `…Z` channel already has its producer: `date.rs:771` `to_rfc3339_opts(SecondsFormat::Secs, true)` inside `impl Serialize for DateTimeValue`. Item 13's *"mandatory, the only allowed outcome"* is already satisfied by a different route |
| `from_seconds` `expect(dead_code)` at `duration.rs:217` (09 items 2, 15) | **fact holds, line stale** — attr now `duration.rs:353`, fn `duration.rs:359`. (`217` was correct at `22ee6743`)                                             |

**Incomplete census.** OBSERVED `#[cfg_attr(not(test), expect(dead_code))]` in the cluster = **6**, and ticket 09 names only one of them:

| Site            | Symbol                    | Listed in ticket 09?                                   |
| --------------- | ------------------------- | ------------------------------------------------------ |
| `date.rs:165`     | `DateValue::shift`          | **no**                                                    |
| `date.rs:192`     | `DateValue::apply`          | **no**                                                    |
| `date.rs:585`     | `DateTimeValue::apply`      | **no**                                                    |
| `duration.rs:353` | `DurationValue::from_seconds` | yes (item 2)                                            |
| `duration.rs:434` | `DurationValue::parts`      | **no**                                                    |
| `duration.rs:454` | `DurationValue::is_calendar`| **no**                                                    |

(`grep -c "dead_code"` → `date.rs` 3, `duration.rs` 3, `engine/date.rs` 0.)

Ticket 03's handoff (03 L162–169) declares **all six** "spec-mandated surface with tests as first consumers". Ticket 09's *"recorded decision per method, not a blanket sweep"* cannot produce a decision for a method it does not enumerate. Item 15's *"expected remainder: `from_seconds`, `to_offset_string`"* is wrong on both counts.

**Also false in ticket 09:** item 13's re-target mandate (above), and by extension review §10 step 6's `to_offset_string`→serial? and `checked_add/sub`→superseded entries.

##### Q1d. `review.md` — evidence column and forward-looking sections

###### §1 Seam landscape (rows 1–3 stale)

| Row claim                                                                                | OBSERVED                                                                                                                                                                                                                        |
| ---------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| "Recognition/parsing … **derived `Deserialize` bypasses it (N12)**"                        | fixed by ticket 01: manual `Deserialize` for `DateTimeValue` `date.rs:778`, for `DurationValue` `duration.rs:901`                                                                                                                   |
| "Calendar arithmetic — **no owner**; adapters `engine date_shift_unit`/`date_diff` (copy), dead `checked_add`/`checked_sub`; missing: query-side arithmetic, `weekday`, shorthands — **all blocked on S1**" | owner landed in ticket 03 (`DatePoint`, `shift_wall`, `DateTimeValue::shift`, both `apply_part`s); `checked_add`/`checked_sub` deleted; engine `date_shift_unit` `engine:469` now delegates                                                                 |
| "Clock — **no doctrine**; `Local::now` ×4, `Utc::now` ×3 (engine)"                          | `grep -rn "Local::now" src/` → **0 hits**. Production clock reads: `engine:96` (`local_now()` = `DateTimeValue::from(Utc::now()).wall_or_utc()`), `engine:659`, `engine:669` (before/after for `is_future`/`is_past`). `date.rs:381` `Utc::now` is behind `#[cfg(test)]` at `date.rs:377` |
| "Formatting — `date.rs` (`format_with`, `Display`)"                                       | `format_with` deleted from `date.rs` (see Q1b)                                                                                                                                                                                   |
| "Precision — engine-private `DatePrecision`/`ParsedDate` → S7"                            | **still true** (04 open) — row accurate                                                                                                                                                                                     |
| "Unit registry — **4th silent ratio copy (N18)**"                                          | ms-table deleted by 03; `fixed_seconds_i64` now derived from `fixed_seconds()` (03 L17)                                                                                                                                          |

###### §2.1 Correctness evidence column

| Finding | Cited evidence       | OBSERVED at HEAD                                                                                                                                        | Verdict                     |
| ------- | -------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------- |
| N1      | `engine date.rs:432` | `chrono::Duration::try_seconds` now `date.rs:509`, `date.rs:1125`, `date.rs:1184`                                                                          | cite stale, **issue fixed** |
| N3′     | `duration.rs:111–121, 717–747` | `DurationSeconds` `PartialEq` `duration.rs:1068`, `Ord` `duration.rs:1085`, `Display` `duration.rs:1095`; `717–747` is now `combine`               | cite stale                  |
| N4      | `engine date.rs:183–201` | `ParsedDate::parse` `engine:210`; source-chain attach `engine:235–238`                                                                                 | cite stale, **issue fixed** |
| N7      | `note/field.rs:525`  | arm at `note/field.rs:525`; `DurationValue::parse(trimmed)` at `note/field.rs:531` (no `can_start` guard)                                                 | holds (arm boundary)        |
| N14     | `duration.rs:224–269`| `from_seconds` now `duration.rs:359`; test `from_seconds_never_lies_about_a_nonzero_magnitude` `duration.rs:1707`                                          | cite stale, **issue fixed** |
| N15b    | `duration.rs:45–47`  | `UNIT_HINT` now `duration.rs:44–46`; sign-rule owner `digits_after_sign` `duration.rs:530`                                                                 | cite stale                  |
| N19     | `duration.rs:737–741`| `Display` now `duration.rs:1095–1107`, exponent dialect over `DISPLAY_EXPONENT_UPPER`/`DISPLAY_EXPONENT_LOWER`                                            | cite stale, **issue fixed** |
| D9      | `engine date.rs:92–117, 699, 709` | `engine:96`, `engine:659`, `engine:669`                                                                                                          | cite stale, **issue fixed** |
| D3      | `engine date.rs:282` | `format_precise` now `engine:331`; body = `format_with(dt.format(precision.format()), precision.format())`, both patterns are `DEFAULT_DATE_FORMAT`/`DEFAULT_DATETIME_FORMAT`, **no subsecond specifier** | cite stale, **issue still live** (correctly deferred to 04 item 18) |

###### §2.2 Interface coherence

| Finding | Cited evidence                       | OBSERVED                                                                                                                             | Verdict             |
| ------- | ------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------ | ------------------- |
| N8      | "parse/parse_prefix hand-copied"     | both share `scan_part`; entries `duration.rs:169` / `duration.rs:233` — two wrappers remain (04 item 15 will merge)                   | holds               |
| N12     | "`duration.rs:561` precedent"        | manual `Deserialize` now `duration.rs:901`, `date.rs:778`                                                                              | cite stale, fixed   |
| N15     | "4 guard sites"                      | `date.rs:115`, `date.rs:127` (defs), callers `date.rs:89`, `date.rs:402`, `inline.rs:221`, `lexer.rs:250`, `duration.rs:499` (`can_start`) | holds, cite implicit |
| N17     | exports                              | `src/lib.rs:93` `pub use date::{DateError, DateTimeValue, DateValue};`, `lib.rs:100` `pub use duration::{DurationError, DurationValue};`  | **fixed by 01**     |
| N18     | "`duration.rs:236–243` ms-table"      | **deleted** by 03; greedy units rebuilt from `SUB_YEAR_DECOMPOSITION_UNITS` (`duration.rs:130`) derived from `fixed_seconds()`          | issue **fixed**     |
| N20     | cascade loop                         | `date.rs:94`, `date.rs:407` — two, not four                                                                                          | partially stale     |
| N21     | `date.rs:255`, `date.rs:463`, engine `date.rs:255` | **two of three deleted**; engine-only remains (`engine:304`)                                                             | **premise broken**  |
| N23     | `From<FieldValueRef>` policy         | `note/field.rs:509–536`; duration step `note/field.rs:531`; no `can_start` guard                                                       | holds               |
| D11     | "`date_add` docs promise six units"  | docs `engine:399–417` now name weeks/ms/abbreviations                                                                                 | **stale**; residual gap = "any spelling" wording + sub-day no-op caveat (04 item 19) |

###### §2.3 Structural / depth debt

- "8 methods behind `expect(dead_code)`" → OBSERVED **6**, different composition (table in Q1c).
- "Dead bridge: `checked_add`/`checked_sub` unusable until A2′ …" → **deleted by 03**.
- D7 (`into_inner`/`From` demoted) — still live: `date.rs:687`, `date.rs:741`, `note/field.rs:111–112`, `engine:213/226/230`. 09 item 14 correctly re-runs the deletion test.
- H3/H4/D10 — still accurate (04 open).

###### §4 Decisions

- "Also fixed (carried): `DEFAULT_DATE_FORMAT` removal from `config/model.rs:49` → substitute `crate::date::DEFAULT_DATETIME_FORMAT`" — **not done**: `src/config/model.rs:52` still holds `const DEFAULT_DATE_FORMAT: &str = "%Y-%m-%dT%H:%M:%S";`, used at `config/model.rs:814` and `config/model.rs:837`. (The *value-identity* claim is correct: config's const value equals `date.rs:51` `DEFAULT_DATETIME_FORMAT`, and differs from `date.rs:48` `DEFAULT_DATE_FORMAT = "%Y-%m-%d"` — three same-named/different-valued constants across two modules, which is exactly the trap story 39 describes.)
- A2′ / B1 / D-b / D11 decision *text* — consistent with the spec; not re-litigated.

###### §6 Principles audit

- DRY row counts stale: "×4 cascades (D1/N20)" → **×2 loops + ×3 coercion cascades**; "×4 scans (D6)" → **×2**; "×4 ratio tables (N18)" → **1** (+ derived `fixed_seconds_i64`); "×3 format-with (N21)" → **1**; "×2 grammars (N8)" → holds; "×4 guard sites (N15)" → holds-ish.
- Open/Closed row "❌ engine unit-match (per-unit arms + shortcuts)" — largely closed by 03: `date_shift_unit` now matches **precision** (2 arms), not units; unit dispatch lives in `shift_wall`/`DateTimeValue::shift`.
- Law of Demeter row "chrono round-trips in `ParsedDate` — one call each after S5/S1" — still live, 04 open.

###### §9 Deepening plan + §10 Execution order

- §9 rows **S1, S2, S6, S10** and §10 **steps 2–3, 5** read in future tense although tickets 01/02/03 are `resolved`. §10 step 6 still lists `to_time_string`, `start_of_day`, `cmp_date`, `to_offset_string`→serial?, `checked_add/sub` — all deleted.
- §4 is headed *"Also fixed (carried)"* yet contains an item (config const) that is not fixed (08 item 14).
- review L5 states *"No implementation performed"* — contradicted by tickets 01 (`c990f6c9`), 02 (`90696f25`), 03 (`57ea1b09` → `bcc938b5`).
- §9 **S4** = "`durationformat` + wire `format_with` + shared `render_pattern`" → half of it has no referent (Q1b).

##### Q1e. `spec.md`

| Line  | Claim                                                                                                                              | OBSERVED                                                                                          |
| ----- | ---------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------- |
| L5–9  | Problem Statement: "silently panics on out-of-range durations … dead fixed-magnitude bridge (`month` = 30 days) … 301-digit number" | all fixed by 01/03; reads as current                                                             |
| L75   | "Query temporal features are `FilterFunction` registry entries only — **never new operators or grammar**"                             | **false as a route to stories 1–3** — see C1                                                       |
| L102  | "One shared renderer … each call site maps it to its own error type (template vs core)"                                              | only the template call site survives (Q1b)                                                        |
| L103  | "Core `format_with` keeps strftime as its only grammar"                                                                            | surface deleted (Q1b)                                                                             |
| L111  | Decision-C list                                                                                                                    | 5 of 7 items already executed by 03; `to_offset_string` outcome already resolved via `Serialize`  |
| L42 / L67 | stories 16/41: RFC3339 `…Z` "reserved for explicit interop (including `Serialize`)"                                             | producer exists (`date.rs:771`) but **no public interop accessor survives** (`to_offset_string` deleted) |
| L47 / L9 | story 21 / Problem Statement: "`DurationSeconds: Display` renders a 301-digit number … has no non-test caller yet"           | **fixed** (`duration.rs:1095`), and its "no non-test caller" premise is now doubly true (09 item 15 says count → 0)  |
| L56 / L111 | story 30 / Decision-C: "`checked_add`/`checked_sub` either calendar-aware or gone"                                            | **gone** (03)                                                                                      |
| L141–146 | Execution order                                                                                                                 | tickets do *not* all encode the edges — see C5                                                     |

##### Q1f. Stale doc comments in code

- **`src/query/grammar/filter.rs:217–218`** — `Eq`/`Ne` doc: *"`Eq`/`Ne` use `is_equal_to_literal`'s existing cross-kind coercion (e.g. a `Date` field against a `DateTime` literal **at midnight UTC`**.)"*. OBSERVED `DateTimeValue::is_equal_to_date` (`date.rs:681–683`) = `self.0 == Self::from(date).0`, and `From<DateValue> for DateTimeValue` (`date.rs:741–749`) resolves **local** midnight via `local_naive_to_utc(midnight)`. Stale since ticket 02.
- **`src/template/engine/date.rs:462–463`** — *"A date-only input stays civil for every unit: a zone-free date has no instant to shift."* True for the wall frame, but sub-day units on date-only inputs render as a no-op through `%Y-%m-%d` (the D3 caveat 04 owns, spec L53/L99). Wording predates 04's planned caveat.
- **`src/note/field.rs:505–508`** — `From<FieldValueRef>` rustdoc describes null→link→duration; consistent with code (`note/field.rs:513–536`), no issue.

##### Q1g. Ticket 05 — factual (not just logical) claims that are false today

| Claim (05 L3, L9, L11–13)              | OBSERVED                                                                                                                                                                                          |
| -------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| "`dur(\"1 month\")`" in a filter        | `grep -rn "dur(" src/query/` → **0 hits**. `dur(` lexes as `Ident` + `LParen` → `parse_function_call` → expects an Ident field path, receives a Literal → **syntax error**                             |
| "`date_add` / `date_diff` / `date_component` work" | `grep -rn "date_add\|date_diff\|date_component" src/query/` → **0 hits**                                                                                                                 |
| "Duration arithmetic (`+`, `-`, `* number`) available to queries" | no `Plus`/`Minus`/`Star` tokens anywhere in `src/query/grammar/`                                                                                                                     |
| "all as registry entries, no new operators or grammar" | the *single* registry entry is `Contains`; see C1/G1 for why the rest is unreachable                                                                                                 |
| "the calendar owner is 03, reached via 04" | dependency direction misstated — see C6                                                                                                                                                |
| "bucketing helpers … are ticket 07's deliverable per spec story 4" | consistent with 07 items 11–12, but contradicts review §9 S9's seam column — see C8                                                                                              |

---

#### Q2 — Design gaps not covered or adjudicated anywhere in the corpus

Ranked by exposure. Each item states (a) the OBSERVED evidence, (b) why no existing ticket/S-item covers it.

##### G1 — The filter expression grammar cannot host ticket 05; no S-item owns the extension *(highest)*

OBSERVED in `src/query/grammar/filter.rs`:

- `enum FilterFunction` has **exactly one** variant, `Contains { field, target }` — `filter.rs:111–120`. `FilterFunction::build` (`filter.rs:124–137`) accepts `name.eq_ignore_ascii_case("contains")` only, and the error message hard-codes the expected name: `QuerySyntaxError::unexpected_end(…, "`contains`")` at `filter.rs:346–354`.
- `FilterAtom` = `Comparison(ComparisonExpr) | Function(FilterFunction)` — `filter.rs:96`. **Both are predicates (→ bool). There is no value-producing expression node.**
- `parse_comparison` (`filter.rs:358–377`) = `field_path <op> literal` only.
- `parse_function_call` (`filter.rs:293–355`) = `name ( field_path , literal )` only — **no two-field-path args, no nested calls, no function-as-value, no optional/trailing args.**
- `parse_literal_arg` (`filter.rs:274–290`) accepts only `FilterToken::Literal` → `dur("1 month")` is a **syntax error** today.
- `control()` (`filter.rs:384–397`) returns `None` for `FilterToken::Op` — comparison operators are consumed *inside* an atom, never as control flow between atoms.
- `grep -rn "Plus\|Minus\|Star\|Token::Add\|binary" src/query/grammar/` → only a test-name match for `TaskField`.
- `CompareOp` (`filter.rs:201–214`) is `Eq/Ne/Lt/Le/Gt/Ge` — no arithmetic.

**Unreachable without grammar work:** value-producing expressions (`date − date` → duration), two-field args (`date_diff(a, b)`), nested calls (`dur("…")`), function results as comparison operands (`date_add(…) > x`).

**Why nobody covers it:** review §3 B17 prescribes *"`FilterFunction` entries (`filter.rs:116–125`), never operators"* — but those lines are the `Contains` **predicate** variant, and a predicate enum cannot yield a duration. Review §9 S3 says *"Seam: `FilterFunction` entries only"*. Spec L75 forbids grammar. Ticket 05 inherits all three. See **C1**.

##### G2 — Unit *spelling* vocabulary has four owners, none derived, none cross-pinned

Review N18 / S2 covered **ratios** only ("exactly one unit-ratio table"). Spellings are a separate table class and are uncovered:

1. `UNIT_MAP` — 35 keys, `duration.rs:53–89` (`phf::Map<&'static str, DurationUnit>`).
2. `UNIT_HINT` — `duration.rs:44–46`. Its own doc (`duration.rs:41–43`) says: *"Kept beside [`UNIT_MAP`] **as a reminder to update both** when a unit or spelling family is added to the registry."* — a **prose reminder, no test**. `grep -rn "UNIT_HINT" src/` → def (`duration.rs:44`), `lib.rs:101` re-export, `engine:46`, `engine:756` (doc), `engine:764` (`format!("unknown unit {unit:?} (expected {UNIT_HINT})")`). **No test references it.**
3. `SUB_YEAR_DECOMPOSITION_UNITS: [(DurationUnit, &str); 6]` — `duration.rs:130`, literal `&str` suffix column consumed by `canonical_raw` at `duration.rs:394`.
4. `#[case::]` mirror in `parses_all_units_and_abbreviations` — OBSERVED **35 cases vs 35 `UNIT_MAP` keys, exact set equality** (verified programmatically: `in map not in test: []`, `in test not in map: []`) — but the case list is **hand-maintained**, not generated from `UNIT_MAP`.

**The corpus has already been bitten by this exact failure mode.** Ticket 03's remediation (03 L117–119): *"`UNIT_HINT` lives beside the unit registry and feeds the engine's unknown-unit message (**ends the drift that hid weeks**)."* The fix removed the *symptom* (one stale message), not the *class* (four unsynchronised copies).

**Recommendation:** a derived test asserting every `UNIT_MAP` family appears in `UNIT_HINT`, and a parse-case list iterated from `UNIT_MAP.entries()` (precedent already exists at `duration.rs:2136–2139`: `fixed_seconds_are_positive_for_all_variants` iterates `UNIT_MAP.entries()`).

##### G3 — A fourth 10-byte date gate is missing from ticket 04's grammar-widening inventory

Ticket 04 item 16 (04 L16) names *"the 10-byte gates `src/note/parser/inline.rs:219` (`advance(pos, 10)`) and `src/note/parser/lexer.rs:247` (`ISO_DATE_LEN`)"*.

OBSERVED a **third**:

```text
src/note/field.rs:113–116
    Self::String(s) => s
        .get(..10)
        .and_then(|prefix| DateValue::parse_iso(prefix).ok())
        .map(DateValue::into_inner),
```

- Ticket 04 item 12 names `note/field.rs:115` **only as an `.ok()` discard**, not as a grammar gate; item 16 does not name it at all.
- Production caller: `src/note/parser/task.rs:212` — `.find_map(|val| val.as_date().map(Into::into))`, i.e. **task date fields sourced from a String value**.
- After 04 widens inline + lexer to the 7-byte `YYYY-MM` shape, this path still does `.get(..10)` on a 7-char string → `None`. Story 15 ("inline dates and YAML-deserialized dates parse through the same grammar … `YYYY-MM` accepted") is left **half-applied**.
- It is also a hidden `guard && parse` site (the N15 idiom), which 04 item 13's AC (*"No production call site … combines a shape guard with `parse`"*) reaches only implicitly.

Note: the *frontmatter* date path is separately gated at `lexer.rs:247` (04 covers it), so the exposure is narrowed to the String→`NaiveDate` helper — but that helper is what `task.rs` uses.

##### G4 — `From<DateValue> for DateTimeValue` silently shifts on tz failure; absent from the divergence register

OBSERVED `src/date.rs:741–749`:

```rust
impl From<DateValue> for DateTimeValue {
    fn from(date: DateValue) -> Self {
        let midnight = date.0.and_time(NaiveTime::MIN);
        Self(local_naive_to_utc(midnight)
            .unwrap_or_else(|_zone_failure| midnight.and_utc()))
    }
}
```

Rustdoc at `date.rs:738–740` declares it: *"A lookup failure falls back to UTC midnight (chrono does the same for a broken zone); the crate's fallible entry points surface it as [`DateError::LocalZoneLookup`] instead."*

- **Reachable from filter equality:** `NoteFieldValueRef::is_equal_to_literal` (`note/field.rs:366`) → `DateTimeValue::is_equal_to_date` (`date.rs:681–683`) → `Self::from(date)`. So in a broken-tz environment, `Date == DateTime` compares at **UTC** midnight while every other path returns `DateError::LocalZoneLookup`.
- Spec **L98 / story 13**: *"None caused by a tz-data/OS error surfaces as an error, **never a silent shift**"*.
- Ticket 08's divergence register (08 item 13 = exact mirror of story 42) lists 17 entries and **does not include this**.

**Options (needs a maintainer call):** register it as divergence #18, or replace `From` with `TryFrom` so the equality path can degrade explicitly. The rustdoc's *"chrono does the same"* defence is a parity argument, not a spec argument — spec D14 is absolute.

##### G5 — `DatePoint::new(wall, instant, has_time)` — ticket 03's "unrepresentable" overclaims

OBSERVED `date.rs:937–947`:

```rust
pub(crate) const fn new(wall: NaiveDateTime, instant: DateTime<Utc>, has_time: bool) -> Self
```

`has_time` is a free `bool` (field `date.rs:929`). A `DatePoint { wall: 10:30, instant, has_time: false }` is constructible — i.e. a **stale/inconsistent precision flag is representable**.

- Ticket 03's remediation (03 L97–101): *"`DatePoint` now carries `has_time` and owns the measurement (`DatePoint::diff`) … the engine's `ParsedDate` holds a `DatePoint` directly, so **a stale precision flag is unrepresentable**."*
- Call sites: production `engine:218` (`DatePoint::new(wall, instant, true)`), `engine:232` (`DatePoint::new(wall, instant, false)`); tests `date.rs:1984`, `date.rs:2007`, `date.rs:2013`. Both production sites derive correctly — so the *invariant holds by convention*, not by construction.
- Ticket 04 item 17 makes `has_time` **derive** from the hoisted `Precision`, and 04 L161 says *"no second precision signal remains"* — but unless `DatePoint::new` takes `Precision` (or is replaced by `from_recognized`), the constructor still accepts an independent bool and the overclaim survives 04.
- Related: review §5.4 says *"not two bools"* — the design intent is one signal; the constructor doesn't enforce it.

##### G6 — The calendar owner exposes two entry shapes; the engine bypasses the specified one

OBSERVED:

- `shift_wall` is `pub(crate)` with **exactly one non-self caller**: `engine:482` `shift_wall(parsed.point.wall, n, unit).map_err(date_error)?`.
- `DateValue::shift` (`date.rs:165`) carries `expect(dead_code)` — **zero production callers**.
- So for date-only precision the engine reaches *past* the typed value interface into the wall-level core, while the method ticket 03 actually specified ("`shift(base, n, unit)` … interface **on `DateValue`/`DateTimeValue`**", review §9 S1) is exercised only by tests.

This conflicts with:

- review §6 target graph *"note/query/template → date.rs|duration.rs → chrono"*;
- review §9 S1's seam column *"interface on `DateValue`/`DateTimeValue`"*;
- review §6 Information hiding / Law of Demeter rows;
- ticket 03's own handoff (03 L152) calling `shift_wall` *"the civil per-unit core"* without justifying its `pub(crate)` visibility.

Also relevant: 03's residual-CRAP note (03 L182–184) gives `shift_wall` 15.1 — an argument for **shrinking** its caller set rather than keeping a second entry.

**Fix shape:** `DatePrecision::Date` arm calls `DateValue::shift(midnight, n, unit)`, then `.local_wall()`; `shift_wall` becomes module-private. No S-item and no ticket names this (09's `proj-pub-crate-internal` rule is not pointed at it).

##### G7 — Ticket 03's `apply` / `parts` production consumer may never materialise

- 03 handoff (03 L164–169) declares the declared consumers: *"the query temporal functions (ticket 04's `classify`, **index-query#05**) are the declared production consumers"* for `DurationValue::parts`, `is_calendar`, `from_seconds`, `DateValue::shift`/`apply`, `DateTimeValue::apply`.
- Ticket 05's checklist (items 1–6) **never mentions multi-part durations or `apply`**; Dataview's headline `date_add(date, amount, unit)` is single-unit.
- The template engine never calls `apply`: `grep -rn "\.apply(" src/template/ src/index/ src/query/` → only `index/service.rs` (`pending.apply(dimensions)`) and `query/plan.rs` (`op.apply(rows)`), both unrelated types.
- If 05 lands single-unit only, `DateValue::apply` (`date.rs:192`) and `DateTimeValue::apply` (`date.rs:585`) stay dead and **ticket 09's deletion test will strike spec-mandated surface that ticket 03's AC (`apply` applies parts L→R, spec L123) pins**.

**Needs a decision before 09, not during it:** either 05 wires a multi-part duration path (`date_add(due, "1d 1h")`-shaped), or 09 is told explicitly that `apply`/`parts`/`is_calendar` are exempt, or the ACs are amended.

##### G8 — Minor: where the `YearMonth` strftime pattern lives after 04

- `DatePrecision::format()` (`engine:176–181`) returns `DEFAULT_DATE_FORMAT` (`%Y-%m-%d`, `date.rs:48`) or `DEFAULT_DATETIME_FORMAT` (`%Y-%m-%dT%H:%M:%S`, `date.rs:51`).
- 04 item 17 deletes `DatePrecision`/`ParsedDate`/`format_precise` and adds a `YearMonth` arm to `match precision` (`engine:480`).
- **Neither 04 nor the spec names `"%Y-%m"` or where that pattern lives.**
- PREDICTED: it belongs on the hoisted `Precision` (an unstated interface decision); and once `ParsedDate` is deleted, `date_shift_unit` must recover `wall` from the value — which *is* derivable (`DateValue` → midnight; `DateTimeValue` → `local_wall()`; `YearMonth` → day-1 midnight) but 04 never says so, which is why the deletion looks riskier than it is.

##### G9 — Smaller, filed for completeness

- **`DurationUnit::parse` returns `Option`** (`duration.rs:946`) while `DurationValue: FromStr` returns `Result` — two error idioms on adjacent types; `conv-tryfrom-fallible` is named in the corpus but not pointed here. *(weak; may be deliberate)*
- **`is_calendar()` has no declared production consumer that 03's own handoff sanctions** — see C9.
- **`as_date`'s `DateTime` arm** (`note/field.rs:112`) silently truncates to a date — no divergence-register entry, though story 42 is exhaustive about other coercions. *(weak)*

**Considered and rejected as Q2 items (already covered or adjudicated):**
- mega-classifier — review §5.3 explicitly rejected, spec L78;
- `Clock`/`TimeZone`/`Dialect` traits, typestate builders, merged error enums, `Cow<'a, str>` — spec L78 / review §5.7;
- `date/` directory split — spec L135 (revisit only if size pushes it);
- null ordering, strictness-by-seam, ISO Monday, DST policy — decided (spec L148);
- `UNIT_HINT` being a *second ratio table* — it isn't (spellings only);
- `DurationSeconds` `Display` dialect (N19) and `from_seconds` lying (N14) — fixed by 01.

---

#### Q3 — Internal contradictions

**C1 (highest). Ticket 05 ↔ review §3 B17 ↔ spec L75 ↔ the grammar.**
- Ticket 05 L3: *"all as registry entries, **no new operators or grammar**"*; demo contract: *"a filter expression that adds `dur(\"1 month\")` to a note's date and compares the result against another date returns the same answer as the template engine would."*
- Spec L75: *"Query temporal features are `FilterFunction` registry entries only — never new operators or grammar."*
- Review §3: *"**B17** query `date ± duration`, `date − date` (their headline idiom L13501/L4807) → `FilterFunction` entries (`filter.rs:116–125`), never operators."*
- **Reality (G1):** `FilterFunction` is a one-variant *predicate* enum with a literal-only argument parser; `dur("…")` does not parse; `date − date` must yield a **value**; a function result cannot be compared.
- **Unresolved:** either the grammar constraint is violated (needs a spec/review amendment naming the minimal expression surface), or stories 1–3 and ticket 05 items 1–3 are undeliverable as written. Not adjudicated anywhere in the corpus.

**C2. Spec L103/L102/L111 ↔ review §9 S4 + §10 step 6 ↔ ticket 06 ↔ ticket 03.**
Spec mandates *"Core `format_with` keeps strftime as its only grammar"* (L103), Decision-C *"`format_with` gets a real consumer (S4)"* (L111), and §5.6's shared `render_pattern`; ticket 06 items 2 and 15 repeat both; **ticket 03 deleted `DateValue::format_with` and `DateTimeValue::format_with` and counted the seven suppression deletions as a remediation win** (03 L105–108). Three artifacts promise a surface the fourth removed.

**C3. Ticket 09 ↔ spec L111 ↔ review §2.3/§10 step 6 ↔ HEAD.**
Four enumerations that disagree: ticket 09's six-method set (five already gone), review's *"8 methods"*, review §10 step 6's six named deletions (four gone), and OBSERVED six *different* live items (Q1c). Plus 09's `duration.rs:217` (now 353) and its `to_rfc3339()` premise (0 hits).

**C4. `review.md` §10 + §9 + §4 vs the resolved tickets.**
- §10 step 2 *"**Correctness:** S10 (N1, N4, N3′, N14, N19, D9, local-naive + DST policy, pinning tests) + S6 (N12/N17)"* — all of it landed via 01/02.
- §10 step 3 *"**Load-bearing seam:** S2 → S1 → S5 → S7"* — S2 and S1 landed via 03.
- §9 rows S1/S2/S6/S10 in future tense.
- §10 step 6 lists four deleted methods.
- §4 headed *"Also fixed (carried)"* while its `DEFAULT_DATE_FORMAT` bullet is still open (08 item 14, `config/model.rs:52` live).
- review L5 *"No implementation performed"* — contradicted by `c990f6c9`, `90696f25`, `57ea1b09`→`bcc938b5`.

**C5. Ticket 03's blocker declaration vs spec's encoding rule.**
- Spec L141: *"Execution order (**tickets will encode these blocking edges**)"*; L142 puts correctness (01/02 scope) **before** the seam chain (03/04): *"1. Correctness: N1, N4, N3′, N14, N19, D9, local-naive parse + DST policy … + serde repair (N12, N17). 2. Load-bearing seam chain: parts/registry (S2) → calendar owner (S1) → classify (S5) → precision hoist (S7) — strictly in this order."*
- Ticket 03 L5: *"**Blocked by:** None (01 and 02 merged)."*
- Ticket 01's own comment (01 L37 and 01 L97): *"**ticket 03 (`Blocked by: 01`)** … can proceed once this lands."*
- Two artifacts disagree about an edge the spec says the tickets own.

**C6. Ticket 05's blocker note misstates dependency direction.**
05 L5: *"**Blocked by:** 04 (needs the calendar owner behind a stable recognition/classification interface; **the calendar owner is 03, reached via 04**)."* The calendar owner is reached directly from 03; 04 is a recognition/precision ticket. Correct statement: 05 needs *both* the calendar owner (03) and the recognition interface (04), and the chain is 03 → 04 → 05.

**C7. Spec D14 / story 13 vs `From<DateValue> for DateTimeValue`.**
- Spec L98: *"None caused by a tz-data/OS error surfaces as an error, **never a silent shift** (gap verified by probing adjacent local times)."*
- Spec L39 (story 13): *"…only a tz-data/OS lookup failure does [fail]."*
- `date.rs:738–747`: silent UTC-midnight fallback, documented in rustdoc, reachable from `==`/`!=` filters via `is_equal_to_date`.
- And absent from ticket 08's divergence register (08 item 13 / spec L117). *(full detail in G4)*

**C8. Review §9 S9 vs the ticket 05/07 split.**
- §9 S9: *"Shorthands `sow/eow/soy/eoy`, `weekday`, ISO-8601 offset translation | seam: **engine adapters over S1** | parity: **B11**, T1, T3."*
- Ticket 05 item 4: *"bucketing helpers … are ticket 07's deliverable per spec story 4, not claimed here."*
- Ticket 07 items 11–12: bucketing on **both** seams — *"as query `FilterFunction` registry entries … and as template shorthands — ticket 07 owns both seams per spec story 4 (05 defers them here)."*
- So S9's seam column names only the engine side while the delivery ticket owns the query registry side too. Minor, but the S-table is what an implementer reads for seam scope.

**C9 (minor). Ticket 03's internal tension around `is_calendar`.**
- 03 handoff L175–181: *"Do not replace the matches with a shared `is_calendar()` predicate — that would bypass the exhaustiveness gate."*
- 03 handoff L164–169: declares `DurationValue::is_calendar()` (a predicate) **spec-mandated surface** whose consumer is *"future classify/temporal callers"*.
- OBSERVED `duration.rs:461–473`: it **is** an exhaustive match (no wildcard: `Day|Week|Month|Year => true`, `Millisecond|Second|Minute|Hour => false`), so no bypass occurs today.
- The tension is that a predicate 03 warns against *using as the shared implementation* is simultaneously declared keepable surface with a speculative consumer. Ticket 09 does not list it, so no deletion test will resolve it.

**C10 (minor). Ticket 04's inventory is internally consistent but externally incomplete.**
- Item 16 names two 10-byte gates; item 12 names `note/field.rs:115` as a *discard*; item 13's AC (*no production call site combines a shape guard with `parse`*) will reach it only implicitly. `note/field.rs:114` is simultaneously a **grammar gate** (story 15) and a **guard site** (N15) and is classified as neither. *(full detail in G3)*

**C11 (minor, factual-inventory flavour). Ticket 04's own stated amendment record vs what was verifiable at pin time.**
04 L30–35: *"Seven factual amendments applied in place … stale `.ok()` claim, cascade/scan counts, `DatePoint::has_time` single-source …"* — the *counts* were corrected, but the *line numbers* were not re-derived, so the amendment pass certified stale numbers (see §0 table). This is a process contradiction (a "review and pin" commit that did not verify), not a content one.

---

## External prior art (post-v4)

- `research/sql-temporal-conventions.md` — SQL engines' temporal conventions (PG/MySQL/SQLite/DuckDB/Trino/BigQuery); backs: UTC-storage precedent, DST gap-forward + PG-later-vs-earlier overlap divergence, magnitude interval equality, ISO Monday in PG `date_trunc`, null-ordering disagreement among engines.
- `research/general-temporal-libraries.md` — Temporal/Luxon/Java/Python/standards; backs: DST `compatible`/earlier consensus, dual civil-vs-zoned timelines, magnitude duration equality, strict-parse norm, CLDR locale-data weeks.
- `research/rust-temporal-ecosystem.md` — chrono/time/jiff/Arrow/DataFusion; backs: `MappedLocalTime` semantics + gap/error conflation, chrono `Days`/`Months` vs `TimeDelta` regime split, Arrow Duration-vs-Interval, chrono strftime as the de-facto Rust dialect, known chrono critiques (RUSTSEC, serde history).

**Post-v4 evolution:** the DST-ambiguity policy (the one open item below) is now **settled** — ambiguous → `.earliest()`, nonexistent → shift forward (see §4 B1) — and null ordering is **decided** (keep current behavior + ADR, §3). ~~Everything else is decided.~~ **(2026-10-05 correction: no longer true — four maintainer decisions are open and recorded as such: §11 X1 query grammar, X4 `From<DateValue>` fallback, U1 comma precedence, X7 multi-part `apply` consumer; plus sequencing questions D1-vs-04 and X8. Spec and tickets carry the questions inline.)** Post-v4: external prior-art research (`research/`) and the chrono-API audit added the local-wall-clock calendar frame, day-as-calendar, the gap-verified resolver, and the chrono format/week delegation mandates (spec 'Implementation Decisions').

**Post-v4 decisions (defined in `spec.md`):**
- **D12** — calendar frame = local wall clock: day/week/month/year application round-trips `DateTime<Utc>` → local naive → UTC through the resolver; sub-day units remain exact on the instant.
- **D13** — day is a calendar application unit; identity stays seconds-based (`1d == 24h` as values, may shift differently across DST).
- **D14** — gap-verified `MappedLocalTime` resolver: ambiguous → earliest, true gap → shift forward, tz-data/OS `None` → error.
- **Serde channel** — `Serialize` emits explicit RFC3339 `…Z` via `to_rfc3339_opts(SecondsFormat::Secs, use_z=true)`; human `Display` remains local-naive. The local→UTC rule therefore applies only to naive user-authored input, while serialized instants round-trip exactly.
