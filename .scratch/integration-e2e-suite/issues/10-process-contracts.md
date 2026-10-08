# 10: Process contracts — exit codes, argv, streams

**What to build:** The process-boundary contract exists as assertions for the first time: the
harness can read numeric exit codes and exact streams, argv failures observe clap's
structurally-unreachable-in-crate exit 2 where it actually runs, representative paths are pinned
to never-101, stream splits are asserted exactly (not `contains`), and the in-crate interrupt
mapping link gains its missing unit. The PTY/130 question is closed with a recorded rejection.

**Blocked by:** 08 (new process-contract tests land in an invariant-conformant E2E suite).

**Status:** ready-for-agent

- [ ] Harness gains numeric code accessor, failure predicate, and exact-stdout/stderr
      assertions — test-crate-internal only, zero `src/` changes (fixture additions to exported
      types are forbidden; harness growth is not a surface change)
- [ ] Exit codes asserted: success → 0, domain failure → 1, argv failure → 2; `--help` /
      `--version` → 0 with correct stdout
- [ ] Argv contract via spawn (unknown subcommand, missing required flag e.g. `table` without
      `--column`, invalid value e.g. `completions --shell tcsh`) — each exit 2 + usage on
      stderr; these are unreachable in-crate (`Cli::parse` exits, `try_parse_from` returns)
- [ ] Representative valid and error paths assert **never 101** (no `ExitCode::from(101)` path
      exists in the binary; release `panic = "abort"` → 134 — and because nextest runs
      dev-profile binaries where a panic *would* give 101, this doubles as a no-panic check;
      ticket 05 owns the EPIPE/panic contract decision)
- [ ] Stream contracts (exact, not `contains`): `trust list` = `path\tstate`, `trust --show` =
      bare root, `trust clean`, `template --list`, `completions --list-templates`; mutating
      commands (`trust`, `untrust`, `index`, `template -i … --no-input`) assert
      `stdout == ""` with status on stderr; `index` asserts `indexed N file(s)` on stderr;
      `trust --all` + companion `untrust --all` with the count asserted via a follow-up spawned
      `trust list`
- [ ] Tests land in the generic process-contract capability file (candidate
      `process_contract.rs`; final name per ticket 19) — not appended to the legacy dispatch
      catch-all
- [ ] In-crate unit: `Cli::run → Aborted(Interrupted)` mapping (today only `Cancelled` is
      tested); the chain `DialogError::UserInterrupted` → `Aborted(Interrupted)` → `main`
      documented in the test doc
- [ ] Decision note recorded: **process-level 130 not approved** (the spec's condition — an
      approved portable PTY mechanism — is unmet; a PTY dependency remains out of scope; the
      question reopens only if that condition is met): with no signal handler in the binary, the
      only faithful injection is raw-mode PTY byte (`\x03`) delivery; no surveyed Rust project
      tests Ctrl-C through a PTY (cargo asserts `!success()` only; mise's exact-130 relies on
      its own signal handler), PTY suites have documented flake/quarantine history, and
      `ExitStatus::code()==Some(130)` remains distinguishable from signal death if ever
      revisited. The PTY ecosystem references are **triage research external to the audit** —
      verify against upstream sources
- [ ] New tests each carry the executes-vs-asserts note and named defect class

**Evidence:** audit §10 E-3 (clap exit-2 unreachable in-crate), §6 exit-code/stream rows
(five zero-assertion stdout commands incl. `trust clean`), §14 P1.6 + P1.11; spec §Gap tests
(exit codes, conditional 130), §Testing Decisions; PTY non-approval research (triage-time,
external to the audit: cargo `death.rs`, mise `test_exit_status`, expectrl/deno flake issues —
verify upstream).
**Defect class:** unwitnessed contract (zero numeric exit assertions existed).
