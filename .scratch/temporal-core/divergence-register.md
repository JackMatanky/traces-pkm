# Temporal Divergence Register

This register documents intentional architectural and behavioral divergences between Traces and external systems (Dataview, Templater, SQL engines, Luxon, and Temporal), citing design decisions and the underlying research corpus:
- [SQL Temporal Conventions](research/sql-temporal-conventions.md)
- [General Temporal Libraries](research/general-temporal-libraries.md)
- [Rust Temporal Ecosystem](research/rust-temporal-ecosystem.md)

---

## Entry 1: UTC Storage of Instants
- **Divergence**: Traces stores all date-time instants as UTC in index and memory, while rendering human displays as the local wall clock.
- **External Behavior**: Dataview and Templater often preserve local timezone offsets or arbitrary strings in memory, leading to offset mismatches across machines or synchronization boundaries.
- **Rationale**: Eliminates cross-platform timezone comparison defects while maintaining human-intuitive local displays.
- **Specification**: Spec Story 12, Decision D9.

## Entry 2: Space-Separated DateTime Acceptance
- **Divergence**: Traces accepts space-separated date-time strings (`2026-07-29 14:30:00`) in frontmatter and query literals alongside standard ISO-8601 `T`-separated strings.
- **External Behavior**: Strict ISO-8601 parsers reject space-separated dates.
- **Rationale**: User frontmatter and markdown vaults frequently use space-separated dates for readability; Traces accommodates common authoring conventions without compromising parse safety.
- **Specification**: Spec Story 14, `TextShape` recognition.

## Entry 3: Dual Regime for Duration Identity vs. Application (Luxon Parity)
- **Divergence**: Duration equality and hashing compare canonical total seconds (`1d == 24h`, `1mo == 30d`), but duration application treats calendar units (`d`, `w`, `mo`, `y`) as calendar units on the civil wall clock.
- **External Behavior**: Strict physical duration models treat all units as fixed seconds; pure calendar models reject cross-unit equality.
- **Rationale**: Follows Luxon's dual-regime convention: users expect `1d == 24h` in numeric comparisons, but expect adding `1d` across a Daylight Saving Time shift to preserve wall-clock time rather than advancing 24 physical hours.
- **Specification**: Spec Decision A2′, Decision D13; backed by Seam 1 test pins.

## Entry 4: Explicit NoteFieldType Rank
- **Divergence**: Field comparisons between mismatched types follow an explicit, type-safe total ordering hierarchy (`Null < Bool < Number < String < Date < DateTime < Duration < Link < List < Object`) rather than JavaScript's dynamic coercion or alphabetical type-tag sorting.
- **External Behavior**: Dataview sorts types by string representation or dynamic JavaScript ordering rules.
- **Rationale**: Guarantees deterministic, reproducible sort keys and query results across platforms.
- **Specification**: Spec Story 42, `NoteFieldValueRef::compare`.

## Entry 5: Canonical Duration Equality
- **Divergence**: Durations compare equal based on total normalized magnitude in seconds, independent of input spelling (`1h 30m == 90m`).
- **External Behavior**: Text-based or AST-based systems distinguish spelling variations as unequal.
- **Rationale**: Users expect semantic equality between equivalent duration expressions.
- **Specification**: Spec Story 42, `DurationValue::eq`.

## Entry 6: Null Ordering in Sort and Filter
- **Divergence**: `Null` ranks below all non-null values in sorting (nulls place first in ASC, last in DESC), and `Null` never satisfies any ordered comparison (`<`, `<=`, `>`, `>=`) in filter expressions.
- **External Behavior**: SQL engines diverge widely: PostgreSQL defaults to `NULLS LAST` in ASC; SQLite puts nulls first; Trino and DuckDB put nulls last. Dataview implicitly coerces nulls to falsy values.
- **Rationale**: Prevents missing or optional fields from inadvertently matching inequality filters while clustering missing records predictably at list boundaries.
- **Specification**: Spec Decision D15; filter and sort evaluation guards.

## Entry 7: Pinned ISO Monday Weeks
- **Divergence**: Week starts and ISO week numbering are pinned strictly to ISO Monday across all platforms, templates, and queries, with no CLDR locale dependency.
- **External Behavior**: Moment.js, Templater, and standard locale libraries vary the start of the week (Sunday vs. Monday) based on the host OS locale.
- **Rationale**: Eliminates environment-dependent query divergence where the same vault produces different weekly buckets on machines with different system locales.
- **Specification**: Spec Story 4, Week mandate L109.

## Entry 8: Local-Naive Parse with Deterministic DST Policy
- **Divergence**: Naive datetime inputs are interpreted in the local system timezone and resolved to UTC via an explicit DST resolver: ambiguous fall-back times resolve to the earliest occurrence; spring-forward gaps advance across the gap; missing system zone information produces an explicit error.
- **External Behavior**: Chrono and standard libraries often reject ambiguous or nonexistent times outright or panic.
- **Rationale**: Replicates Temporal `'compatible'` / RFC 5545 behavior so that user notes written across DST transitions never fail to load.
- **Specification**: Spec Decision D14; `local_minus_utc` resolution.

## Entry 9: Strictness-by-Seam
- **Divergence**: Input parsing strictness depends explicitly on the operational seam: note and template inputs are lenient with actionable error diagnostics; serde deserialization is strict; query filters enforce typed evaluations without silent coercions.
- **External Behavior**: Many engines enforce either blanket leniency (silent null fallbacks) or blanket strictness.
- **Rationale**: Balances forgiving human authoring with invariant-preserving persistence and query correctness.
- **Specification**: Spec Story 42.

## Entry 10: Strftime Dialect with Scoped Moment-Token Translation
- **Divergence**: Core formatting uses `chrono::format::strftime` syntax, while template date filters accept moment.js tokens via a scoped translation layer (bracket literals, `YYYY MM DD HH mm ss`, `Do`/`S` ordinals, `dddd`/`ddd`/`MMM`/`MMMM`). Unsupported moment tokens fail with an explicit diagnostic naming the token.
- **External Behavior**: Templater uses unvalidated moment.js strings; Dataview uses Luxon tokens.
- **Rationale**: Maintains a single robust formatting engine while allowing templates copied from Templater recipes to render accurately.
- **Specification**: Spec Story 8–9, Decision D-b.

## Entry 11: DST Overlap Resolves Earlier
- **Divergence**: During an ambiguous fall-back DST transition (e.g., 01:30 repeating twice), Traces resolves to the earlier UTC instant.
- **External Behavior**: PostgreSQL resolves to the later occurrence; Temporal, Jiff, and BigQuery resolve to the earlier occurrence.
- **Rationale**: Adheres to modern ecosystem consensus (Temporal/RFC 5545) for deterministic instant assignment.
- **Specification**: Spec Story 13, Decision D14.

## Entry 12: Calendar Unit vs. Sub-Hour Exact Application
- **Divergence**: Applying days, weeks, months, or years shifts the civil wall-clock date, whereas sub-day units (hours, minutes, seconds, milliseconds) apply as exact physical durations on the instant.
- **External Behavior**: Naive timestamp libraries treat `+ 1 day` as `+ 86400 seconds`, which drifts the wall clock across DST boundaries.
- **Rationale**: Replicates PostgreSQL `interval '1 day'` vs `interval '24 hours'` semantics: human calendar intent differs between "tomorrow at this time" and "in exactly 86,400 seconds."
- **Specification**: Spec Decision D13.

## Entry 13: WebAssembly Local Time Limitation Documented
- **Divergence**: On the `wasm32-unknown-unknown` target, local timezone resolution falls back to UTC because `chrono::Local` returns only `Single` instances in browser runtimes (chrono #1701).
- **External Behavior**: Undocumented failure or unexpected UTC display.
- **Rationale**: Formally documents the target limitation; native targets remain fully zone-aware.
- **Specification**: Spec Story 42.

## Entry 14: Non-Gregorian Calendar Systems Out of Scope
- **Divergence**: Traces supports only the proleptic Gregorian calendar; eras, lunar calendars, and non-Gregorian systems supported by Luxon are explicitly out of scope.
- **External Behavior**: Luxon supports diverse international calendar systems.
- **Rationale**: Focuses scope strictly on personal knowledge management requirements.
- **Specification**: Spec Story 42.

## Entry 15: RFC 3339 `…Z` Serialization
- **Divergence**: `Serialize for DateTimeValue` emits explicit RFC 3339 `…Z` UTC representations (`to_rfc3339_opts(SecondsFormat::AutoSi, true)`), whereas `Display` renders the everyday local wall clock.
- **External Behavior**: Some libraries serialize using the same format as human display, losing timezone context across serialization boundaries.
- **Rationale**: Interoperable data interchange requires explicit UTC offsets, while human display requires local wall clocks.
- **Specification**: Spec Story 41, Decision RPT-F3.

## Entry 16: Computed Duration Display Synthesis
- **Divergence**: A computed duration's `Display` output is synthesized from normalized total seconds (`canonical_raw`) and is display-only; internal equality and hashing remain strictly seconds-based. Equal-seconds durations may display a representation differing from their written components (e.g., `parse("1mo") + parse("0s")` displays as `"4w 2d"` while retaining calendar month application).
- **External Behavior**: Systems either discard written components entirely or serialize them as unnormalized strings.
- **Rationale**: Raw string representations serve display purposes only; witness parts are preserved for calendar math without corrupting value equality.
- **Specification**: Spec Decision NU-1, Spec L84/L90.

## Entry 17: `From<DateValue> for DateTimeValue` Fallback to Neutral UTC Frame
- **Divergence**: In the infallible conversion `From<DateValue> for DateTimeValue`, if the host system timezone lookup fails, the conversion falls back to UTC midnight rather than surfacing `DateError::LocalZoneLookup`.
- **External Behavior**: Fallible operations surface `LocalZoneLookup` directly.
- **Rationale**: Infallible promotions are required by `Ordering::cmp`, sort key construction, and filter equality checks that cannot propagate errors. A `TryFrom` refactor would duplicate local-zone fallback logic across every sort and comparison site. Fallible query and template operations continue to surface `LocalZoneLookup`.
- **Specification**: Spec Decision X4(a), Decision D14 declared exception.

## Entry 18: List-First Comma Precedence at Inline Value Seam
- **Divergence**: In markdown body and inline field text, when a comma follows a complete atomic value with remaining content (e.g., `1h, 30m`), list parsing takes precedence, yielding a two-element list of durations. Commas retain duration-part separator semantics only in explicit duration contexts (`dur("1h, 30m")`, YAML scalars).
- **External Behavior**: A duration parser with comma lookahead would consume the comma and parse `1h, 30m` as a single compound duration, breaking markdown list syntax.
- **Rationale**: Preserves standard Markdown comma-separated list parsing while keeping compound duration parsing functional in explicit expressions.
- **Specification**: Spec Decision U1/F1, Ticket 04.

## Entry 19: No Epoch-Day Division for Week Calculations
- **Divergence**: Week calculations use chrono's ISO week primitives (`NaiveDate::iso_week()`, `from_isoywd_opt`), strictly avoiding epoch-day integer division (`floor(day / 7) + 1`).
- **External Behavior**: Dataview historically calculated week numbers via `floor(day / 7) + 1`, causing incorrect week assignments near year boundaries.
- **Rationale**: Prevents the Dataview `.week` footgun; ISO-8601 week calculations are mathematically correct and standard.
- **Specification**: Spec Story 4, Week mandate L109.

## Entry 20: No Lenient Format Guessing in `parse_with`
- **Divergence**: `DateValue::parse_with(text, fmt)` enforces a strict contract: matching formats succeed; non-matching text or invalid format patterns return explicit errors, rejecting heuristic guessing.
- **External Behavior**: Templater and moment.js attempt heuristic format recovery when a format pattern fails to match.
- **Rationale**: Silent or heuristic date parsing causes subtle data corruption in notes and queries; strict errors ensure predictable behavior.
- **Specification**: Spec Story 18, Ticket 07.
