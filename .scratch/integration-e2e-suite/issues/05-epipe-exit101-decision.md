# 05: EPIPE / exit-101 contract decision record

**What to build:** The EPIPE/exit-101 question stops being an ambient unknown: a decision is
recorded in-repo naming the intended contract and the cheap test shape that will guard it. No
production code changes here — this is the "decide, don't fix" half; a fix would be a separate
spec.

**Blocked by:** None (can start immediately).

**Status:** ready-for-agent

- [ ] Premise verified before deciding: reproduce `traces list | head` EPIPE behavior first
      (dev-profile panic → exit 101; release `panic = "abort"` → 134) — the decision records
      what was observed, not what the audit asserts
- [ ] Decision recorded in-repo with rationale, covering the observed behavior: Rust ignores
      SIGPIPE, so pipe errors arrive in-band as `ErrorKind::BrokenPipe`
- [ ] Research-informed recommendation documented as the leading option: **in-band
      `BrokenPipe → exit 0` detection** (ecosystem contract converged by ripgrep/bat/fd/ast-grep)
      rather than resetting to `SIG_DFL` (Windows has no SIGPIPE, so in-band detection must
      exist anyway)
- [ ] Decision notes the follow-up test shape (no PTY, no signal machinery): real OS pipe, close
      the read end early, spawn, assert the child did not exit 101 — precedents cited
- [ ] Decision clarifies the profile distinction: a dev-profile panic gives 101, release gives
      134; ticket 10's never-101 assertion therefore also acts as a no-panic check under
      nextest's dev-profile binaries — exit-101 stays asserted only as "never" until this
      decision is acted on
- [ ] Audit §10 potential-defects entry cross-referenced from the decision

**Evidence:** audit §10 (EPIPE/exit-101 "open question, not a ticket — decide the intended
contract first"). Ecosystem references (ripgrep #200 design contract, ast-grep #2887 prior-art
survey, rust#46016 known footgun) come from **triage research external to the audit** — verify
against the upstream sources; they are not in the repo's evidence base.
**Defect class:** unwitnessed contract (decision debt).
