# 04: Recognize with precision — classify entry + precision hoist

**What to build:** Each domain gets exactly one recognition entry point whose contract tells callers "not mine / mine-but-invalid / valid", so no call site can forget a guard; and a recognized string keeps its precision through arithmetic and re-formatting, so a `YYYY-MM` note never silently becomes a day. Demo contract: a `YYYY-MM` value classifies, shifts by a month, and re-formats as year-month — and the template engine's private precision/parsed-date types are gone.

**Blocked by:** 03 (classify/parse paths built on the settled duration shape and calendar owner).

**Status:** ready-for-agent

Skills: `rust-skills`, `rust-unit-testing`, `rust-doc`, `codebase-design`. Rules: `api-parse-dont-validate` (N7, N15, N23), `api-must-use` (classify), `type-enum-states` (`Precision` — not in equality); D11 doc written per `rust-doc` conventions. Design record: `../review.md` §2.2, §5.3–5.4, §9 (S5+S7).

- [ ] `classify` exists on both domains with the trichotomy contract: `None` = cheap O(1) shape gate (no allocation), `Some(Err)` = detail, `Some(Ok)` — declared `#[must_use]`, tested through the interface
- [ ] Guard functions (`can_start`/`is_iso_shape`/`has_four_digit_year`) are private; the `can_start && parse` call sites and the coercion-cascade protocol copy are retired to `classify` (N7, N15, N23); `TextShape` becomes a caller of `classify`, not a fourth recognition fragment (H3)
- [ ] Format enums own the cascade loop once (`parse_any`-style); the four duplicated cascades and the `"1h 1x"` divergence are gone (N8, N20, D1); ISO-prefix scan deduplicated (D6)
- [ ] `Precision` (`YearMonth` | `Date` | `DateTime`) hoisted to the recognition result; engine's `ParsedDate`/`DatePrecision`/`format_precise` delegate then deleted (D10); precision never participates in value equality (N16)
- [ ] Fractional truncation fixed once behind the hoisted precision (D3)
- [ ] `date_add` unit documentation widened to any `DurationUnit` spelling, with the sub-day-on-date-only caveat, landing with the precision fix (D11)
- [ ] `YYYY-MM` precision survives arithmetic round-trip (demo contract above)
- [ ] `mise run verify` green
