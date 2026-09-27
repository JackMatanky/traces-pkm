# 03: Duration semantics end-to-end — parts, single registry, calendar owner

**What to build:** `dur("1 month")` means one thing, declared in one place: the value's identity is its magnitude (fixed ratios, named `fixed_seconds`), its parsed shape is retained, and applying it to a date is calendar-correct through a single calendar owner that the template engine delegates to. Demo contract: `dur("1 month") == dur("30 days")` as values yet they shift a date differently — demonstrated by a pinning test, documented in the glossary.

**Blocked by:** 01 (duration display/`from_seconds` settled first), 02 (datetime `apply` needs the zone doctrine).

**Status:** ready-for-agent

Skills: `rust-skills`, `rust-unit-testing`, `codebase-design`. Rules: `api-parse-dont-validate` (Σ invariant by construction), `type-enum-states` (parts as regime witness), `num-overflow-explicit`, `anti-over-abstraction` (no `Clock`/`TimeZone` traits — one adapter = hypothetical seam); calendar application uses chrono's own primitives (`checked_add_months(Months)` / `checked_add_days(Days)` on the local naive value — never `naive_local()`, which panics) — no hand-rolled month arithmetic. Design record: `../review.md` §4 (A2′), §5.1–5.2, §9 (S1+S2).

- [ ] `DurationValue` retains `parts: Option<…>` alongside seconds; parsing is the only path filling `Some` and computes seconds by summing those same parts in one statement — no state where they disagree
- [ ] `from_seconds` stores `None` (honest: cannot synthesize Month/Year); regime witness matches exhaustively: month/year parts ⇒ calendar, else fixed, `None` ⇒ fixed
- [ ] `seconds()` renamed `fixed_seconds()` (and `seconds_i64` → `fixed_seconds_i64`) at every call site; calendar meaning exposed nowhere except behind the calendar owner
- [ ] Exactly one unit-ratio table: the private ms-table is derived from the registry const; Month/Year omission from magnitude conversion documented (N18, N2)
- [ ] Calendar owner exists in the date module: `shift(base, n, unit)`, `diff(a, b, unit)`, `apply(base, &DurationValue)` applying parts left-to-right in written order; the engine's copied shift/diff deleted and delegation in place (H4 shrinks)
- [ ] A2′ pinning test demonstrates equal values shifting dates differently (`dur("1 month")` vs `dur("30 days")`); the cross-spelling equality contract (`dur("1h 30m") == dur("90m")`) stays pinned green; CONTEXT.md clause (c) text is ticket 08's job — this ticket supplies the executable test it cites
- [ ] `checked_add`/`checked_sub` superseded by `apply` (removed or re-backed — no API pretending to arithmetic it can't do); ticket 09 verifies no residue remains
- [ ] `shift`/`apply` preserve the local wall clock (spec D12): `DateTime<Utc>` → local naive → chrono calendar add → back through the resolver; month clamping matches chrono's documented behavior (Jan 31 + 1 month → last day of Feb), pinned by a test citing chrono's own example
- [ ] `day` is a calendar application unit (spec D13): `1d == 24h` as values but they shift differently across a DST transition — the extended A2′ incoherence pinned by test; sub-day units apply as exact durations
- [ ] `signed_years_since`/`signed_months_since` relocated from the template engine into the calendar owner (chrono has no months-between primitive — this custom code is justified and single-owned here)
- [ ] `mise run verify` green
