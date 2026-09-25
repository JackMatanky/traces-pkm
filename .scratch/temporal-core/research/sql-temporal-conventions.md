# SQL & Query-Engine Conventions for Dates, Times, Durations, Zones, and Ordering

Research-only findings. Every claim below carries a source marker `[source: …]`. Anything
asserted but not primary-source-confirmed is listed under **Unverified** at the end.
Empirical checks (marked `local PostgreSQL 18.6`) were run against a temporary PostgreSQL
cluster, session `TimeZone = UTC`; exact statements are reproduced in the **Empirical
appendix**.

---

## Sources

| # | Source | What it is |
|---|--------|-----------|
| S1 | https://www.postgresql.org/docs/current/datatype-datetime.html | PostgreSQL 18 manual, ch. 8 — date/time/interval types, session zone, interval output styles |
| S2 | https://www.postgresql.org/docs/current/functions-datetime.html | PostgreSQL 18 manual — date/time functions, `extract`/`date_trunc`/`age`, SQL-standard time functions |
| S3 | https://www.postgresql.org/docs/current/sql-select.html | PostgreSQL 18 manual — `ORDER BY` clause, default null ordering |
| S4 | https://www.postgresql.org/docs/current/functions-comparison.html | PostgreSQL 18 manual — comparison operators, three-valued logic, `IS [NOT] DISTINCT FROM` |
| S5 | https://raw.githubusercontent.com/postgres/postgres/master/src/backend/utils/adt/datetime.c | PostgreSQL source — time-zone transition handling (`DetectDST`-era comment at line ~1727) |
| S6 | https://raw.githubusercontent.com/postgres/postgres/master/src/backend/utils/adt/timestamp.c | PostgreSQL source — `interval_cmp_value` (interval comparison model) |
| S7 | local PostgreSQL 18.6 cluster (empirical) | Statements in the Empirical appendix; DST/zone/week/interval results observed on PG 18.6 |
| S8 | https://sqlite.org/lang_datefunc.html | SQLite manual — date/time functions, modifiers, `timediff`, caveats |
| S9 | https://sqlite.org/datatype3.html | SQLite manual — storage classes and `ORDER BY` sort order |
| S10 | https://trino.io/docs/current/language/types.html | Trino manual — data types incl. `TIMESTAMP [WITH TIME ZONE]`, interval types |
| S11 | https://trino.io/docs/current/functions/datetime.html | Trino manual — date/time functions, interval arithmetic, `date_trunc`, `human_readable_seconds` |
| S12 | https://trino.io/docs/current/sql/select.html | Trino manual — `SELECT`/`ORDER BY`, default null ordering |
| S13 | https://duckdb.org/docs/current/sql/data_types/interval | DuckDB docs — `INTERVAL` type: three basis units, comparison rules, arithmetic (current path; `/docs/data_types/interval.html` now 404s) |
| S14 | https://duckdb.org/docs/data_types/timestamp.html | DuckDB docs — `TIMESTAMP`/`TIMESTAMPTZ` storage and time-zone binning |
| S15 | https://duckdb.org/docs/query_syntax/orderby.html | DuckDB docs — `ORDER BY`, default null ordering, contrast with PostgreSQL |
| S16 | https://dev.mysql.com/doc/refman/8.0/en/datetime.html | MySQL 8.0 refman §13.2.2 — `DATE`/`DATETIME`/`TIMESTAMP`, storage conversion, ranges, invalid-value handling |
| S17 | https://dev.mysql.com/doc/refman/8.0/en/date-and-time-functions.html | MySQL 8.0 refman §14.7 — `DATEDIFF`, `TIMEDIFF`, `TIMESTAMPDIFF`, `DATE_ADD`, `UNIX_TIMESTAMP` |
| S18 | https://dev.mysql.com/doc/refman/8.0/en/time-zone-support.html | MySQL 8.0 refman — time zone support, `time_zone` variable, `CONVERT_TZ` |
| S19 | https://dev.mysql.com/doc/refman/8.0/en/order-by-optimization.html | MySQL 8.0 refman — `ORDER BY` optimization (checked for null-ordering statement) |
| S20 | https://cloud.google.com/bigquery/docs/reference/standard-sql/data-types | BigQuery standard SQL — data types; includes explicit DST ambiguity/round-trip rules |
| S21 | https://cloud.google.com/bigquery/docs/reference/standard-sql/timestamp_functions | BigQuery — `FORMAT_TIMESTAMP`, `TIMESTAMP_TRUNC` etc. with `time_zone` argument |
| S22 | https://datafusion.apache.org/user-guide/sql/data_types.html | Apache DataFusion docs — SQL↔Arrow type mapping for date/time/interval |
| S23 | https://blacksmithgu.github.io/obsidian-dataview/queries/data-commands/ | Dataview docs (attempted — no null-ordering statement found; see Unverified) |

*Access notes:* MySQL reference pages return 403 to generic fetchers and were retrieved with a
Googlebot user agent. PostgreSQL source lives in `src/backend/utils/adt/` (`interval.c` no
longer exists; interval comparison is in `timestamp.c`). DuckDB's interval docs moved to
`/docs/current/sql/data_types/interval`.

---

## Findings

### Temporal model and types

1. PostgreSQL offers six date/time types — `timestamp`, `timestamptz`, `date`, `time`,
   `timetz`, `interval`. Writing bare `timestamp` is defined as `timestamp without time zone`,
   which the manual states is what the SQL standard means. [source: S1]
2. `timestamptz` values are held as UTC instants and rendered in the session time zone;
   `SET TIME ZONE` changes the session zone and `RESET TIME ZONE` restores the configured
   default. Naive `timestamp` values are *not* affected by the session zone at all —
   empirically, `SET TIME ZONE 'Asia/Tokyo'` changes `'…+00'::timestamptz` to a `+09` display
   while `'2024-01-01 00:00:00'::timestamp` prints identically under both zones.
   [source: S1; local PostgreSQL 18.6]
3. A PostgreSQL `interval` is a three-part value: **months, days, microseconds** — months and
   days are calendar quantities, the remainder is fixed-length time. [source: S1]
4. Interval literals must list units in matching order, and the manual notes the SQL standard
   permits only the short forms `'1-2'` (year-month) and `'3 4:05:06'` (day-time) as
   implicit forms; `interval '1' year` / `interval '1' day` are unit multipliers.
   [source: S1]
5. The SQL standard's year-month vs. day-time interval split is visible in Trino's two
   interval types: `INTERVAL YEAR TO MONTH` and `INTERVAL DAY TO SECOND` (second form takes a
   precision, e.g. `DAY TO SECOND(3)`). [source: S10]
6. Trino distinguishes wall-clock and instant semantics explicitly: `TIMESTAMP(P)` is
   "calendar date and time of day **without** a time zone… effectively a combination of DATE
   and TIME(P)", while `TIMESTAMP(P) WITH TIME ZONE` "includes the date and time of day … and
   with a time zone. Values of this type are rendered using the time zone from the value" —
   the zone is part of the value (UTC, `±hh:mm` offsets, or IANA names), not session state.
   [source: S10]
7. BigQuery models three temporal layers: `DATE`, `DATETIME` ("a Gregorian date and a time, as
   they might be displayed on a watch, **independent of time zone**"), and `TIMESTAMP` ("an
   absolute point in time, independent of any time zone or convention such as daylight saving
   time (DST)"). [source: S20]
8. DataFusion maps `DATE → Date32`, `TIME → Time64(Nanosecond)`,
   `TIMESTAMP → Timestamp(Nanosecond, None)` (the `None` = no zone), and
   `INTERVAL → Interval(IntervalMonthDayNano)` — months/days/nanoseconds, no zone-bearing
   timestamp type in the type table. [source: S22]
9. SQLite has no dedicated date/time storage class; temporal values are stored as ISO-8601
   text, Julian day, or unix epoch and interpreted via modifiers. [source: S8, S9]

### Instants, zones, and DST

10. MySQL converts `TIMESTAMP` "from the current time zone to UTC for storage, and back from
    UTC to the current time zone for retrieval. (This does not occur for other types such as
    `DATETIME`.)" The session `time_zone` drives the conversion. `TIMESTAMP` range:
    `'1970-01-01 00:00:01' UTC` … `'2038-01-19 03:14:07' UTC`; `DATETIME` range
    `'1000-01-01'` … `'9999-12-31'`. [source: S16, S18]
11. PostgreSQL DST behavior (empirical, PG 18.6): spring-forward **gap** local
    `2024-03-10 02:30 America/New_York` → `07:30 UTC` (= 03:30 EDT, i.e. **shifted forward**);
    fall-back **ambiguous** local `2024-11-03 01:30 America/New_York` → `06:30 UTC`
    (= 01:30 EST, i.e. the **later/second** occurrence); Berlin ambiguous `2024-10-27 02:30`
    → `01:30 UTC` (= 02:30 CET, later occurrence). PostgreSQL source carries explicit
    before/after-interpretation preference logic for transitions: "It's an invalid or
    ambiguous time due to timezone transition. In a spring-forward transition, prefer the
    'before' interpretation; in a …". [source: S7; S5]
12. PostgreSQL itself documents that calendar and clock arithmetic diverge across DST: the
    manual's interval section shows `'2005-04-02 12:00 MST' + interval '1 day'` differing from
    `+ interval '24 hours'` because a DST change intervened. [source: S1]
13. BigQuery documents **both** DST edge rules explicitly: rendering a UTC instant into a zone
    during spring-forward — "When there's ambiguity in how to represent a civil time in a
    particular timezone because of DST, the later time is chosen" (e.g. `10:30 UTC` →
    `03:30 UTC-7`); parsing a civil time during fall-back — "During the transition from DST to
    standard time, one hour is repeated. A civil time that shows a time during that hour is
    treated as if it's the **earlier instance** of that time" (`2024-11-03 01:30
    America/Los_Angeles` → the DST/earlier instance). [source: S20]
14. SQLite's `localtime` modifier is only defined for values already in local time — applying
    it to other values is "undefined behavior" (non-UTC inputs); DST rules come from the
    platform C library's `localtime`, and days are exactly 86400 s (no leap seconds).
    Month/year shifting has "no consensus" semantics, so the default modifier rounds
    **up (ceiling, later date)** and a `floor` modifier exists for the other choice.
    [source: S8]
15. MySQL `UNIX_TIMESTAMP()` ↔ `FROM_UNIXTIME()` are not bijective across DST: "it is possible
    for `UNIX_TIMESTAMP()` to map two values that are distinct in a non-UTC time zone to the
    same Unix timestamp value" — round-tripping loses information. Named-zone conversions
    (`CONVERT_TZ`) depend on the server's zone tables. [source: S16, S18]
16. DuckDB `TIMESTAMPTZ` stores the number of microseconds since the unix epoch as INT64 with
    no zone attached; "timestamp arithmetic, binning, and string formatting for
    this type are performed in a configured time zone" (default: system zone). Plain
    `TIMESTAMP` stores the same INT64 but follows UTC rules; the docs note such values are
    commonly local observations "recorded in an unspecified time zone". [source: S14]
17. PostgreSQL time-of-day functions are anchored to transaction start: `now()` and the
    SQL-standard `CURRENT_TIMESTAMP`/`LOCALTIMESTAMP` "all return values based on the start
    time of the current transaction". [source: S2]

### Nulls and ordering

18. PostgreSQL's `ORDER BY` default is `NULLS LAST` for `ASC` and `NULLS FIRST` for `DESC` —
    "default is to act as though nulls are larger than non-nulls". [source: S3]
19. Trino's default: "The default null ordering is **NULLS LAST, regardless of the ordering
    direction**." (`NULLS FIRST|LAST` is available explicitly.) [source: S12]
20. DuckDB defaults to nulls-last in both directions and explicitly contrasts itself with
    PostgreSQL's nulls-largest rule. [source: S15]
21. SQLite's default sort order is the storage-class order: `NULL` sorts **first**, then
    numeric, then `TEXT`, then `BLOB`. [source: S9]
22. PostgreSQL comparisons are three-valued: "Ordinary comparison operators yield null
    (signifying 'unknown')" when either input is null; `IS [NOT] DISTINCT FROM` is the
    null-safe form. [source: S4]

### Duration equality vs. calendar application

23. PostgreSQL interval **comparison** is a fixed linear model: "Interval comparison is based
    on converting interval values to a linear representation … with days assumed to be always
    24 hours and months assumed to be always 30 days." Empirically on PG 18.6:
    `interval '1 hour 30 min' = interval '90 minutes'` → true; `interval '1 day' =
    interval '24 hours'` → true; `interval '1 month' = interval '30 days'` → true;
    `interval '1 month' = interval '31 days'` → false; `interval '1 year' = interval '360
    days'` → true; `interval '1 year' = interval '365 days'` → false. [source: S6; S7]
24. DuckDB documents the same rule and its consequence: "For equality and ordering comparisons
    only, the total number of microseconds in an `INTERVAL` is computed by converting the days
    basis unit to 24 × 60 × 60 × 1e6 microseconds and the months basis unit to 30 days … As a
    result, `INTERVAL`s can compare equal even when they are functionally different, and the
    ordering of `INTERVAL`s is not always preserved when they are added to dates or
    timestamps" — e.g. `INTERVAL 30 DAYS = INTERVAL 1 MONTH` yet
    `DATE '2020-01-01' + INTERVAL 30 DAYS != DATE '2020-01-01' + INTERVAL 1 MONTH`.
    [source: S13]
25. DuckDB's `INTERVAL` is stored as three basis units (months, days, microseconds); "units
    that aren't `months`, `days`, or `microseconds` are converted to equivalent amounts in the
    next smaller of these three basis units" at construction — mirroring PostgreSQL's model.
    The rationale is spelled out: "a day doesn't correspond to a fixed amount of microseconds
    (days can be 25 hours or 23 hours long because of daylight saving time)" and "February has
    fewer days than March". [source: S13]
26. PostgreSQL **calendar application** uses real units, separate from the equality model:
    adding a month clamps to the last valid day (documented `2005-01-31 + 1 month →
    2005-02-28`-style behavior; MySQL shows `DATE_ADD('2024-03-31', INTERVAL 1 MONTH) →
    2024-04-30`); `justify_hours`/`justify_days`/`justify_interval` renormalize (30-day spans
    to months, ±24 h to days); `extract` normalizes components — `EXTRACT(MINUTES FROM
    INTERVAL '80 minutes')` is documented as **20**, not 80. [source: S1, S2; S17]
27. MySQL month arithmetic clamps identically: `DATE_ADD('2024-03-30', INTERVAL 1 MONTH)` and
    `DATE_ADD('2024-03-31', INTERVAL 1 MONTH)` both → `2024-04-30` (same for
    `TIMESTAMPADD(MONTH, …)`). [source: S17]
28. DuckDB's difference operators deliberately degrade precision: `DATE − DATE` returns an
    **integer** day count; `TIMESTAMP − TIMESTAMP` returns an interval "with only the days and
    microseconds components". Documented round-trip failure:
    `TIMESTAMP '2000-02-01' + (TIMESTAMP '2000-02-01' - TIMESTAMP '2000-01-01')` →
    `'2000-03-03'`, **not** `'2000-03-01'`. [source: S13]
29. DuckDB also documents that its duration helpers disagree with each other on the same
    inputs: `datediff('day', TIMESTAMP '2020-01-01 01:00:00', TIMESTAMP '2020-01-02 00:00:00')`
    → `1`, but `datepart('day', TIMESTAMP '2020-01-02 00:00:00' - TIMESTAMP '2020-01-01
    01:00:00')` → `0`. [source: S13]
30. Trino interval/date arithmetic: adding a month to a month-end date clamps (documented
    `2012-10-31 + interval '1' month → 2012-11-30`); `date_trunc('week', …)` is Monday-based
    (`2001-08-22` → `2001-08-20`, an ISO week); `date_diff(unit, t1, t2)` returns "t2 − t1 in
    terms of unit"; `AT TIME ZONE` converts in both directions between zone-less and
    zone-qualified timestamps. [source: S11]
31. PostgreSQL week handling is ISO-8601/Monday: `extract(week from date)` returns the ISO week
    number (empirically `2001-08-22` → week `34`), and `date_trunc('week', …)` truncates to
    the Monday (empirically → `2001-08-20`). [source: S2; S7]
32. MySQL's duration functions are asymmetric by design: `DATEDIFF(e1, e2)` "returns e1 − e2
    expressed as a value in days **from one date to the other**" — date parts only:
    `DATEDIFF('2007-12-31 23:59:59', '2007-12-30')` → `1`; `TIMEDIFF(e1, e2)` "returns e1 − e2
    expressed as a **time value**" and "is limited to the range allowed for `TIME` values";
    `TIMESTAMPDIFF(unit, e1, e2)` "Returns datetime_expr2 − datetime_expr1" as an **integer**
    in the named unit ("`TIMESTAMPDIFF()` and `UNIX_TIMESTAMP()`, both of which return
    integers"). [source: S17]
33. MySQL parsing is lenient by default: invalid `DATE`/`DATETIME`/`TIMESTAMP` values "are
    converted to the 'zero' value of the appropriate type (`'0000-00-00'` …) if the SQL mode
    permits this conversion" (behavior depends on strict/`NO_ZERO_DATE` modes); 2-digit years
    are expanded by fixed rule: `00–69 → 2000–2069`, `70–99 → 1970–1999`. [source: S16]
34. DuckDB builds its zone support on ICU: `TIMESTAMPTZ` binning/arithmetic/formatting run
    through ICU, zones are listed via `pg_timezone_names()`, and non-Gregorian calendars are
    available via `SET Calendar`; during fall-back a one-hour bin can legitimately be two
    hours long unless UTC-offset bins are used alongside. [source: S14]

### Human-facing formatting

35. Engine-provided humanizers: PostgreSQL offers interval output styles
    (`sql_standard`, `postgres`, `iso_8601`), `justify_*` renormalization, and `to_char`;
    Trino ships `human_readable_seconds()`; SQLite's `timediff()` "returns the difference
    between two date/time values as a human-readable string" with a documented
    "precision vs. human readability" trade-off (limited, coarse granularity by design).
    [source: S1, S11, S8]

---

## Notable conventions worth adopting or citing

- **UTC-instant storage + session-zone display, naive values zone-blind** (Findings 2, 10, 16):
  the project's model is exactly MySQL `TIMESTAMP` and DuckDB `TIMESTAMPTZ` semantics; the
  empirical PG result (naive timestamp ignores session zone) is a ready citation.
- **Spring-forward gap → shift forward** (Finding 11): PostgreSQL behaves the same way — a
  direct citation for the spec's rule.
- **Fall-back ambiguity → earliest instance** (Findings 11 vs 13): BigQuery's documented rule
  ("treated as if it's the earlier instance") matches the spec; PostgreSQL empirically picks
  the later instance. Cite BigQuery as precedent and PostgreSQL as the deliberate divergence.
- **Duration equality = magnitude identity; calendar application uses true units**
  (Findings 23–27): both PostgreSQL and DuckDB implement exactly this split, and DuckDB
  states the rationale (DST days, varying month lengths). `'1h 30m' == '90m'` is Finding 23.
  Caveat to resolve in the spec: both engines *also* equate `1 month = 30 days` in the
  magnitude model — the spec should say whether month is part of duration equality at all.
- **Null ordering is unsettled across engines** (Findings 18–21): PostgreSQL = nulls-largest,
  Trino/DuckDB = nulls-last, SQLite = nulls-first. The spec's "null never satisfies ordering"
  matches none of them — cite the table above as justification for a deliberate departure (or
  adopt nulls-last for familiarity, per Trino/DuckDB).
- **ISO Monday weeks** (Findings 30, 31): PostgreSQL `extract(week)`/`date_trunc('week')` and
  Trino's `date_trunc('week')` example both confirm ISO semantics — matches the spec.
- **Month-end clamping on calendar addition** (Findings 26, 27): identical in PostgreSQL and
  MySQL — matches "calendar application uses units".
- **Strict parse errors at public seams** (contrast Finding 33): MySQL's zero-value leniency is
  the canonical example of what not to do; cite it as motivation for the spec's strictness.
- **Humanizers exist in three engines** (Finding 35): precedent for a duration→string
  presentation layer (`justify_*`, `human_readable_seconds`, `timediff`).

---

## Surprises / anti-patterns engines regret

- **Equality that disagrees with arithmetic**: PostgreSQL and DuckDB both let
  `interval '1 month' = interval '30 days'` (Findings 23, 24), so ordering intervals doesn't
  survive adding them to dates — DuckDB documents this failure mode outright (Finding 24).
- **DuckDB's documented round-trip failure**: `date + (ts2 − ts1)` can land on the wrong date
  (`'2000-03-03'` vs `'2000-03-01'`) because timestamp differences degrade to days+µs
  (Finding 28); its own helpers disagree (`datediff` = 1 vs `datepart` = 0, Finding 29).
- **PostgreSQL's own DST example**: `+ interval '1 day'` ≠ `+ interval '24 hours'` across a
  transition (Finding 12) — the same source code must serve both duration and calendar paths.
- **MySQL `DATEDIFF` silently drops time-of-day** (`DATEDIFF('2007-12-31 23:59:59',
  '2007-12-30')` = 1) while `TIMEDIFF` returns a `TIME` and `TIMESTAMPDIFF` returns an integer
  in a caller-chosen unit — three subtraction functions, three result types (Finding 32).
- **MySQL zero-date leniency and 2-digit years** (Finding 33): invalid input becomes
  `'0000-00-00'`, and `00–69 → 2000s` while `70–99 → 1970s` — ambiguity moved into the data.
- **MySQL `TIMESTAMP`'s 2038 ceiling and non-bijective DST round-trip** (Findings 10, 15).
- **SQLite's `localtime` on non-local input is undefined behavior**, with DST rules delegated
  to the platform C library, and an admitted "no consensus" month-shift default of rounding
  up (Finding 14).
- **PostgreSQL `extract` normalization surprises**: `EXTRACT(MINUTES FROM INTERVAL '80
  minutes')` = 20 (Finding 26).
- **Two documented engines pick opposite sides of DST ambiguity** — BigQuery "earlier instance"
  vs PostgreSQL "later occurrence" (Findings 11, 13): a language-design choice with no
  consensus, worth stating explicitly in any spec.

---

## Unverified

1. **The SQL standard (ISO/IEC 9075) itself is paywalled** — the year-month vs day-time
   interval split (Finding 5), `timestamp` bare-name equivalence (Finding 1), and
   `CURRENT_TIMESTAMP`/`SET TIME ZONE` requirements (Findings 2, 17) are attested by engine
   manuals that cite the standard, not by reading the standard text.
2. **MySQL's default `ORDER BY` null placement**: no statement found in the reference pages
   fetched (`select.html`, `order-by-optimization.html`, live and via Wayback); not
   empirically tested (no local MySQL server).
3. **MySQL's rule for ambiguous/nonexistent local times** during named-zone conversion — the
   docs discuss DST only via `CONVERT_TZ` test examples, no stated preference (earlier vs
   later) for the repeated/missing hour.
4. **Dataview's null-greatest sort convention** (referenced by the spec): no null-ordering
   statement found in the fetched Dataview documentation pages (`queries/data-commands/`,
   `queries/differences-to-sql/`, `api/data-array/`) — possibly in JS-rendered or
   repo-wiki content not retrieved.
5. **BigQuery physical storage of `TIMESTAMP`** (UTC): the docs define it semantically as an
   absolute point independent of zones; the literal "stored in UTC" sentence was not
   captured from the page.
6. **MariaDB divergence from MySQL** on any of the above — MariaDB `select.html` /
   `order-by-optimization.html` were fetched but not compared.
7. **Leap-second handling** beyond DuckDB's explicit "no leap seconds, days are 86400 s"
   statement (Finding 14) — no other engine doc consulted addresses leap seconds.

---

## Empirical appendix (local PostgreSQL 18.6, session `TimeZone=UTC`)

Run with: `psql -h <socket> -p 55432 -U tester -d postgres -X` against a temporary cluster
(`initdb` + `pg_ctl start -o "-c timezone=UTC -p 55432"`).

```sql
-- DST gap (spring forward): 02:30 local does not exist -> shifted forward
SELECT '2024-03-10 02:30:00 America/New_York'::timestamptz;   -- 2024-03-10 07:30:00+00 (=03:30 EDT)
-- DST ambiguous (fall back): 01:30 occurs twice -> later (EST) chosen
SELECT '2024-11-03 01:30:00 America/New_York'::timestamptz;   -- 2024-11-03 06:30:00+00 (=01:30 EST)
SELECT '2024-10-27 02:30:00 Europe/Berlin'::timestamptz;      -- 2024-10-27 01:30:00+00 (=02:30 CET, later)
-- Interval magnitude equality (30-day months, 24-hour days)
SELECT interval '1 hour 30 min' = interval '90 minutes',      -- t
       interval '1 day' = interval '24 hours',                -- t
       interval '1 month' = interval '30 days',               -- t
       interval '1 month' = interval '31 days',               -- f
       interval '1 year' = interval '360 days',               -- t
       interval '1 year' = interval '365 days',               -- f
       interval '1 day 1 hour' > interval '24 hours';         -- t
-- Session zone: timestamptz re-renders, naive timestamp does not
SET TIME ZONE 'Asia/Tokyo';
SELECT '2024-01-01 00:00:00+00'::timestamptz;                -- 2024-01-01 09:00:00+09
SELECT '2024-01-01 00:00:00'::timestamp;                     -- 2024-01-01 00:00:00
RESET TIME ZONE;                                              -- back to configured default (UTC)
-- ISO weeks
SELECT date_trunc('week','2001-08-22'::timestamp),           -- 2001-08-20 00:00:00 (Monday)
       extract(week from '2001-08-22'::date);                -- 34 (ISO week number)
```
