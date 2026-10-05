# 09: Dead-surface cleanup (Decision-C)

**What to build:** The cluster's public surface is all true surface: every method is either wired to a real consumer or gone. Callers no longer discover methods that only tests exercise.

**Blocked by:** 04 (precision hoist decides who owns re-serialization; 04 subsumes 03), 05 (demo contracts 03–05 must be re-runnable after every deletion here), 06, 07 (parity wiring can add or retire callers — the dead-surface census is stable only once parity lands), 08 (docs first: deletions here can strand 08's rustdoc/intra-doc links, and `mise run verify` does not build docs).

**Status:** ready-for-agent — X7(a)/D1-ordering/F5 closed 2026-10-05 (see Comments); brief attached
**Category:** enhancement — surface/visibility cleanup with no behavior change; the stale spec-decision-12 divergence is resolved in 05 (X7), not here.

Skills: `codebase-design`, `rust-skills`. Rules: the deletion test — if deleting the module makes complexity vanish it was a pass-through, if it reappears across callers it earns its keep; `proj-pub-crate-internal` for visibility decisions; follow ticket 02's named zone-conversion rule — no unguarded `naive_local()` at its range edge, and never `naive_utc()` where a local wall clock is required. Design record: `../review.md` §2.3 (Decision-C), §10 step 6.

- [ ] **(rewritten 2026-10-05, §11 X-B)** Each dead-surface method is wired to a real consumer or deleted — recorded decision per method, not a blanket sweep. **The original set below is stale: five of its six methods no longer exist** — `to_time_string`, `start_of_day`, `cmp_date`, `checked_add`/`checked_sub` residue, and `to_offset_string` were deleted by ticket 03's remediation (`rg "to_time_string|start_of_day|cmp_date|to_offset_string" src/` → 0 hits; `to_date_string` on `DateTimeValue` too). The **live census is six `expect(dead_code)` sites**: `DateValue::shift` (`src/date.rs:165`), `DateValue::apply` (`src/date.rs:192`), `DateTimeValue::apply` (`src/date.rs:585`), `DurationValue::from_seconds` (`src/duration.rs:353`, fn `:359`), `DurationValue::parts` (`src/duration.rs:434`, fn `:441`), `DurationValue::is_calendar` (`src/duration.rs:454`, fn `:461`). **Do not produce "wire or delete" decisions for methods that are already gone.** Also note ticket 03's handoff declares all six "spec-mandated surface with tests as first consumers" and names declared consumers (04's `classify` for `is_calendar`/`parts`, 05 for the arithmetic set) — so the default outcome here is *verify the consumer materialized, else record an exemption with rationale*, and only delete when neither exists after 05–08 land. *(Original audit note retained for history: `to_date_string` on `DateTimeValue` was absent from `review.md`'s audit — a code-review-only find, which is one reason the audit's "8 methods" count never matched the code.)*
- [ ] **(added 2026-10-05, §13 T-new-2/T-new-5) The `rg dead_code` census under-counts — extend it.** The live six-site census (item above) greps the `dead_code` attribute and is blind to three strata: **(a) six zero-production-caller trait impls** — `FromStr for DurationValue` (`src/duration.rs:754`), `FromStr for DateValue` (`src/date.rs:313`), `FromStr for DateTimeValue` (`src/date.rs:752`), `From<NaiveDate> for DateValue` (`src/date.rs:297`), `TryFrom<DurationValue> for TimeDelta` (`src/duration.rs:763`), `TryFrom<DurationSeconds> for TimeDelta` (`src/duration.rs:1043` — reachable only through the dead `apply`, and it is one half of the §11 D4 conversion duplication ticket 05 unifies); **(b) census-blind micro-dead-code** — `DateTimeValue::now()` behind `#[cfg(test)]` (`src/date.rs:377-385`, validates `std`, not the module, so it will survive an "ideally zero" AC), the dead `.ok_or(OutOfRange)` in `DatePoint::diff` (`src/date.rs:990`), and the phantom `Result<DateDiff, DateError>` error channel (doc concedes it at `src/date.rs:964-966`, overlaps §11 D8); **(c)** spec L113 now carries the same census caveat. Produce a wire-or-delete/keep record for each stratum alongside the existing six-site decisions — enumerate the trait impls explicitly in the deliverable rather than relying on the grep.
- [ ] `from_seconds` is not deletable: wired to a live caller, or kept as the documented synthesis constructor with its `expect(dead_code)` (`src/duration.rs:353`, fn `src/duration.rs:359` — re-pinned 2026-10-05; this ticket previously cited `src/duration.rs:217`, a pre-`bcc938b5` position) justified — deletion would void ticket 03's `from_seconds`-stores-`None` AC and ticket 01's N14 display test (`from_seconds(1e300)`), so no removal-list entry may name it, nor anything else tickets 01–08 pin
- [ ] **(premise retired 2026-10-05, §11 X-C)** ~~`to_offset_string` re-targeted as the `…Z` interop seam~~ — **the work already happened by deletion**: `to_offset_string` no longer exists (`rg to_offset_string src/` → 0 hits, deleted by ticket 03's remediation), and the `…Z` producer spec reserved is `Serialize for DateTimeValue`, which already emits `to_rfc3339_opts(SecondsFormat::Secs, use_z = true)` (`src/date.rs:771`); `rg "to_rfc3339\(" src/` → 0 hits, so there is no `+00:00` emitter left to re-target. Residual action: confirm with one assert that serialized datetimes end in `Z` (if ticket 01/03 did not already pin it) and delete this item's decision from the per-method table.
- [ ] Deletion test re-run on `into_inner`/`From` chrono surface after the calendar owner landed: keep the impls, confirm the need actually shrank (D7 demotion honored)
- [ ] `#[cfg_attr(not(test), expect(dead_code))]` count reduced to (ideally) zero; any remainder justified in the ticket comment (expected after the rewrites above: `from_seconds` if kept as synthesis constructor, plus whichever of the six-site census items 04/05 do not wire — original expectation of a retained `to_offset_string` is void, that method is deleted)
- [ ] **(added 2026-10-05, §11 F5/U5 — extraction assigned to this ticket)** Extract one crate-level `normalize_zero` helper from the byte-identical pair `fn normalize_zero` at `src/note/field.rs:468-474` and `src/query/sort.rs:445` (both introduced by `21ae8b1d`, the commit that closed signed-zero drift by duplicating the policy), and move the third consumer at `src/note/field.rs:263` (`NoteFieldValueRef::compare`'s `Number` arm) onto it. One helper for the two raw-`f64` comparison sites, all three consumers routed through it, behavior identical (`if n == 0.0 { 0.0 } else { n }`); keep `DurationSeconds::normalized` (`src/duration.rs:1022`) untouched — construction invariant ≠ comparison idiom; `mise run verify` green. Ticket 08 keeps only the doctrine record (CONTEXT/ADR alongside "canonical duration Eq"); the extraction itself is this ticket's, not 08's.
- [ ] No behavior change for the surviving surface; deletions recorded per method — `mise run verify` green and `mise run doc --all-features` clean (verify skips cargo-doc, so the docs build is a separate gate this ticket must run, since it deletes items 08 just documented), and the demo contracts from tickets 03–05 still pass

**Review amendments (2026-10-05 adversarial rust-design pass; source: `../review.md` §11):**

- [ ] **(X7 resolved 2026-10-05 → option (a), §11 X-B) Wire-or-delete with no pre-granted exemption.** Spec decision 12 is wired, not amended: ticket 05 routes `date_add` with a multi-part duration through `DateValue::apply` (`src/date.rs:192`)/`DateTimeValue::apply` (`src/date.rs:585`) in written order, and ticket 04's `classify` work covers the other declared consumers (`DurationValue::parts` `src/duration.rs:441`, `is_calendar` `src/duration.rs:461`). The test for `apply`/`parts`/`is_calendar` therefore runs with **no pre-granted exemption**: default = verify each surface is genuinely wired to a real production consumer, then delete only what is genuinely unreached after 05–08 land — no "record an exemption with rationale" fallback. The only escape hatch is 05 failing to deliver its wiring; treat that as a risk to detect at execution, not a planned exemption. Do not invent a third option at execution time. Same verification applies per item 11's census.
- [ ] **(ordering resolved 2026-10-05, §11 D1) Add `shift_wall` to the `proj-pub-crate-internal` visibility review — no longer conditional.** The shift-frame fold lands in ticket 04, before its `YearMonth` arm — i.e. before this ticket runs — so the earlier "run this *after* D1 is decided" caveat and the "natural home for the visibility decision" claim no longer contradict: the fold is assigned to 04, and this ticket runs on the settled post-04 tree. `shift_wall` is `pub(crate)` at `src/date.rs:1143` solely for the engine's `date_shift_unit` caller (`src/template/engine/date.rs:482`), and the frame policy it implements is stated in five places (§11 D1). Demotion itself is 04's to execute — this ticket does not perform it; it verifies the post-04 state: nothing outside the item's module still reaches it — and records the visibility decision (private vs `pub(crate)`) against that settled caller set rather than one that is about to change.
- [ ] **(dead-surface stability) The census above is a point-in-time snapshot at HEAD `5b7dc748`.** Re-run `rg "dead_code" src/date.rs src/duration.rs` when this ticket starts; tickets 04–08 all add or remove callers. Expected stable-after-08 set: the six sites in item 11 (minus whatever 04/05 wired). The re-run must also cover the §13 strata (trait impls, `#[cfg(test)]` helpers, phantom Result channel) — `rg "dead_code"` alone misses all three; see the census-extension item above.

## Comments

> *This was generated by AI during triage.*

## Triage Notes (2026-10-05)

**What we've established so far:**

- Category `enhancement`, state `needs-info` — the core AC is explicitly gated on X7 (an undecided item recorded in ticket 05, itself awaiting triage answers), and the ticket simultaneously calls D1's fold "unassigned" and claims this ticket as its natural home while demanding it run *after* D1 is decided — so `ready-for-agent` did not hold.
- Census re-verified at current HEAD: exactly six live `dead_code` sites (`DateValue::shift`, `DateValue::apply`, `DateTimeValue::apply`, `DurationValue::from_seconds`, `parts`, `is_calendar`) at the cited locations (parts fn cite corrected :440 → :441); the five originally-named methods are confirmed gone; no `to_rfc3339(` emitter remains and the `…Z` serializer is in place; `shift_wall` is `pub(crate)` with the single engine caller as cited.
- Redundancy check: no other ticket or ADR owns the six-site wire-or-delete (04/05 declare *consumers*, 03's handoff points here, docs/adr has nothing on visibility/`shift_wall`) — 09 is the correct sole aggregator.
- Prior rejection: no `.out-of-scope/` directory exists — not a previously rejected request.
- Census command re-pinned in this pass (`rg "dead_code" …`); the six-site count remains a point-in-time snapshot to re-run at pickup.

**What we still need from you (@maintainer):**

- **X7 (gates the core AC — decide in 05)** — does `date_add` with a compound duration land in 05, giving `DateValue::apply`/`DateTimeValue::apply` a consumer, or do they get deleted (amending spec decision 12) or kept under an explicit exemption with the intended consumer named? Same bucket: `DurationValue::parts`, `is_calendar`.
- **D1 ownership** — assign the shift-frame fold before ticket 04, into 04, or into this ticket? (This ticket's "run after D1" ordering note and its "natural home for the visibility decision" claim resolve only one way each — they contradict until you assign it.)

> *This was generated by AI during triage.*

**2026-10-05 (decisions — maintainer):**

1. **X7 → option (a):** ticket 05 wires the compound path — `date_add` with
   multi-part durations routes through `DateValue::apply`/`DateTimeValue::apply`
   in written order — and ticket 04's classify work covers the other declared
   consumer. Therefore this ticket's wire-or-delete test for
   `apply`/`parts`/`is_calendar` runs with **no pre-granted exemption**:
   default = verify each surface is genuinely wired, then delete only what is
   genuinely unreached; the only escape hatch is 05 failing to deliver its
   wiring (treat as a risk to detect at execution, not a planned exemption).
   Answers the X7 notes question.
2. **D1 ordering resolved:** the shift-frame fold lands in ticket 04 (before
   its `YearMonth` arm), i.e. before this ticket runs. Your shift-`wall`
   visibility review is no longer conditional on "after D1 is decided" — it
   verifies the post-04 state (demotion itself is 04's to execute; you confirm
   nothing still reaches a private item). Dissolves the ordering contradiction
   in your notes.
3. **F5/U5 extraction → THIS ticket:** new work item — extract one crate-level
   `normalize_zero` helper from the byte-identical pair (note/field +
   query/sort), cover the third consumer, keep behavior identical, `mise run
   verify` green. Ticket 08 keeps only the doctrine record.

## Agent Brief

**Category:** enhancement
**Summary:** Make the temporal core's surface all true surface — every
`dead_code`-gated survivor is wired to a real consumer or deleted, `shift_wall`'s
visibility is settled against its post-04 caller set, and the duplicated
signed-zero normalization policy becomes one crate-level helper — with no
behavior change.

**Current behavior:**
The crate carries six `expect(dead_code)` sites: `DateValue::shift`,
`DateValue::apply`, `DateTimeValue::apply`, `DurationValue::from_seconds`,
`DurationValue::parts`, and `DurationValue::is_calendar`. Their declared
consumers come from sibling tickets: ticket 04's `classify` recognition entry
points are meant to consume `parts` and `is_calendar`, and ticket 05's
compound `date_add` is meant to route multi-part durations through the two
`apply` methods in written order — until those consumers are verified in
place, "wire or delete" cannot be decided honestly, and a blanket sweep or a
blanket exemption would both be wrong. Separately, `shift_wall` is
crate-visible only for the template engine's date-shift dispatch — a
frame-policy function whose ownership ticket 04's shift-frame fold settles —
so its visibility is currently pinned to a caller set that is about to change.
And the signed-zero normalization policy for raw `f64` comparison exists as
two byte-identical private helpers (note field comparison and query sort) plus
a third hand-rolled consumer: the commit that closed signed-zero drift
duplicated the policy it closed.

**Desired behavior:**
- Every survivor in the `dead_code` census is demonstrably wired to a
  production consumer before anything is deleted; genuinely unreached methods
  are deleted with a recorded per-method decision — no blanket sweeps, no
  pre-granted exemptions, no decisions for methods already gone.
- `DateValue::apply`, `DateTimeValue::apply`, `DurationValue::parts`, and
  `DurationValue::is_calendar` are each verified wired through ticket 04's
  classify and ticket 05's compound `date_add`; each is deleted only if, after
  those tickets land, no production consumer reaches it.
- `shift_wall` is verified against the settled post-04 caller set with nothing
  outside its module still reaching it, and its visibility is recorded on that
  set (the demotion itself is ticket 04's to execute).
- Signed-zero normalization is one crate-level helper used by both duplicated
  sites and the third consumer; `DurationSeconds::normalized` (a construction
  invariant, different responsibility) is untouched; observable behavior is
  identical.
- The docs gate stays green: deletions must not strand rustdoc or intra-doc
  links ticket 08 wrote.

**Key interfaces:**
- The `dead_code` census (date and duration modules): re-run at pickup —
  tickets 04–08 add and remove callers; each method gets its own wire-or-delete
  record
- `DateValue::apply` / `DateTimeValue::apply`: multi-part duration application
  in written order; production consumer is compound `date_add` (ticket 05)
- `DurationValue::parts` / `DurationValue::is_calendar`: production consumers
  are the date/duration `classify` recognition results (ticket 04)
- `DurationValue::from_seconds`: not deletable — documented synthesis
  constructor pinned by tickets 01/03; may retain `expect(dead_code)` with
  justification
- `shift_wall`: frame-policy function; visibility decided against the post-04
  caller set, demotion executed by ticket 04
- `normalize_zero` (new, crate-level): the single signed-zero normalizer for
  raw-`f64` comparison; both former copies and the third consumer call it;
  `DurationSeconds::normalized` stays as-is

**Acceptance criteria:**
- [ ] Every method still carrying `dead_code`/`expect(dead_code)` after tickets
      04–08 is shown wired to a production consumer (04's classify consumers
      and 05's compound `date_add` among them) before any deletion is made,
      and each deletion/retention is recorded per method rather than as a
      blanket sweep
- [ ] `DateValue::apply`, `DateTimeValue::apply`, `DurationValue::parts`, and
      `DurationValue::is_calendar` are each verified wired; any that no
      production consumer reaches is deleted, and any that remains is wired —
      no exemption-by-default record stands in for a missing consumer
- [ ] `DurationValue::from_seconds` and any other surface tickets 01–08 pin
      appear in no removal list
- [ ] In the post-04 tree, nothing outside its module reaches `shift_wall`,
      and its visibility (private vs crate-visible) is recorded against that
      settled caller set
- [ ] One crate-level `normalize_zero` exists; both duplicated private copies
      and the third raw-`f64` consumer call it; `DurationSeconds::normalized`
      is unchanged; signed-zero behavior is unchanged and still pinned by the
      existing tests
- [ ] No behavior change for the surviving surface: the demo contracts from
      tickets 03–05 still pass
- [ ] `mise run doc --all-features` clean (`RUSTDOCFLAGS=-D warnings` — verify
      does not run cargo-doc), and no deletion strands a doc link ticket 08
      wrote
- [ ] `mise run verify` green
- [ ] Every checklist item in this ticket's body is checked

**Out of scope:**
- Implementing the consumers themselves (tickets 04 and 05) — verify them,
  don't build them
- The signed-zero doctrine record, CONTEXT/ADR clauses, and divergence-register
  entries (ticket 08 keeps those; this ticket only extracts the helper)
- Engine behavior changes, including the shift-frame fold and the `shift_wall`
  demotion (ticket 04 executes those; this ticket verifies the result)
- Query temporal functions (05), template format dialects (06),
  shorthands/`parse_with` (07) as features
- Deleting `DurationValue::from_seconds` or any other surface tickets 01–08 pin
- Spec amendments — spec decision 12 is wired per the option-(a) decision
  recorded above, not amended
