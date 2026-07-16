# `traces trust` CLI (default, path, list, clean)

Status: implemented

## Parent

`.scratch/config-service/spec.md`

## What to build

The `traces trust` command surface, wired through clap to the trust store from issue 04. Issue 04 only built `ConfigTrust::trust()`/`is_trusted()` (check/create/reject) — `list`/`clean` **do not exist yet** on `ConfigTrust`/`ConfigService` and are this issue's responsibility to add, mirroring `ConfigTracker::list_all()`/`clean()` and `ConfigService::list_tracked()`/`clean_tracked_store()`. Nor does a `src/cli/` module or a `clap` dependency exist yet — this issue is also the first CLI scaffolding, not just wiring onto an existing surface.

- `traces trust` — trust `cwd`
- `traces trust <path>` — trust the given path
- `traces trust list` — list all trusted directories (read the trust store entries)
- `traces trust clean` — remove stale entries whose target directory no longer exists, including orphaned content-hash companions

## Resolved design (from triage grilling — see Comments)

`ConfigService::trust(root, config_file)` takes two paths because issue 04's local-config trust needs a config file to content-hash. The CLI only ever has one user-supplied path. Resolution:

- **Deriving `config_file`:** the CLI computes `config_file = <path>/.traces/config.toml` (or `cwd/.traces/config.toml` for bare `traces trust`).
- **Tolerate a missing config file.** `traces trust <path>` before `.traces/config.toml` exists (i.e. before `traces init`) is a valid, intended flow — not an error. When the derived `config_file` doesn't exist, `ConfigTrust::trust()` must record the root only (via the store, no content-hash companion) instead of propagating `TrustError::Hash`. **This changes `ConfigTrust::trust()`'s behavior from issue 04**, not just the CLI layer: add a companion-optional path through `ConfigTrust::trust()` (or a `trust_root_only()` variant) that skips the `hash_file_contents`/companion-write steps when the config file is absent.
- **Known consequence, not a bug:** after a companion-less trust, a later `init` + `build()` will see a missing companion and report `Stale` (existing `is_trusted` behavior: missing companion always fails toward re-verification). The user re-runs `traces trust` once after `init` to bind the content hash. Document this in `--help`/error text; do not "fix" `is_trusted` to special-case this — it's the same fail-safe behavior issue 04 already relies on for legacy entries.

## Acceptance criteria

- [x] `ConfigTrust` gains `list_all() -> Result<Vec<PathBuf>, StoreError>` and `clean() -> Result<usize, StoreError>`, delegating to the shared `ConfigFileStore` for the base entries
- [x] `ConfigTrust::clean()` also removes each pruned entry's orphaned `<entry>.hash` companion — cannot be a bare delegation to `ConfigFileStore::clean()`, which has no concept of companions. Add whatever `ConfigFileStore` primitive is needed to know which entries were/would be removed (mirrors how `entry_path` was added in issue 04 for the same companion-file need)
- [x] `ConfigService` gains `list_trusted()`/`clean_trusted_store()` (or equivalent names matching `list_tracked`/`clean_tracked_store`'s convention), delegating to `ConfigTrust`
- [x] `ConfigTrust::trust()` (and thus `ConfigService::trust()`) accepts a `config_file` that does not exist: records the root only, skips the content-hash companion, does not error
- [x] `traces trust` with no args trusts `cwd` (config file derived as `cwd/.traces/config.toml`)
- [x] `traces trust <path>` trusts the given directory (config file derived as `<path>/.traces/config.toml`), whether or not that file exists yet
- [x] `traces trust list` prints all currently trusted directories (roots only, no staleness column — consistent with `list_tracked`)
- [x] `traces trust clean` removes dangling/stale trust entries (and their companions) and reports what was removed
- [x] `src/cli/` module created, with its own error type (miette-wrapping `TrustError`/`StoreError` with `help` text, per `src/config/mod.rs`'s stated plan for a future CLI layer) — no other module currently owns CLI errors
- [x] `clap` added to `Cargo.toml`; `src/main.rs` wires the `Cli`/`Commands` root and calls into `src/cli/`
- [x] Integration tests verify each subcommand's effect on the trust store (temp store), including: trust-before-init (no companion) not erroring, and clean removing both a stale root entry and its companion — as unit tests in `src/cli/trust.rs`; see implementation notes for why unit tests (not a `tests/` crate) were used, and for a wiring-coverage gap this found and closed

## Rust guidance

Relevant skills: `domain-cli`, `m05-type-driven`, `m06-error-handling`.

- **clap shape (domain-cli):** model `trust` as a subcommand whose payload is an optional positional path *plus* nested actions. `list` and `clean` are subcommands of `trust`, while bare `traces trust` and `traces trust <path>` share the default action. Use `#[derive(Subcommand)]` with an enum; give `list`/`clean` their own variants and a default `Trust { path: Option<PathBuf> }` variant. Verify clap can disambiguate `trust <path>` from `trust list` (reserve `list`/`clean` as keywords).
- **stdout vs stderr (domain-cli):** `trust list` output is data → `println!` to stdout (pipeable). Errors and the `clean` removal report go to stderr via `eprintln!`. **Note:** this workspace's `Cargo.toml` denies `clippy::print_stdout` (only `print_stderr` is allowed) — `trust list`'s `println!` call site needs an explicit `#[allow(clippy::print_stdout, reason = "...")]`, matching the existing `print_stderr = "allow"` precedent, or `cargo clippy -- -D warnings` fails.
- **Thin CLI layer (m05):** the command handlers should be thin adapters over the trust store API — parse args, derive `config_file` from the positional path, call `trust()`/`is_trusted()`/`list_trusted()`/`clean_trusted_store()`, format output. No trust logic in the CLI layer beyond the path derivation above.
- **`clean` semantics (m06):** report which entries were removed; a store with no stale entries is success with an empty report, not an error.

## Blocked by

- `.scratch/config-service/issues/04-trust-store.md` (implemented — see its Status/Comments; the check/create/reject core this issue builds on is done)

## Comments

> *This was generated by AI during triage.*

Re-triaged: issue 04 (its blocker) turned out to already be fully implemented despite a stale `Status: ready-for-agent`, but its own Comments flagged that `list_all`/`clean` on `ConfigTrust` were deliberately deferred to this issue and never added. Grilling also surfaced a real signature mismatch — `ConfigService::trust(root, config_file)` needs a config file to hash, but the CLI only ever has one user path — and ADR 0002's "Update (issue 04)" section, which explicitly left "template-directory trust... still owed to issue 05" as an open thread. Resolved: one trust store (not two), with `ConfigTrust::trust()` tolerating a missing config file by recording the root only. See ADR 0002's amendment for the closed-out rationale. Brief above rewritten to reflect this; kept `ready-for-agent` rather than downgrading, since the design is now fully resolved.

## Implementation notes

Built in worktree `.worktrees/trust-cli` off `feat/trust-cli` (branched from `main` at `758429f`), across four commits: `ffce07c` (initial CLI + `list`/`clean` + companion-optional trust), `36f6773` (first code-review round: `TrustTarget` enum, `ConfigFileStore::clean_with_companion`, `ConfigTrustCliError`, ADR amendment), `1015360` (`pub` → `pub(crate)` visibility audit, `TrustTarget::from(&CandidateConfigFile)`), and a fourth commit closing an AC-coverage gap found on a subsequent AC-by-AC review (see below).

**File layout:**
- `src/cli/mod.rs` (new) — `Cli`/`Commands` root (`#[derive(Parser)]`/`#[derive(Subcommand)]`), currently one variant: `Trust(trust::TrustArgs)`. `pub fn run() -> Result<(), ConfigTrustCliError>` parses argv, builds a `ConfigService`, dispatches.
- `src/cli/trust.rs` (new) — `TrustArgs { action: Option<TrustAction>, path: Option<PathBuf> }` with `#[command(args_conflicts_with_subcommands = true)]` so clap disambiguates `trust <path>` from `trust list`/`trust clean` (a bare positional can't collide with a reserved subcommand name; combining `list`/`clean` with a path is a clap usage error, not silently ignored). `TrustAction::{List, Clean}`. `trust()` derives `config_file = root.join(LOCAL_CONFIG_FILE)` and builds a `TrustTarget` via `TrustTarget::for_root`; `list()`/`clean()` are one-line delegations to `ConfigService::list_trusted()`/`clean_trusted_store()`.
- `src/cli/error.rs` (new) — `ConfigTrustCliError` (`thiserror` + `miette::Diagnostic`, `fancy` feature enabled), one variant per command (`Trust`/`List`/`Clean`), each wrapping the boxed `config` source error with its own error code (`traces::cli::trust::{failed,list_failed,clean_failed}`) and `help` text. This is deliberately the *only* place in the crate presentation concerns (codes, help text) get attached — `config`'s own errors (`StoreError`/`TrustError`) stay `thiserror`-only, per `config/mod.rs`'s stated layering.
- `src/main.rs` — reduced to `fn main() -> miette::Result<()> { traces_pkm::cli::run()?; Ok(()) }`, replacing a hand-rolled error/source-chain print loop with miette's standard idiom.
- `src/config/trust.rs` — `TrustTarget<'a>` enum (`Directory(&'a Path) | ConfigFile { root: &'a Path, config_file: &'a Path }`) replaces the brief's originally-planned two-positional-`&Path`-param signature (a deviation from the AC wording, not the intent: still satisfies "accepts a config_file that does not exist... records the root only"). `TrustTarget::for_root(root, config_file)` — the config-file-exists check, moved out of the CLI layer here per review (Feature Envy) — is the CLI's only construction path. `impl From<&CandidateConfigFile> for TrustTarget` is the infallible constructor for `config`-internal callers that already have a discovered candidate (always yields `ConfigFile`, since a candidate's path is discovery-confirmed to exist); the CLI never uses it (no discovery runs for a user-typed path). `ConfigTrust` gained `list_all()`/`clean()`; `clean()` now delegates entirely to `ConfigFileStore::clean_with_companion(COMPANION_SUFFIX)`.
- `src/config/store.rs` — `ConfigFileStore::clean_with_companion(suffix)` and `companion_path(entry, suffix)` (suffix-parameterized, not trust-specific) own both plain and companion-aware cleaning. Fixed a latent bug while extracting this: the pre-existing clean logic treated *any* non-symlink store entry as dangling, which would have deleted live `.hash` companions sitting next to real entries — a shared `recorded_target()` helper now distinguishes "not our entry" from "genuinely dangling entry."
- `src/config/service.rs` — `list_trusted()`/`clean_trusted_store()` added (mirrors `list_tracked`/`clean_tracked_store`); `trust()` takes a `TrustTarget` instead of two `&Path`s.
- `Cargo.toml` — `clap` (derive) added; `miette` gained the `fancy` feature (needed for the graphical diagnostic rendering `ConfigTrustCliError` uses, wasn't enabled before).
- `docs/adr/0002-symlink-based-config-trust-and-tracking.md` — amended to de-emphasize templates as the trust *subject* (workspace root + config file are what's trusted; templates are the motivating risk and a downstream consumer, not independently trusted) and removed the stale "template-directory trust... still owed to issue 05" language that had confused this issue's first triage pass.

**Visibility (third commit, not originally scoped by the brief):** audited every `pub` item `config` exposed. `main.rs` (a separate crate from the lib, since bin/lib targets compile independently) only ever calls `cli::run`, and nothing outside the crate touches `config` — no `tests/` integration crate, and unlike `dialog` (whose doctests compile as external crates and require true `pub`), `config` has no doctests. Downgraded to `pub(crate)`: `ConfigService` and its methods, `TrustTarget`/`TrustState`/`TrustError`, `StoreError`, `ConfigBuilderError`, `DiscoveryError`/`DiscoveryOutcome`, `Config`/`ResolvedTemplate`/`ResolutionError`. `mod config;` in `lib.rs` is now private. This surfaced two real findings: (1) `pub` items are exempt from `dead_code` analysis, so the downgrade revealed `config`'s discover/build/track/resolve pipeline and the read side of trust (`is_trusted`/`TrustState`) have no production caller yet — only `trust`/`list`/`clean`'s write+list+clean paths are wired into the CLI so far. Legitimate, tested groundwork for a future `init`/`render` command, marked with one `#[allow(dead_code, unused_imports, reason = "...")]` on `mod config;` rather than deleted or silently re-widened. (2) `clippy::unused_self` (deny-level) doesn't fire on fully-`pub` methods but does on `pub(crate)` ones — caught that `ConfigService::discover` never used `self`; changed from a method to a plain associated function.

**AC-by-AC review (fourth commit) found one real gap, since closed:** every AC above was independently re-verified against the code, not the self-report. All held except AC11's "integration tests" framing — `src/cli/trust.rs`'s handler tests (temp-dir-backed `ConfigService`, calling `trust::run()` directly) fully covered both named scenarios (trust-before-init not erroring, clean removing a stale root + companion), but the top-level `Cli`/`Commands` parser (`src/cli/mod.rs`) — the actual wiring AC10 describes `main.rs` as depending on — had zero test coverage: nothing called `Cli::parse`/`Cli::try_parse_from` or asserted on `Commands::Trust`. A misconfigured `#[command(subcommand)]` attribute or a broken `Commands::Trust(args) => trust::run(args, &service)` match arm would not have been caught by `cargo test`. Closed with one cheap parsing-only test in `cli/mod.rs` (`trust_argv_parses_to_the_trust_subcommand`, asserting real `["traces", "trust", "some/path"]` argv reaches the `Commands::Trust` variant) rather than a full `tests/`-crate, process-spawning integration test: the codebase has no `tests/` directory anywhere (including for the discover/build/track pipeline), the risk this closes is narrowly a wiring/parsing regression (already covered by real filesystem-effect tests one layer down), and `run()` has no injectable-args entry point to test the full dispatch without either a signature change or a process spawn — neither of which the AC's actual intent (verify subcommand effects) required.

**Testing:** 97 (issue 04 baseline) → 132 → 141 → 142 → 143 (fourth commit, `+1` for the `Cli`/`Commands` wiring test) unit tests, 10 doctests throughout.

**Verification (every commit):** `cargo test`/`mise run test-all`, `cargo clippy --workspace -- -D warnings`/`mise run clippy`, `rustup run nightly cargo fmt --all`/`mise run fmt`, `mise run lint` (hk, full pre-commit suite) all clean. `detect_changes()` reported `risk_level: low` on each commit. Manual binary smoke tests (trust → list → clean round trip, forced-error diagnostic rendering) verified.

**Two rounds of `/code-review`** (Standards + Spec axes, parallel sub-agents, diffed against `main`) came back clean of hard violations both times; findings folded in as part of the second and third commits (see commit messages `36f6773`/`1015360` for the itemized list). A subsequent AC-by-AC re-verification (not a `/code-review` round) is what found and closed the `Cli`/`Commands` wiring-coverage gap described above.

**Process note:** this issue file was initially (mistakenly) edited at the primary worktree's `.scratch/` path instead of this feature branch's `.scratch/` path — a copy-paste of a stray uncommitted change on `main`, reverted there and redone correctly here.
