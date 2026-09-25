# 01: Hardening — panic-free conversion, signed zero, honest display, error sources, serde, exports

**What to build:** The date and duration value types become safe and truthful under hostile or extreme input: nothing in parse/convert/display can panic, signed zero compares like zero, extreme magnitudes render honestly, deserialized YAML dates behave exactly like inline dates, errors carry their source chains, and the two domains' public surfaces are symmetric. Verifiable entirely through in-module tests at the value-type seam.

**Blocked by:** None (can start immediately).

**Status:** ready-for-agent

Skills: `rust-skills`, `rust-unit-testing`. Rules: `err-result-over-panic` (N1), `num-float-compare` (N3′), `type-display-vs-debug` + `type-numeric-fmt` (N14/N19), `serde-try-from-validate` + `api-parse-dont-validate` (N12), `api-non-exhaustive` + `proj-pub-crate-internal` (N17). Design record: `../review.md` §2, §5.5.

- [ ] Out-of-range duration conversion returns an error instead of panicking (N1)
- [ ] `"-0m"` equals `"0m"` and satisfies `>= 0`; signed-zero can't fail a range filter (N3′)
- [ ] Display never renders a nonzero duration as `"0s"` and never dumps raw 301-digit `f64` — chosen rendering dialect with exponent threshold (N14, N19)
- [ ] Deserialization runs through the module's own parser: four-digit-year rule honored, `YYYY-MM` accepted, round-trip Display ↔ Deserialize stable, `…Z` reserved for explicit interop (N12)
- [ ] Both errors are `#[non_exhaustive]`; `DateTimeValue`/`DateError` exported symmetrically with duration counterparts (N17)
- [ ] `"-1h 30m"` whole-duration sign rule pinned by a test (N15b)
- [ ] Demo scenario: a hostile YAML note (extreme-magnitude duration, `-0m`, `YYYY-MM` date, `…Z` datetime) parses, round-trips, and displays without panic, silent coercion, or lying output
- [ ] Engine-side error source chains (N4) are ticket 02's scope — not claimed here
- [ ] New/changed suites: every case-surface row maps to a named test; `mise run verify` green
