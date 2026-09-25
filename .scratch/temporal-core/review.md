# Design Record: Comprehensive Review v4 — `date.rs` + `duration.rs`

Companion to `spec.md`: the full findings/analysis record. Tickets cite IDs from here instead of re-deriving. Status: accepted (decisions final).

**Decisions:** A2′ accepted · B1 + naive = local→UTC (DST: ambiguous → earliest, nonexistent → shift forward) · D-b scoped · D11 widen (bundled with S7) · ISO Monday · null never satisfies ordering (ADR) · test seams confirmed (value types / FilterFunction registry / template filters).

---

## 1. Seam landscape

| Seam                | Module                                  | Adapters today                                                        | Missing / broken                                                     |
| ------------------- | --------------------------------------- | --------------------------------------------------------------------- | -------------------------------------------------------------------- |
| Recognition/parsing | `date.rs`, `duration.rs`                    | note lexer/inline/list, `TextShape`, field coercion, engine `ParsedDate`  | derived `Deserialize` bypasses it (N12); guard protocol leaks (N15/N7) |
| Formatting          | `format_with`, `Display`                      | engine `format_precise` + engine's own `format_with`                      | moment-dialect input (D-b → S8); `durationformat` (B14 → S4)           |
| Calendar arithmetic | **no owner**                                | engine `date_shift_unit`/`date_diff` (copy), dead `checked_add`/`checked_sub` | query-side arithmetic, `weekday`, shorthands — all blocked on S1       |
| Unit registry       | `UNIT_MAP`/`DurationUnit`                     | duration parse, engine kwarg + shift                                  | regime declaration (N2 → S2); **4th silent ratio copy** (N18)            |
| Clock               | **no doctrine**                             | `Local::now` ×4, `Utc::now` ×3 (engine)                                   | local-naive decided, display policy (D9 → S10)                       |
| Precision           | engine-private `DatePrecision`/`ParsedDate` | arithmetic re-serialization only                                      | recognition result should own it (N16/D10 → S7)                      |

Deletion test: every value type earns its keep (grammar + registry + error taxonomy reappear across ≥6 call sites). Row 3 is the structural root of N2/D1/D9; row 6 is a seam an adapter had to invent because the public one lost information.

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
| **N12** | Derived `Deserialize` bypasses `parse_iso`: chrono `NaiveDate::from_str` skips 4-digit guard + rejects `YYYY-MM`; datetime RFC3339-only; serial `…Z` ≠ Display; `NoteFieldValue` carries both | S6 (manual impl, duration's manual-impl precedent)                                     |
| **N15** | Guard-protocol leak (`can_start`/`is_iso_shape`/`has_four_digit_year` are protocol functions); `field.rs` forgot the protocol precisely because the interface demands hidden knowledge    | S5 — classify, guards go private                                                 |
| **N16** | `YYYY-MM` → day 1, shape forgotten; precision belongs to the recognition result                                                                                                     | S7 (hoist `Precision`)                                                             |
| **N17** | `DateValue` public, `DateTimeValue`/`DateError` `pub(crate)`; external `FromStr::Err` unnameable; no `#[non_exhaustive]` on either error                                                      | S6                                                                               |
| **N18** | `from_seconds` carries a private 4th copy of unit ratios (ms-table) omitting Month/Year undeclared                                                                                   | S2 (derive from registry; document omission)                                     |
| **N20** | Cascade loop copied between `DateValue::parse_iso` and `DateTimeValue::parse_iso`                                                                                                     | S5-era: `parse_any` on format enums                                                |
| **N21** | Invalid-pattern detection exists 3× (date, datetime, engine)                                                                                                                            | S4: shared renderer                                                                |
| **N23** | `From<FieldValueRef>` embeds classification policy (null→link→duration→string) inside a `From` impl                                                                                   | S5: duration step → `classify`                                                     |
| **D11** | Doc ≠ behavior: `date_add` docs promise six units; `DurationUnit::parse` accepts `weeks`/`ms`/abbrevs                                                                                     | decided: widen doc, bundle with S7 (real trap = precision no-ops, S7 fixes)     |

### 2.3 Structural / depth debt

| ID  | Finding                                                                                                                                                                                                                                      |
| --- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **D1**  | Format cascade duplicated ×4 (date/datetime × parse/engine)                                                                                                                                                                                  |
| **D6**  | ISO-prefix byte scan duplicated ×4                                                                                                                                                                                                           |
| **D7**  | `NaiveDate`/`DateTime<Utc>` leak via `into_inner`/`From` — **demoted**: `NaiveDate` is already validated, so this is surface-area, not a parse-don't-validate hole. Keep the `From` impls; shrink the *need* (S1/S7); re-run deletion test on `into_inner` after |
| **D10** | Engine-private `DatePrecision` + `ParsedDate` + `format_precise` = a recognition-result type an adapter had to build because the public seam lost precision — direct evidence for N16                                                              |
| **H3**  | Recognition is not one seam: `TextShape` + three guard fragments of one classifier                                                                                                                                                             |
| **H4**  | Engine is a fat adapter: owns calendar arithmetic, formatting, clock                                                                                                                                                                         |
| **—**   | Dead bridge: `checked_add`/`checked_sub` unusable until A2′ makes them calendar-aware (or superseded by S1's `apply`)                                                                                                                              |
| **—**   | 8 methods behind `#[cfg_attr(not(test), expect(dead_code))]` — Decision-C: wire (S4 wires `format_with`) or delete (`to_offset_string` may become interop serial, `to_time_string`/`start_of_day`/`cmp_date` likely delete)                              |

## 3. Parity assessment (condensed)

### Dataview (22,124 lines, L-cited)
**At parity:** ISO recognition incl. `YYYY-MM`; multi-unit durations (superset with `y`/`ms` — keep); cross-spelling value equality (stronger than theirs, E4); template-side arithmetic; `file.ctime/cday/mtime/mdate`; task date fields; midnight coercion; null-placement superset.

**Gaps:** **B17** query `date ± duration`, `date − date` (headline idiom L13501/L4807) → `FilterFunction` entries, never operators; **B18** duration arithmetic (follows); **B14** `durationformat` (spec L6803–6825); **B12** parse-with-format (L6075); **B11** `sow/eow/soy/eoy` (L14960); **B24** components minus `.week`; **B26/B30** `file.day` (L3652).

**Divergences to register:** UTC instants (tz-bug pedigree #1412/#601 — strengthened: parse is TZ-dependent by design, storage stays UTC); null never satisfies an ordering (ADR, decided: keep current behavior); explicit `NoteFieldType` rank; canonical duration Eq; space-datetime acceptance (note `TextShape` reclassification side effect).

**Do-not-port:** `.week`=`floor(day/7)+1`; epoch-falsy; Luxon `YYYY` week-year; null-min comparisons; `today()`/`now()` parse-then-fail; alphabetical type ordering.

### Templater (13,045 lines, L-cited)
**At parity:** day offsets; `tomorrow`/`yesterday`; arbitrary format output; `start/end_of_month`.

**Gaps:** **T1** ISO-8601 `P1M`/`P-1M` offsets (L4973) → adapter translation at `date.now`, not a second grammar; **T2** `reference`/`reference_format` (L4968) → converges with B12, one `parse_with`; **T3** `weekday(n)` (L4992), pinned ISO Monday; **T4** moment tokens (L10978/11106) → D-b; **T5** 100% local clock (L4963) → B1.

**Do-not-port:** type-overloaded `offset`; `P-1M` internal sign; lenient reference guessing; locale weekday; `stat.ctime` lie; unvalidated format strings (we keep `InvalidPattern`).

## 4. Decisions (final)

### A2′ — accepted
- `mo/y` stay parseable; `DurationValue` retains `parts: Option<Box<[(f64, DurationUnit)]>>` alongside `seconds`.
- **Identity stays seconds-based** (`fixed_seconds()` ratios); **application** to dates is calendar for Month/Year parts via one owner (S1), applied left-to-right in written order.
- Luxon-style incoherence (`"1mo" == "30d"` but different date-shifts) **accepted and declared** — pinned by test + CONTEXT.md clause.
- A1′ (reject `mo/y`) breaks Dataview L4814 and Templater L4973; A3′ (shape identity) breaks pinned `"1h 30m" == "90m"`. Rejected.

### B1 + naive-input = local → UTC on parse — accepted
- Instants stored as UTC; naive input interpreted in local zone, converted to UTC at parse (parity; avoids wrong-by-hours comparisons vs `file.mtime`).
- Date-only values are civil dates — no zone; `DateValue` untouched.
- `now`/`today`/`tomorrow`/`weekday`/file-stat display from the local clock; storage always UTC.
- **DST policy (decided):** ambiguous → `.earliest()`; nonexistent → shifted forward by the gap. Tests inject `TZ`.

### D-b — accepted, scoped
- moment→strftime translator **at the template seam only**; `format_with` keeps exactly one grammar (strftime).
- Scope 1: bracket literals `[Daily]`, `YYYY MM DD HH mm ss` family, `Do`/`S`, `dddd`/`ddd`/`MMM`/`MMMM`; else `InvalidPattern`.
- Error message is part of the design: names the offending token, identifies moment-dialect, points at the supported list.
- Strictness deliberate: silently mistranslating ambiguous tokens (`YYYY` week-year) would be worse than erroring. Growth additive (OCP).

### D11 — widen doc, bundle with S7
- `date_add`: "unit accepts any `DurationUnit` spelling" — never a second registry.
- Real trap is precision truncation (sub-day units on date-only inputs no-op) — fixed by S7; doc line lands with it.

### Also fixed (carried)
- Start-of-week = ISO Monday, documented divergence.
- **Null ordering (decided):** keep "null never satisfies an ordering"; ADR replaces the code comment.
- Config default-format constant removal → substitute `DEFAULT_DATETIME_FORMAT` (different value — note it).

## 5. Type & pattern designs

### 5.1 `DurationParts` — A2′ without dual authority
```text
DurationValue { raw: Box<str>, seconds: DurationSeconds, parts: Option<Box<[(f64, DurationUnit)]>> }
```
- Sole identity = `seconds` (Eq/Ord/Hash unchanged). `parts` = parsed shape; `None` for synthesized values (`from_seconds` can't know Month/Year; ms-round would falsify exact-parts claims anyway).
- Invariant by construction: `parse` is the only path filling `Some`, computes `seconds` by summing those parts in one statement.
- `parts` doubles as regime witness: `Some` with `Month|Year` ⇒ calendar; `Some` without ⇒ fixed; `None` ⇒ fixed. Data, matched exhaustively.
- Rejected: deriving seconds on demand (dies on `None`); always-`Some` approximations (representable invalid state).

### 5.2 Declared regimes
- `DurationUnit::seconds()` → `fixed_seconds()`, `seconds_i64()` → `fixed_seconds_i64()`.
- Calendar meaning gets no counterpart method on the unit — exists only behind S1's `shift` (locality).

### 5.3 `classify` — both domains, one protocol each
```text
DateValue::classify(s)     -> Option<Result<DateValue,     DateError>>      // #[must_use]
DurationValue::classify(s) -> Option<Result<DurationValue, DurationError>>
```
- Trichotomy is the guard protocol: `None` = cheap shape gate (O(1), no allocation — lexer hot path), `Some(Err)` = allocating detail, `Some(Ok)`.
- Replaces `can_start && parse` (sort), fixes N7 (coercion), kills N8 divergence, absorbs N23's copy; guard functions go private (N15 closed).
- `TextShape` becomes a caller.
- Rejected: combined mega-classifier — coercion decision is coercion-layer policy.

### 5.4 Precision hoist (N16/D10)
```text
pub(crate) enum Precision { YearMonth, Date, DateTime }
```
- `parse_iso` unchanged (clean value for Eq); precision-carrying recognition entry for re-serialization. Engine's trio delegates → shadow interface deleted; `YYYY-MM` precision survives; D3 fixable once.
- `Precision` never in `DateValue`'s Eq (`type-enum-states`).

### 5.5 Serde repair (N12/N17)
- Manual `Deserialize` via `parse_iso` (`Cow<'de, str>` + `Error::custom`) — duration's existing manual-impl precedent.
- One serial spelling per channel: serialize via `Display` (round-trips by construction); RFC3339 `…Z` for explicit interop (`to_offset_string`).
- `#[non_exhaustive]` on both errors.

### 5.6 Smaller mechanisms
- N18: delete ms-table; derive greedy units from one registry `const`; document Month/Year omission.
- N21: one shared renderer returning a neutral error; call sites map to their own error types.
- N20/D1: format enums gain a `parse_any` constructor owning the cascade loop.
- B18: `Add`/`Sub` for `DurationValue` (seconds add, parts concat when both `Some`, `-0.0` normalized); `Mul<f64>` scales both.
- N19: chosen rendering dialect (short/general, exponent above threshold).

### 5.7 Considered and rejected (`anti-over-abstraction`)
`Clock`/`TimeZone` traits; `Dialect` trait; typestate builders; merged errors; `Cow<'a, str>` raw; locale first-day-of-week.

## 6. Principles audit (summary verdicts)

| Principle                | Verdict                                                                                         |
| ------------------------ | ------------------------------------------------------------------------------------------------ |
| Separation of concerns   | ❌ calendar arithmetic ownerless (S1); precision split across seam (D10/S7); Display mixing into raw `f64` rendering (N19) |
| Single responsibility    | ❌ `From<FieldValueRef>` classifies + converts (N23/S5); `DurationUnit`'s third job legitimate only under A2′ + S1 |
| Cohesion & coupling      | ❌ engine reaches through registry/newtypes into chrono; target graph `note/query/template → date|duration → chrono`; `date/` split only if S1+S7+S11 push size |
| DRY                      | ❌ ×4 cascades (D1/N20), ×4 scans (D6), ×4 ratio tables (N18), ×3 format-with (N21), ×2 grammars (N8), ×4 guard sites (N15) — all single-point remedies |
| Modularity               | ❌ guard protocol changes rippled to callers (N15); S5 restores "change inside, not outside"     |
| Information hiding       | ❌ protocol fns `pub(crate)` → private under S5; `into_inner` monitored surface (D7); hidden facts → normative CONTEXT.md |
| Open/Closed              | ✅ `FilterFunction`, filter tables, format enums, `UNIT_MAP` — parity features are registry entries; ❌ engine unit-match            |
| General flexibility      | S1's `shift`/`diff`/`apply` serve N callers from one implementation; flexibility as data (pinned Monday, declared regimes), never single-adapter traits |
| Law of Demeter           | ❌ `can_start && parse`, chrono round-trips in `ParsedDate` — one call each after S5/S1          |
| Least astonishment       | N3′ `-0.0`, N14/D11 no-op shifts, N12 Display≠serde, strictness-by-seam → documented/pinned; local-naive flip was a least-astonishment correction |

## 7. CONTEXT.md revisions (a–g)

- (a) Funnel claim normative **and true** incl. deserialization (after S6).
- (b) Calendar owner named: date module owns shift/diff; template/query are thin adapters.
- (c) Duration regime clause (identity = magnitude; application = calendar L→R; equal values may shift differently — test-backed).
- (d) Date interface facts: four-digit-year; `YYYY-MM` → day 1; precision at recognition layer; naive datetime = local → stored UTC.
- (e) One `*Avoid*` glossary rule (never a concept name) — stop using Avoid terms inside definitions.
- (f) "Single owner of unit registry" without caveat (after `fixed_seconds()` + S1).
- (g) Strictness-by-seam + ISO Monday + DST earliest/gap policy.

## 8. Rust-skills conformance

| Rule                                    | Status                                                    |
| --------------------------------------- | --------------------------------------------------------- |
| `api-parse-dont-validate`                 | ❌ N12, N15/N7 → S6/S5; ✅ 5.1 invariant-by-construction |
| `serde-try-from-validate`                 | ✅ duration precedent; ❌ date → S6                      |
| `err-custom-type`/`err-source-chain`        | ✅ enums; ❌ N4                                           |
| `num-float-compare`/`num-overflow-explicit` | ✅ `total_cmp`/`try_from`; ⚠ N3′; ❌ N1                       |
| `type-enum-states`                        | ✅ proposed; ❌ today (D10)                               |
| `type-display-vs-debug`/`type-numeric-fmt`  | ❌ N14, N19                                               |
| `api-non-exhaustive`                      | ❌ N17                                                    |
| `conv-tryfrom-fallible`                   | ✅ `TryFrom<DurationValue> for TimeDelta`                   |
| `anti-over-abstraction`                   | ✅ 5.7 rejection list                                     |
| `proj-pub-crate-internal`                 | ⚠ N17                                                     |

## 9. Deepening plan (final, ranked)

| #   | Suggestion                                                                                                                                                                                                            | Seam / interface                     | Collapses                                       | Parity        |
| --- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------ | ----------------------------------------------- | ------------- |
| **S1**  | Calendar owner in date module: `shift(base, n, unit)`, `diff(a, b, unit)`, `apply(base, &DurationValue)` (parts, L→R)                                                                                                        | interface on Date/DateTime value types | engine copy, dead bridge, H4, future query impl | C10, L13506   |
| **S2**  | `parts: Option<…>` + `fixed_seconds()` rename + N18 registry merge                                                                                                                                                        | additive; single constructor         | N2 declared as data, N18                        | A2′           |
| **S3**  | Query temporal functions `date_add`/`date_diff`/`date_component` + duration arithmetic                                                                                                                                      | `FilterFunction` entries only          | —                                               | **B17, B18, B24** |
| **S4**  | `durationformat` + wire `format_with` + shared renderer                                                                                                                                                                   | template filters                     | N21, dead-code attrs                            | B14           |
| **S5**  | `classify` both domains; guards private; `parse_any` on format enums                                                                                                                                                      | one `#[must_use]` entry per type       | N8, N7-class, N15, N23, N20, D1                 | —             |
| **S6**  | Serde repair: manual `Deserialize`, Display serial spelling, `#[non_exhaustive]`                                                                                                                                          | duration precedent                    | N12, N17                                        | —             |
| **S7**  | Hoist `Precision`/recognition result; engine delegates; D11 doc widened lands here                                                                                                                                              | recognition result owns precision    | D10, N16, D3, D11                               | —             |
| **S8**  | D-b translator (scope 1, token-naming errors)                                                                                                                                                                       | pure functions, template seam        | T4                                              | T4            |
| **S9**  | Shorthands `sow/eow/soy/eoy`, `weekday`, ISO-8601 offset translation                                                                                                                                                      | engine adapters over S1              | —                                               | B11, T1, T3   |
| **S10** | Correctness pass: N1, N4, N3′, N14, N19, D9 + local-naive parse + DST (earliest/gap), N15b + A2′ pinning tests                                                                                                            | —                                    | —                                               | T5            |
| **S11** | `parse_with(text, fmt)` with `Result` contract                                                                                                                                                                            | serves `reference` + future LSP        | —                                               | B12, T2       |
| **S12** | `file.day` at index time                                                                                                                                                                                                | index computes, query exposes        | —                                               | B26, B30      |
| **S13** | Docs: CONTEXT.md (§7), null-ordering ADR, divergence register, config-const removal (`DEFAULT_DATETIME_FORMAT` caveat)                                                                                                   | —                                    | —                                               | C-audit       |

## 10. Execution order

1. ~~Decisions~~ — done (all seven: A2′, B1+local-naive+DST, D-b, D11, ISO Monday, null ADR, seams).
2. Correctness: S10 + S6.
3. Load-bearing seam chain: S2 → S1 → S5 → S7 (strict order).
4. Parity: S3 → S4, S8, S9, S11, S12.
5. Docs: S13.
6. Decision-C dead-surface deletions/wirings (`to_time_string`, `start_of_day`, `cmp_date`, `to_offset_string`→serial?, `checked_add/sub`→superseded by `apply`, `from_seconds`→keep).
