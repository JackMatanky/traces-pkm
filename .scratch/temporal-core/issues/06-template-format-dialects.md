# 06: Template parity — both format dialects + durationformat

**What to build:** Template authors can format dates in the dialect their copied recipe uses and can render human-readable durations: `durationformat` produces "3 days, 4 hours"; common moment tokens (`YYYY MM DD HH mm ss`, `Do`/`S`, `dddd`/`ddd`/`MMM`/`MMMM`, bracket literals) work; an unsupported token fails with an error that **names the token**, says the input was treated as moment-dialect, and points at the supported list. Invalid-pattern detection exists once. The previously dead `format_with` surface gains a real consumer.

**Blocked by:** 03 (duration parts/formatting settled; display dialect from 01/03 final).

**Status:** ready-for-agent

Skills: `rust-unit-testing`, `rust-integration-testing`, `rust-doc`, `verification-before-completion`. Rules: doctests maintained (`cargo test --doc`); translator is a pure function at the template seam — no `Dialect` trait (review §5.7); strictness deliberate — never silently mistranslate an ambiguous token (D-b). Design record: `review.md` §4 (D-b), §9 (S4+S8); spec stories 7–9.

- [ ] `durationformat` filter renders human-readable durations (B14), spec at review §3 Dataview L6803–6825
- [ ] `format_with` has a live consumer (first Decision-C wiring); shared renderer owns invalid-pattern detection once, call sites map errors to their own types (N21)
- [ ] Moment-dialect translation scope 1 at the template seam: bracket literals, `YYYY MM DD HH mm ss` family, `Do`/`S`, `dddd`/`ddd`/`MMM`/`MMMM`; core `format_with` keeps strftime as its only grammar
- [ ] Unsupported moment token → error naming the offending token, identifying moment-dialect, listing where supported tokens are documented
- [ ] `durationformat` and translator tested through the template seam only; doctests for changed public examples pass
- [ ] `mise run verify` green
