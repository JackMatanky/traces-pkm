# 06: Template parity — both format dialects + durationformat

**What to build:** Template authors can format dates in the dialect their copied recipe uses and can render human-readable durations: `durationformat` produces "3 days, 4 hours"; common moment tokens (`YYYY MM DD HH mm ss`, `Do`/`S`, `dddd`/`ddd`/`MMM`/`MMMM`, bracket literals) work; an unsupported token fails with an error that **names the token**, says the input was treated as moment-dialect, and points at the supported list. Invalid-pattern detection exists once. The previously dead `format_with` surface gains a real consumer.

**Blocked by:** 04, 05 (spec's execution order sequences parity 05 → 06 → 07, all after the seam chain; 03's duration parts and 01's display dialect ride transitively).

**Status:** ready-for-agent

Skills: `rust-unit-testing`, `rust-integration-testing`, `rust-skills`, `rust-doc`. Rules: `type-numeric-fmt` + `type-display-vs-debug` (N19's exponent-threshold dialect governs only `DurationSeconds`'s `Display` fallback rendering; `durationformat` is the humanizer of parsed durations); the translator emits only a subset of chrono's documented strftime table (`chrono::format::strftime`, linked via intra-doc) and adds NO parallel specifier validator — invalid patterns are chrono's `DelayedFormat` error; `%+` is forbidden (chrono advises against it); docs note `%Z` = offset only, `%S` may be 60 (leap second), week tokens are `%V`/`%G`; doctests maintained (`cargo test --doc`); translator is a pure function at the template seam — no `Dialect` trait (review §5.7); strictness deliberate — never silently mistranslate an ambiguous token (D-b). Design record: `../review.md` §4 (D-b), §9 (S4+S8); spec stories 7–9.

- [ ] `durationformat` filter renders human-readable durations (B14), spec at review §3 Dataview L6803–6825 — compound unit-list display follows the humanizer convention catalogued in ../research/general-temporal-libraries.md (magnitude-based, e.g. pretty-ms/`humanize` precedents)
- [ ] `format_with` has a live consumer (first Decision-C wiring); shared renderer surfaces chrono's invalid-pattern error once, call sites map errors to their own types (N21)
- [ ] Format bindings per spec: documented grammar is `chrono::format::strftime` (intra-doc linked in touched rustdoc); `%+` is never emitted — pinned by test; invalid-pattern errors come from chrono's `DelayedFormat` only — no parallel specifier validator — with a test asserting an invalid pattern surfaces chrono's error mapped at the call site
- [ ] Format rustdoc notes `%Z` prints only an offset, `%S` may render 60 (leap second), and week numbers use `%V`/`%G` (ISO) — never `%U`/`%W`
- [ ] Moment-dialect translation scope 1 at the template seam: bracket literals, `YYYY MM DD HH mm ss` family, `Do`/`S`, `dddd`/`ddd`/`MMM`/`MMMM` (T4); core `format_with` keeps strftime as its only grammar
- [ ] Unsupported moment token → error naming the offending token, identifying moment-dialect, listing where supported tokens are documented
- [ ] `durationformat` and translator tested through the template seam only; doctests for changed public examples pass
- [ ] `mise run verify` green
