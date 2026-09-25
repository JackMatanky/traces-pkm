# General date/time libraries — how established systems handle dates, DST, equality, durations, formatting

Research findings for the temporal-core spec. All claims below were read from primary sources
(specs, official docs, library source code) fetched during this research session.

## Sources

1. TC39 Temporal proposal — Time Zones and Resolving Ambiguity: https://tc39.es/proposal-temporal/docs/timezone.html
2. TC39 Temporal proposal — Temporal.ZonedDateTime: https://tc39.es/proposal-temporal/docs/zoneddatetime.html
3. TC39 Temporal proposal — Temporal.Duration: https://tc39.es/proposal-temporal/docs/duration.html
4. RFC 3339 Date and Time on the Internet: Timestamps: https://datatracker.ietf.org/doc/html/rfc3339
5. ISO 8601 popular-standards page (and linked ISO 8601-1:2019 / 8601-2:2019): https://www.iso.org/iso-8601-date-and-time-format.html
6. CLDR weekData.json (cldr-core): https://raw.githubusercontent.com/unicode-org/cldr-json/main/cldr-json/cldr-core/supplemental/weekData.json
7. Python `datetime` module documentation (docs/python.org, built from `Lib`/`Doc/library/datetime.rst`): https://docs.python.org/3/library/datetime.html
8. PEP 495 — Local Time Disambiguation: https://peps.python.org/pep-0495/
9. IANA tzdb "Theory and pragmatics of the tz code and data" (theory.html): https://data.iana.org/time-zones/tzdb/theory.html
10. OpenJDK `java.time.ZonedDateTime` source: https://github.com/openjdk/jdk/blob/master/src/java.base/share/classes/java/time/ZonedDateTime.java
11. OpenJDK `java.time.Duration` source: https://github.com/openjdk/jdk/blob/master/src/java.base/share/classes/java/time/Duration.java
12. moment.js official documentation: https://momentjs.com/docs/
13. Luxon `DateTime` source: https://github.com/moment/luxon/blob/master/src/datetime.js
14. date-fns `parse` source (v4 monorepo, JSDoc is the docs source): https://github.com/date-fns/date-fns/blob/main/pkgs/core/src/parse/index.ts
15. `@date-fns/tz` README: https://github.com/date-fns/date-fns/blob/main/pkgs/tz/README.md
16. pandas `Timestamp.tz_localize` reference: https://pandas.pydata.org/docs/reference/api/pandas.Timestamp.tz_localize.html
17. python-humanize documentation (humanize.time): https://github.com/python-humanize/humanize
18. pretty-ms README: https://github.com/sindresorhus/pretty-ms
19. vercel/ms README: https://github.com/vercel/ms

## Findings

### Type model and naive/aware separation

1. Temporal splits exact time from wall-clock time into separate types: `Temporal.Instant`
   stores exact time only; `PlainDate`/`PlainTime`/`PlainDateTime` store calendar + wall-clock
   time; `ZonedDateTime` bundles exact time + IANA zone + calendar. A `Z` suffix or UTC offset
   in a string is what marks it as an exact time. [source: 1 — tc39.es/proposal-temporal/docs/timezone.html]

2. Python explicitly defines "naive" (no tzinfo) vs "aware" (tzinfo attached) datetimes and
   states "An aware object represents a specific moment in time that is not open to
   interpretation", while naive ones carry no UTC relation. [source: 7 — docs.python.org/3/library/datetime.html]

3. Python: "Naive and aware datetime objects are never equal." Comparing them raises
   `TypeError`; naive-minus-aware subtraction also raises `TypeError`. [source: 7]

4. Python: when both datetimes are aware with the *same* tzinfo, tzinfo and fold are ignored
   and the bare local datetimes are compared; with different tzinfo they are compared as if
   converted to UTC (without overflow). Instances in a repeated interval are never equal to
   instances in another interval (fold participates in cross-tzinfo equality). [source: 7]

5. Python docs concede that time zone rules are "more political than rational, change
   frequently, and there is no standard suitable for every application aside from UTC". [source: 7]

### DST gap/overlap disambiguation

6. Temporal exposes an explicit `disambiguation` option when converting wall-clock → exact
   time: `'compatible'` (default) = `'earlier'` for backward transitions (overlaps) and
   `'later'` for forward transitions (gaps); plus `'earlier'`, `'later'`, and `'reject'`
   (throws `RangeError`). [source: 1]

7. Temporal states that `'compatible'` "matches the behavior of legacy `Date` as well as
   libraries like moment.js, Luxon, and date-fns" and "also matches … RFC 5545 (iCalendar)" —
   i.e. the ecosystem de-facto standard is *earlier-on-overlap, later-on-gap* (equivalent to
   "shift nonexistent times forward"). [source: 1]

8. Temporal documents worked DST examples: gap `2020-03-08T02:30` in `America/Los_Angeles`
   resolves (`compatible`) to `03:30-07:00`; overlap `2020-11-01T01:30` resolves to the first
   (PDT) instance. The wall-clock difference between the two disambiguated gap instants is
   `PT2H` while the real elapsed time is `PT1H` — showing why wall-clock arithmetic and
   exact-time arithmetic must be kept apart. [source: 1]

9. Temporal: when both an offset and a zone ID are present but conflict (e.g. a string saved
   before a zone abolished DST), the `offset` option chooses: `'use'`, `'ignore'`,
   `'prefer'`, `'reject'`. `ZonedDateTime.from` defaults to `'reject'` (throws), while
   `.with()` defaults to `'prefer'` so small field edits don't silently jump an hour. The
   docs' worked example is Brazil's 2019 DST abolition invalidating previously-stored future
   timestamps. [source: 1]

10. Temporal: round-tripping through a plain wall-clock type loses the offset — a stored
    "second 1:30AM" can come back as the "first 1:30AM" when re-resolved with the zone. [source: 1]

11. Python PEP 495 resolves ambiguity with a `fold` flag: `fold=0` selects the earlier
    occurrence, `fold=1` the later. For a *gap* (nonexistent local time) with `fold=0`,
    `timestamp()` treats it as the later/shifted-forward instant. Adding a timedelta resets
    `fold` to 0; naive comparisons ignore `fold`. [source: 8 — PEP 495]

12. Java `ZonedDateTime.of`/`ofLocal`: on overlap pick the earlier offset; on gap shift the
    local time forward; `ofStrict` throws when no valid offset exists. [source: 10]

13. Luxon resolves a nonexistent ("hole") local time by shifting it *forward* (returns the
    max-side offset and flags the instance via `wasHole`; `DateTime.local(2017, 3, 12, 2).wasHole === true`).
    For ambiguous times it picks the instance consistent with an *offset guess*
    (code comment: "offset we'll pick in ambiguous cases (e.g. there are two 3 AMs b/c
    Fallback DST)"), and offers `getPossibleOffsets()` to enumerate both candidates. [source: 13]

14. moment parses a nonexistent local time (e.g. `'2013-03-10 2:30'` US) to a value that is
    *browser-dependent* ("either adjusting the time forward or backwards"), and documents
    `isDSTShifted()` as the way to detect this condition. [source: 12 — momentjs.com/docs, "Parsing dirty dates"]

15. pandas `Timestamp.tz_localize` defaults both hazards to *raising*: `ambiguous='raise'`
    and `nonexistent='raise'` (`ValueError`), with opt-ins `ambiguous ∈ {bool, 'NaT'}` and
    `nonexistent ∈ {'shift_forward', 'shift_backward', 'NaT', timedelta}`. [source: 16]

16. IANA tzdb warns that process-global `TZ` handling has sharp edges: `tm_isdst`/`tzname`/
    `daylight`/`timezone` are called out as vestigial APIs, and `mktime`-style disambiguation
    via `tm_isdst` works for proleptic TZ strings but **not** for geographical zones like
    `America/New_York`. [source: 9 — tzdb theory.html]

### Equality and ordering

17. Temporal `ZonedDateTime.equals` requires same calendar fields, same offset, same zone ID,
    and same calendar ID; `compare` orders by exact time (instant) first. [source: 2]

18. Java `ZonedDateTime.equals` = local dateTime + offset + zone; `compareTo` orders by
    instant, then local date-time, then zone id, then chronology; `isBefore`/`isAfter`/
    `isEqual` are *pure instant* comparisons. [source: 10]

19. Luxon `equals` requires identical millisecond value + same zone + same locale;
    `hasSame(other, unit)` instead checks whether `this` falls in the startOf/endOf window of
    `other` after re-zoning (`keepLocalTime`) — a *local-calendar* comparison, deliberately
    distinct from `equals`. [source: 13]

20. Java `Duration.equals` is magnitude identity over the normalized `(seconds, nanos)`
    representation — `PT1H30M` equals `PT90M`; `hashCode` derives from those two fields. [source: 11]

21. Temporal `Temporal.Duration.valueOf` throws (no relational comparison on instances); all
    ordering/rounding/totaling goes through static `Temporal.Duration.compare`/`round`/`total`.
    [source: 3 — duration.html]

### Durations: dual timelines and mixed units

22. Java's rule for adding durations to zoned date-times: *date-based* units (years, months,
    days) operate on the local timeline (keeping wall-clock time), *time-based* units
    (hours, minutes, seconds) operate on the exact timeline; `until` mirrors the split. [source: 10]

23. Luxon documents the same split: adding hours/minutes/seconds/ms changes the timestamp by
    exactly that many milliseconds, while adding days/months/years shifts the calendar
    "accounting for DSTs"; explicitly "dt.plus({hours: 24}) may result in a different time
    than dt.plus({days: 1}) if there's a DST shift in between". [source: 13]

24. moment documents the same split for `add`: "If you are adding years, months, weeks, or
    days, the original hour will always match the added hour"; adding 24 *hours* across DST
    changes the hour (5:00 → 6:00 example), while adding 1 *day* keeps it. [source: 12 — momentjs.com/docs, "Adding Time"]

25. Temporal requires an explicit `relativeTo` for any operation that mixes calendar units:
    `add`/`subtract` with nonzero years/months/weeks *throw* without it; `compare`/`round`/
    `total` likewise need it. Calendar days are otherwise treated as 24 hours; only a
    *zoned* `relativeTo` makes day arithmetic DST-aware (their documented example has DST
    flipping the sign of a `compare`). Temporal's default rounding mode is `halfExpand`.
    [source: 3]

26. Java `Duration.toString` emits ISO-8601 (`PT4H30M`) and deliberately **never** prints
    days: the javadoc states multiples of 24 hours are not output as days "to avoid confusion
    with Period"; zero prints `PT0S`; sign is uniform across sections. [source: 11]

27. moment's month addition clamps out-of-range days: Jan 31 + 1 month = Feb 28. [source: 12 — momentjs.com/docs, "Adding Time"]

### Parsing

28. date-fns `parse` is format-string-driven and strict about the *whole* string: any parse
    failure, non-matching literal, or leftover non-whitespace input returns `Invalid Date`;
    incompatible token combinations (e.g. `HH` with `a`) and unescaped latin letters in the
    format throw `RangeError`. [source: 14]

29. date-fns `parse` fills unspecified fields from a required `referenceDate`, assigns
    parsed values in descending unit priority, guesses two-digit-year centuries by proximity
    to the reference date, and gates dangerous tokens (`YYYY`/`DDD` week-year/day-of-year
    confusion) behind opt-in flags that throw by default. Tokens follow Unicode TR-35 with
    date-fns-specific additions (`i`, `I`, `R`, `o`, `P`, `p`). [source: 14]

30. moment's default parsing is *forgiving* (non-strict): unmatched trailing input is
    ignored; strict parsing is opt-in per-format, and the docs themselves recommend strict
    parsing. [source: 12 — momentjs.com/docs, "Asymmetric parsing"]

31. Temporal parses via `.from()` on each type; an offset present in the source string is
    *ignored* by non-exact types (e.g. `PlainDateTime.from('2019-02-19T00:00-03:00')` drops
    the offset), so creating an exact value later can still be ambiguous. [source: 1]

### Formatting and serialization

32. RFC 3339 (a profile of ISO 8601 for Internet use) requires a UTC offset (naive local
    time is unacceptable on the wire), requires the `T` separator (a space may be used for
    readability by humans), permits lowercase `t`/`z` while generators SHOULD uppercase them,
    allows arbitrary fractional-second digits, allows leap-second `:60`, defines `-00:00` as
    "local offset unknown" (distinct from `+00:00` = UTC), and guarantees lexicographic sort
    order equals chronological order for complete representations. [source: 4]

33. ISO 8601 fixes element order as year, month, day, hour, minutes, seconds, milliseconds,
    with `YYYY-MM-DD` as the date form; it covers date, time of day, UTC, local time with
    offset, intervals, and recurring intervals. The current editions are ISO 8601-1:2019
    (basic rules) and ISO 8601-2:2019 (extensions); the full text is paywalled. [source: 5]

34. CLDR's week data is *not* universal: `firstDay` differs by territory (001=Monday,
    US=Sunday, AF/BH/DJ=Saturday, …), `minDays` differs (001=1, US=1, most of EU=4), and the
    standard weekend is Saturday–Sunday for region 001. Week-start and week-of-year rules are
    therefore locale data, not a constant. [source: 6]

35. Temporal's `duration.toString` is ISO 8601, `toLocaleString` delegates to
    `Intl.DurationFormat` (falling back to ISO), and duration comparison/rounding need a
    relativeTo context for calendar units (see finding 25). [source: 3]

36. IANA tzdb explicitly recommends **numeric offsets instead of abbreviations** for new
    uses: "these abbreviations are ambiguous in practice: e.g., CST means one thing in China
    and something else in North America … To avoid ambiguity, use numeric UT offsets like
    -0600 instead of time zone abbreviations like CST." [source: 9]

### Humanization

37. moment's `duration.humanize()` renders a *single* unit chosen by locale thresholds
    (defaults: >45s counts as a minute, >22h as a day, …), tunable via
    `relativeTimeThreshold` and `relativeTimeRounding`. [source: 12 — momentjs.com/docs, "Humanizing a duration"]

38. python-humanize's `naturaldelta` also collapses to the single largest sensible unit
    (1001 s → "16 minutes"), while `precisedelta` renders compound output
    ("2 days, 1 hour and 33.12 seconds") with `minimum_unit`/`suppress` controls. [source: 17]

39. pretty-ms renders a compound sequence up to the largest unit (e.g. `1337000000` →
    "15d 11h 23m 20s") with `compact`/`verbose`/`colonNotation`/`secondsDecimalDigits`
    options; it has **no months/years** — days are always 24 h. [source: 18]

40. vercel/ms renders a *single* unit, treats `y` = 31557600000 ms (365.25 days), and offers a
    `long` option for full names. [source: 19]

41. `@date-fns/tz` shows the classic JS pitfall it exists to fix: adding 2 hours to
    `2022-03-13T00:00` in the *system* zone across US spring-forward yields 03:00 local,
    whereas a `TZDate` pinned to `Asia/Singapore` yields 02:00; it also provides `tzScan`
    for transition enumeration and a `{ in: tz }` context option for zone-aware comparisons
    in date-fns v4. [source: 15]

## Conventions worth adopting or citing

- **DST default = earlier-on-overlap / shift-forward-on-gap** (findings 6, 7, 12, 13, PEP 495
  finding 11): this is the ecosystem consensus — Temporal's `'compatible'` explicitly claims
  moment/Luxon/date-fns/legacy-Date/RFC 5545 parity, Java and Luxon match, and pandas is the
  outlier (raise by default, finding 15). Matches the spec's "ambiguous→earliest,
  nonexistent→shift forward" policy; cite finding 7 as the citable norm.
- **Explicit disambiguation knobs at the API seam** (6, 15): even where defaults are chosen,
  established libraries expose `reject`/`NaT`/`earlier`/`later` overrides — supports the
  spec keeping a strict default but an escape hatch.
- **Exact-time vs wall-clock dual timelines** (1, 22, 23, 24, 25): Java's date-based/local vs
  time-based/exact split is mirrored identically by Luxon and moment, and formalized by
  Temporal's separate types — citable backing for the spec's "calendar units applied
  per-unit, time units absolute" rule and for keeping calendar dates separate from instants.
- **Wall-clock ≠ elapsed arithmetic** (8, 23): Temporal's own example (PT2H wall vs PT1H
  real) is a ready-made citation for why duration equality must be magnitude-based and why
  `1h 30m == 90m` (finding 20, Java `Duration.equals`) is the right model.
- **Magnitude-identity duration equality** (20, 26): Java normalizes to (seconds, nanos) and
  never emits `P2D` (days) in `Duration.toString` to avoid Period confusion — supports the
  spec's magnitude-identity equality and keeping date-component durations out of the exact
  duration type.
- **Strict parsing at public seams** (28, 29, 15): date-fns returns `Invalid Date` on any
  leftover input and throws on bad format strings; pandas raises by default — citable
  contrast to moment's forgiving default (30) which the moment docs themselves walk back.
- **Naive/aware never-equal and TypeErrors** (2, 3, 4): Python's hard separation, plus its
  admission that only UTC is universally sane (5), backs the spec's naive→UTC-on-parse rule
  and separate calendar-date type.
- **Numeric offsets, not abbreviations, on the wire** (36, 32): IANA's own guidance plus
  RFC 3339's offset requirement support serializing offset numbers (and zone IDs like
  Temporal's `[America/Los_Angeles]`, finding 9/10), never `CST`.
- **Week rules are data, not constants** (34): CLDR per-territory `firstDay`/`minDays`
  justifies the spec's configurable week convention (ISO Monday weeks as the default, with
  locale-aware alternatives out of scope or pluggable).
- **RFC 3339 profile for output** (32): uppercase `T`/`Z`, mandatory offset, space-permitted-
  for-humans-only, `-00:00` unknown-offset distinction — a citable serialization spec.
- **Offset-vs-zone conflicts need an explicit policy** (9, 10): Temporal's `offset` option and
  its "stored data invalidated by tz-rule change" example is the citable precedent for how
  the spec should treat persisted zoned strings whose zone rules moved (Temporal: reject by
  default).
- **Humanize: single-unit vs compound are different products** (37, 38, 39, 40): moment/ms
  pick one unit, pretty-ms/humanize `precisedelta` compound them; pretty-ms/vercel-ms
  conventions (days=24h, y=365.25d) are citable if the spec's humanizer defines units.
- **DST-safe day arithmetic needs a zone context** (23, 24, 41, 25): Luxon/moment docs and
  `@date-fns/tz`'s 2h example all document `+24h ≠ +1day` across transitions — citable
  justification for requiring a zone for calendar-unit arithmetic.

## Known regrets / footguns

- **moment**: in maintenance mode; forgiving default parse (30); nonexistent-time parse
  result is *browser-dependent* (14); month clamping silently changes day-of-month (27);
  `humanize` collapses to one unit with tunable thresholds (37) — surprises when values
  cross thresholds.
- **IANA tzdb**: `TZ` is process-global (16); `tm_isdst`/`tzname`/`daylight` are vestigial
  and `tm_isdst` disambiguation doesn't work for geographical zones (16); abbreviations are
  ambiguous across countries (36) — historical source of the "which CST?" bug class.
- **Python**: naive/aware mixed operations raise `TypeError` (3) — a frequent runtime
  footgun; same-tzinfo comparisons silently ignore fold/tzinfo (4); cross-zone equality
  depends on fold (4); famous advice often quoted as "better to raise an exception than
  return a wrong result" is *not* in the current docs (see Unverified).
- **pandas**: `raise` defaults (15) mean messy data loads need explicit
  `ambiguous='NaT'`/`nonexistent='shift_forward'` — strictness pushed onto every call site.
- **Luxon**: ambiguous-time resolution is an *internal offset guess*, not documented as a
  stable contract (13) — the docs' "makes no promises" claim could not be re-verified (see
  Unverified); `wasHole` is easy to miss.
- **Temporal**: `valueOf` throws (21) so naive `<`/`>` on durations panics; default
  `offset:'reject'` (9) means previously-valid stored strings start throwing after tzdb
  updates (the Brazil example is the docs' own cautionary tale); `from()` silently drops
  offsets on non-exact types (31); `add` throws on calendar units without `relativeTo` (25).
- **Java**: `compareTo` chaining instant → local → zone → chronology (18) means `compare`
  consistency depends on chronology choice; `Duration.toString` omitting days (26) can
  surprise readers expecting `P2D`.
- **date-fns**: century guessing from `referenceDate` (29); protected tokens throw unless
  opted in (29); `parse` returns `Invalid Date` (a NaN Date object) rather than an error
  type (28) — errors are easy to propagate silently.
- **vercel/ms**: `1y = 365.25 d` (40) drifts vs calendar years; **pretty-ms**: days are
  always 24 h (39), so "1d" ≠ calendar day across DST; both are single-/compound-output
  formatters, not calendars.
- **JS ecosystem**: `new Date()` local-zone arithmetic across DST is the canonical bug that
  `@date-fns/tz` exists to patch (41).

## Unverified

- Luxon documentation phrasing for ambiguous times ("makes no promises … sometimes picks one
  and sometimes the other") — the *code* behavior (offset-guess pick + `getPossibleOffsets`)
  is verified (13), but the exact docs sentence could not be re-fetched this session
  (luxon `docs/help.md` 404; site is an SPA).
- Python's often-quoted guidance "it is better to raise an exception than to return a wrong
  result" — searched current `datetime` docs and it does **not** appear there (it may come
  from a different Python doc or older version); do not cite it as a datetime-docs quote.
- ECMA-402 hour-cycle values (`h11`/`h12`/`h23`/`h24`) — not fetched this session; any
  hour-cycle claims should be verified against ECMA-402/`Intl` docs before citing.
- ISO 8601-2:2019 *content* (week-date forms, ordinal dates, negative durations, etc.) —
  only the ISO catalog page was readable (5); the standard text is paywalled, so detailed
  8601-1 vs 8601-2 formatting rules were not verified from the standard itself.
- moment-timezone's own ambiguous-parsing docs (`parsing-ambiguous-inputs`) — referenced by
  Temporal's docs (1) but not fetched directly this session.
