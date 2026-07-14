# Trust store: check + create + untrusted rejection

Status: ready-for-agent

## Parent

`.scratch/config-service/spec.md`

## What to build

The trust mechanism, following the mise `config/tracking.rs` pattern (see ADR 0002, now accepted). A directory is trusted by creating a symlink (plain file on Windows) in the **trusted** store, named by the SHA-256 hash of the canonical directory path, pointing back at the directory.

The shared symlink-store component (`ConfigFileStore` in `src/config/store.rs`) already exists — it is instance-based, holds its root as a field, and exposes `record`, `list_all`, and `clean`. The trust store path (`dirs::TRUSTED_CONFIGS`) is already defined in `src/config/dirs.rs` (currently `#[allow(dead_code)]`). Build a thin **`ConfigTrust` adapter** (analogous to `ConfigTracker` in `tracker.rs`) that wraps `ConfigFileStore` at `dirs::TRUSTED_CONFIGS`. `ConfigService` gains `trust(dir)` and `is_trusted(dir)`. An untrusted directory check yields a miette error that includes the path and suggests running `traces trust`.

The `Trusted` builder stage (`ConfigBuilder::trust()`, in `builder.rs`) is currently a no-op placeholder — issue 04 makes it real: `trust()` accepts `&ConfigTrust`, checks the parent directory of each candidate config file path via `ConfigTrust::is_trusted()`, and returns `Result<ConfigBuilder<Trusted>, ConfigError>`. An untrusted candidate directory fails the build with a miette error suggesting `traces trust`. This guards config loading at the source — if a config file lives in an untrusted directory, it's rejected before parsing.

This is the programmatic trust API; the `traces trust` CLI is issue 05.

## Current codebase state

Relevant existing code at a glance:

- **`ConfigFileStore`** (`src/config/store.rs`): shared component. `new(&dirs::StateDirRoot)` for production, `at(PathBuf)` for tests. Methods: `record(&self, target) -> Result`, `list_all(&self) -> Result<Vec<PathBuf>>`, `clean(&self) -> Result<usize>`. Cross-platform: `#[cfg(unix)]` symlink, `#[cfg(windows)]` plain file.
- **`dirs::TRUSTED_CONFIGS`** (`src/config/dirs.rs`): `LazyLock<StateDirRoot>`, resolves to `$TRACES_STATE_DIR/trusted-configs`. Currently `#[allow(dead_code)]`.
- **`ConfigTracker`** (`src/config/tracker.rs`): the pattern to follow. Wraps `ConfigFileStore`, has `new()` and `#[cfg(test)] at()`, non-`Result` `track()` that logs-and-swallows, `list_all()`, `clean()`.
- **`ConfigBuilder::trust()`** (`src/config/builder.rs:103`): no-op `Tracked → Trusted` transition.
- **`ConfigService`** (`src/config/service.rs`): has `tracker: ConfigTracker` field; expose a trust adapter the same way.
- **`dirs` crate**: removed. Path resolution is self-contained in `dirs.rs`.
- **ADR 0002**: accepted — trust store path (`trusted-configs` under state dir) is ratified.

## Acceptance criteria

- [x] `trust(dir)` creates the hashed symlink/file in `dirs::TRUSTED_CONFIGS` via `ConfigFileStore::record` _(superseded — see "Revision" below: `trust` now takes `root` and `config_file` separately)_
- [x] `is_trusted(dir)` returns true only when a valid trust entry exists for the canonical path _(superseded — see "Revision": now returns a 3-state `TrustState`, not `bool`)_
- [x] Untrusted rejection error includes the path and a `traces trust` suggestion (miette)
- [x] Canonicalization ensures the same directory hashes consistently regardless of relative path (handled by `ConfigFileStore::record` already — canonicalize-then-hash lives in the shared component)
- [x] Trust logic reuses `ConfigFileStore` rather than reimplementing hashing/symlink/clean
- [x] Tests verify trust creation, positive/negative checks, and rejection error — using temp dirs and `#[cfg(test)]` `ConfigFileStore::at`/`ConfigTrust::at`

## Rust guidance

Relevant skills: `m11-ecosystem`, `m06-error-handling`, `m13-domain-error`, `m01-ownership`.

- **Pattern-match `ConfigTracker`** (`src/config/tracker.rs:25-88`). Build a `ConfigTrust` with the same shape: a `store: ConfigFileStore` field, `new(&dirs::TRUSTED_CONFIGS)`, `#[cfg(test)] at(root: PathBuf)`, and domain methods.
- **`trust(dir)` delegates to `self.store.record(dir)`** — `ConfigFileStore::record` already canonicalizes then hashes (SHA-256 hex filename). Record returns `Result<(), StoreError>`; unlike tracking (best-effort), trust decisions should propagate errors since a security gate that silently fails is worse than a crash.
- **`is_trusted(dir)`** checks whether `dirs::TRUSTED_CONFIGS / hash(canonicalize(dir))` exists as a file. A simple `Path::exists` check is correct because `ConfigFileStore::record` created it as a symlink/file. No need to resolve the link target for the boolean check — only existence matters. Use `try_exists()` (`m01-ownership`) instead of `exists()` to distinguish "not found" from "I/O error" for the trust-error path.
- **Rejection error (`m13`):** the untrusted-directory error is **user-facing and actionable** — add a `ConfigError::Untrusted { path: PathBuf }` variant rendered through miette with a `help` note suggesting `traces trust`. Distinguish "not trusted" (expected, actionable) from "trust store I/O failed" (internal — wrap `StoreError`).
- **Enforcement in the builder:** `ConfigBuilder::trust()` takes `&ConfigTrust` and checks `candidate.path().parent()` for each config file. This is the programmatic gate — an untrusted config source directory blocks the build before `merge()` reads the file. The check iterates `self.local` then `self.global` so the error points at the first untrusted source.
- **New module:** create `src/config/trust.rs` for `ConfigTrust` and its error types.

## Unblocked by

Issue 03 (`config-tracking-store.md`) is implemented on `feat/config-tracking-store` — `ConfigFileStore` and `dirs::TRUSTED_CONFIGS` are available. Start immediately.

## Revision: root()-anchored local trust, global auto-trust, content-hash staleness

Design review after the first implementation pass (documented in Comments below) found three problems with checking `candidate.path().parent()` for every candidate, local and global alike:

1. **Wrong anchor for local candidates.** `path().parent()` resolves to `.traces/`, an implementation detail of where discovery looks — not the project root a user would recognize as "the thing I'm trusting," and one that breaks the moment config-file location and project location can diverge (e.g. a future config-path override).
2. **Over-broad grant for global candidates.** `path().parent()` (and `root()`, identically) resolves to the *entire* `~/.config/traces/` folder — a single, fixed, shared location. Trusting it once trusts everything ever placed there, well beyond the one config file the decision was actually about.
3. **No re-verification on edit.** A pure path-hash trust entry, once created, never expires — an edit to an already-trusted config file (accidental, or malicious) is silently accepted forever.

Research into mise's actual trust implementation (`config_file/mod.rs`) and direnv's `.envrc` trust found: mise's *default* (non-paranoid) mode is directory-level, anchored at the project root (`config_root()`, which collapses several possible config file locations within one project to a single trust decision) — not the config file's own path. mise's *paranoid* mode adds file-content-hash re-verification on top. mise **never** hash-gates its own global config at all — it's unconditionally auto-trusted, since only the user can write to their own `$HOME`. direnv, solving a different problem (a single standalone executable script, not a project), hashes path+content together at file granularity.

**Revised design:**

- **Local candidates:** trusted at `candidate.root()` (the project root — directory-level, matching mise's default and fixing problem 1), **plus** a BLAKE3 content hash of the config file itself as a companion record (matching mise's paranoid mode, addressing problem 3).
- **Global candidates:** the trust check is skipped entirely — always considered trusted, matching mise's own global-config carve-out. This resolves problem 2 directly: there is no longer a directory-level trust *decision* for the global config folder to over-grant.
- **New `src/hash.rs`** (top-level, not `src/config/hash.rs` — content hashing is a generic utility, not config-specific): `hash_file(path) -> Result<blake3::Hash, HashError>`. `HashError` is `thiserror`-only, no `miette::Diagnostic` — it's an internal detail always wrapped by a higher-level trust error before reaching any CLI-facing diagnostic.
- **`ConfigFileStore` gains `entry_path(&self, target) -> Result<PathBuf, StoreError>`**, extracted from the canonicalize+hash logic `record`/`contains` already share, so `ConfigTrust` can derive a companion `<entry>.hash` path without reaching into `ConfigFileStore`'s internals.
- **New `TrustState { Untrusted, Stale, Trusted }`** (in `domain.rs`, public — `ConfigService::is_trusted` returns it) replaces the boolean: `Stale` means the directory trust entry exists but the config file's content hash no longer matches what was recorded at trust time. A missing/corrupt companion hash file is treated as `Stale`, not silently `Trusted` — fail toward re-verification.
- **New `TrustError`** (wraps `StoreError` and `HashError`, each its own `#[from]` — different source types, no ambiguity) replaces the ad-hoc `ConfigError::TrustIo { source: StoreError }` from the first pass.

**Revised acceptance criteria:**

- [x] Local candidates are trusted at `root()` (the project root), not `.traces/` or the config file's own path
- [x] Global candidates skip the trust check entirely and always pass, with no trust-store interaction
- [x] `src/hash.rs` provides `hash_file` returning a `blake3::Hash`; `HashError` derives `thiserror::Error` only, not `miette::Diagnostic`
- [x] Trusting a local candidate records both the `root()` directory-trust entry and a companion BLAKE3 content hash of the config file
- [x] Checking a local candidate re-verifies the config file's current content against the stored hash; a mismatch (directory trusted, content changed) returns `TrustState::Stale`, distinct from `Untrusted`
- [x] `TrustState::Stale` surfaces as its own `ConfigError` variant with miette help text distinguishing "never trusted" from "trusted, but the file changed since"
- [x] A missing or corrupt companion hash file is treated as `Stale`, not `Trusted`
- [x] Tests cover: root()-anchored local trust, global candidates always passing without touching the trust store, trust → edit → recheck yielding `Stale`, and re-trust after an edit clearing staleness

**Out of scope for this revision:** companion-`.hash` file cleanup (pruning orphans) — `ConfigTrust::list_all`/`clean` don't exist yet at all (already deferred to issue 05 in the first pass's Comments); when issue 05 adds them, the companion file needs the same treatment.

## Comments

Implemented on branch `feat/config-trust-store` (worktree `.worktrees/feat-config-trust-store`, based on `feat/config-tracking-store`).

**File layout:**
- `src/config/trust.rs` — new module. `ConfigTrust`, the trust adapter over `dirs::TRUSTED_CONFIGS`, mirroring `ConfigTracker`'s shape (`new()`, `#[cfg(test)] at()`) but propagating store errors from `trust()`/`is_trusted()` instead of swallowing them.
- `src/config/store.rs` — added `ConfigFileStore::contains(&self, target) -> Result<bool, StoreError>`, the canonicalize-then-hash-then-`try_exists()` check `ConfigTrust::is_trusted` delegates to. Reuses the exact same canonicalize/hash path as `record`, per the "reuse `ConfigFileStore`" acceptance criterion.
- `src/config/domain.rs` — `ConfigError` gains two variants: `Untrusted { path }` (expected/actionable, miette `help` suggests `traces trust <path>`) and `TrustIo { path, source: StoreError }` (internal trust-store I/O failure). `TrustIo` is *not* `#[from]` because `ConfigError` already has `Tracking(#[from] StoreError)` for the tracking store, and thiserror forbids two `#[from] StoreError` impls on one enum — `TrustIo` is constructed explicitly at its one call site instead.
- `src/config/builder.rs` — `ConfigBuilder<Tracked>::trust()` is no longer a no-op pass-through: it now takes `&ConfigTrust`, iterates `self.local` then `self.global`, checks `candidate.path().parent()` against `is_trusted`, and returns `Result<ConfigBuilder<Trusted>, ConfigError>` (`Untrusted` on the first untrusted directory, `TrustIo` on a store failure).
- `src/config/service.rs` — `ConfigService` gains a `trust: ConfigTrust` field (alongside the existing `tracker`), and two new public methods, `trust(&self, dir: &Path) -> Result<(), ConfigError>` and `is_trusted(&self, dir: &Path) -> Result<bool, ConfigError>`. `build()` now threads `&self.trust` through the fallible `.trust(&self.trust)?` builder stage.
- `src/config/dirs.rs` — removed the `#[allow(dead_code)]` on `TRUSTED_CONFIGS` now that `ConfigTrust::new()` is a real caller.

**Design notes:**
- `ConfigFileStore::contains` deliberately lives in `store.rs`, not `trust.rs` — the canonicalize-then-hash logic for a boolean membership check is identical to `record`'s, and duplicating it in the adapter would violate the "reuse `ConfigFileStore`" acceptance criterion the same way a copy-pasted `hash_path` would. `is_trusted` on `ConfigTrust` is a one-line delegation to `store.contains(dir)`.
- Trust propagates errors (`Result`-returning `trust`/`is_trusted`) where tracking swallows them (`ConfigTracker::track` logs and continues) — this is the same asymmetry the issue's Rust guidance calls out: tracking is best-effort bookkeeping, trust is a security gate where a silent failure is worse than a loud one.
- The builder's `trust()` checks `candidate.path().parent()`, literally as specified — for a local candidate this is the `.traces/` directory (not the project root), matching the issue's instruction precisely; broader "is the project root trusted" semantics were not introduced.

**Testing:** 15 new tests (69 → 84): `store::tests` (4) — `contains` true/false/relative-path-equivalence/canonicalize-failure. `trust::tests` (5) — untrusted-by-default, trust-then-trusted, idempotent re-trust, canonicalize failure on a missing directory, and a store-write-failure propagating as `Err` (not swallowed, unlike tracking's equivalent test). `domain::tests` (2) — `Untrusted`'s Display/miette-help text names the path and `traces trust`, `TrustIo` preserves the wrapped `StoreError` as its `source()`. `builder::tests` (2 new + 6 existing updated) — rejects the first untrusted candidate directory with the correct path, passes once that directory is trusted; all six pre-existing builder tests were updated to pre-trust their candidate directories through the now-fallible `.trust(&trust)` stage. `service::tests` (2) — `build` rejects a candidate in an untrusted directory, and `ConfigService::trust`/`is_trusted` round-trip correctly; the three pre-existing tracking tests were updated to construct a trusted `ConfigService` first.

**Bug caught during the loop:** the first version of a builder-test `trust_all` helper created its `ConfigTrust` from a `tempfile::TempDir` local to the helper function and returned only the `ConfigTrust`, not the guard — the `TempDir` dropped (deleting its directory) the instant the helper returned, so every "trusted" directory the tests set up was silently gone by the time the builder's `trust()` stage checked it, and six tests failed with `Untrusted` errors that were actually a test-fixture lifetime bug, not a trust-logic bug. Fixed by having the caller own the `TempDir` for the whole test's duration and passing `&ConfigTrust` into the helper instead of returning one from it.

**Formatting note:** the file-editing tool used during this session auto-formats on save with stable-channel rustfmt defaults, which drifted from this project's nightly/unstable-feature rustfmt style (visible as a "combine trailing `vec!`/struct-literal argument" difference) on lines it touched incidentally while inserting unrelated code nearby. Caught by `rustup run nightly cargo fmt --all -- --check` before commit; fixed with `cargo fmt --all` (write mode). Not a logic change — worth flagging for anyone re-running this workflow with the same tool.

**Verification:** `cargo test` / `cargo nextest run` (84 tests, up from 69), `cargo clippy --workspace --all-targets -- -D warnings` (clean), `rustup run nightly cargo fmt --all -- --check` (clean after the fix above). GitNexus's index for this repo predates this feature area (reported 0 changed symbols against 6 changed files, "stale index" — confirmed by direct source-level caller tracing instead: every caller of `ConfigBuilder::trust()` is within `builder.rs`'s own tests and `service.rs::build()`, both files already under review in this change). `detect_changes()` reported `risk_level: low`.

**For issue 05 (`traces trust` CLI):** `ConfigService::trust(dir)` / `is_trusted(dir)` are the programmatic surface for `traces trust [path]`. `ConfigTrust` does **not** yet expose `list_all`/`clean` — this issue's brief only asked for check/create/reject, not list/clean, so they weren't added speculatively (`ConfigFileStore::list_all`/`clean` are already there and ready to delegate to, same shape as `ConfigTracker`). `traces trust list`/`traces trust clean` from the spec will need `ConfigTrust::list_all()`/`clean()` plus matching `ConfigService` methods (mirroring `list_tracked()`/`clean_tracked_store()`) added as part of issue 05.

## Revision implementation notes

Implements the "Revision" section above, on the same branch/worktree.

**File layout:**
- `src/hash.rs` — new, top-level (not under `config/`). `hash_file(path) -> Result<blake3::Hash, HashError>`. `HashError` is `thiserror`-only, `pub` (not `pub(crate)`) because `TrustError` carries it as a `#[from]` source and a `pub` field can't have a private type — same reasoning `StoreError` already established.
- `src/config/store.rs` — added `ConfigFileStore::entry_path(&self, target) -> Result<PathBuf, StoreError>`, extracted from the canonicalize+hash step `record`/`contains` already shared. `record`/`contains` themselves are unchanged (each still computes its own canonical+entry inline — `entry_path` is additive, for `ConfigTrust`'s companion-file use, not a refactor of the existing methods, to avoid a redundant second canonicalize per call).
- `src/config/domain.rs` — new `TrustState { Untrusted, Stale, Trusted }` (public, with an `is_trusted()` convenience method). `ConfigError::Untrusted`'s message dropped "directory" (a project root, not necessarily read as a directory by the caller); new `ConfigError::Stale { path }` variant with its own miette help text; `ConfigError::TrustIo`'s `source` field changed from `StoreError` to the new `TrustError`.
- `src/config/trust.rs` — full rewrite. New `TrustError` (`pub`, `thiserror`-only — wraps `StoreError` and `HashError`, each its own `#[from]`, no ambiguity). `ConfigTrust::trust(root, config_file)` and `is_trusted(root, config_file)` (two-arg now): `trust` records `root` via the existing store and writes `config_file`'s BLAKE3 content hash to a companion file at `entry_path(root) + ".hash"`; `is_trusted` checks `root`'s entry, then compares `config_file`'s current hash against the companion, returning `TrustState`.
- `src/config/builder.rs` — `ConfigBuilder<Tracked>::trust()` now iterates `self.local` **only** (global candidates are never checked at all — no loop iteration, no store call, nothing), calling `trust.is_trusted(candidate.root(), candidate.path())` and mapping all three `TrustState` outcomes to the right `ConfigError` variant (or none, for `Trusted`).
- `src/config/service.rs` — `trust`/`is_trusted` signatures become `(&self, root: &Path, config_file: &Path)`; `is_trusted` returns `Result<TrustState, ConfigError>` instead of `bool`.
- `Cargo.toml` — added `blake3 = "1.8"` (default features; `--all-features` fails to build due to a `pure`+`neon` feature conflict in blake3's own build script — irrelevant here since defaults don't enable either).

**Why `entry_path` doesn't replace the logic inside `record`/`contains`:** `entry_path(target)` canonicalizes internally to resolve the entry location. Routing `record`/`contains` through it (instead of their own inline canonicalize) would canonicalize the same path twice per call (once via `entry_path`, once implicitly needed for the symlink's own target in `record`'s case) — `entry_path` is additive for `ConfigTrust`'s new companion-file use, not a refactor of the pre-existing, already-tested methods.

**Testing:** 13 new tests (84 → 97). `hash::tests` (3) — deterministic hashing, differs on content change, errors on a missing file. `store::tests` (2) — `entry_path` matches where `record` actually writes, errors on a nonexistent target. `trust::tests` (9, replacing the prior 5) — untrusted-by-default, trust-then-trusted, idempotent re-trust, **edit-then-stale**, **re-trust-clears-staleness**, a root trusted without ever getting a companion hash (simulating an entry from before this feature existed) is `Stale` not `Trusted`, canonicalize failures on both `root` and `config_file`, and a store-write failure still propagates as `Err` (not swallowed). `domain::tests` (+3) — `Stale`'s message/help, `TrustIo` now sourced from `TrustError` not `StoreError` directly, `TrustState::is_trusted()`'s three cases. `builder::tests` (renamed + 1 new) — the untrusted/trusted-root tests rewritten for the two-arg API with a properly root()-anchored candidate (the original two tests had actually constructed their candidate with a *mismatched* root, decoupled from `path`, since the old check only ever used `path().parent()` — that mismatch had to be fixed alongside the anchor change), plus a new `global_candidates_are_never_checked_against_the_trust_store` test asserting an empty trust store still lets a global-only build succeed. `service::tests` (+2) — a stale-root rejection through the full `build()` pipeline, and `trust`/`is_trusted` round-tripping through `TrustState` instead of `bool`.

**Verification:** `cargo test` / `cargo nextest run` (97 tests), `cargo clippy --workspace --all-targets -- -D warnings` (clean), `rustup run nightly cargo fmt --all -- --check` (clean), `cargo doc --no-deps` (clean — one `rustdoc::private_intra_doc_links` warning caught and fixed by dropping the intra-doc link brackets around a private-module reference in a doc comment). `detect_changes()` reported `risk_level: low` across 8 changed files (`Cargo.toml`, `src/lib.rs`, `src/hash.rs`, `src/config/{store,domain,trust,builder,service}.rs`).

**Left for issue 05, unchanged from the first pass:** `ConfigTrust::list_all`/`clean` still don't exist. When issue 05 adds them, the companion `.hash` files need the same pruning treatment as the base entries — a dangling companion (base entry removed, `.hash` file orphaned) is a new cleanup case this revision introduces that the first pass's `list_all`/`clean` deferral didn't have to consider.

**Code review (Standards + Spec axes) on this revision:** Spec axis found zero gaps — all 8 ACs verified directly against the diff (not the self-report above), including confirming the builder's trust loop is genuinely `for candidate in self.local` with no `.chain(self.global)` left over, and that `Stale`'s help text is actually distinct wording from `Untrusted`'s, not copy-pasted. Standards axis found one real bug and one real test gap, both fixed:

- **`TrustState` was never re-exported from `mod.rs`.** `ConfigService::is_trusted()` returns `Result<TrustState, ConfigError>`, but `pub use domain::{Config, ConfigError, ...}` never listed `TrustState` — a value of a type external callers couldn't name. Not caught by clippy (a `pub` item in a private module returned from a `pub` fn doesn't trip `private_interfaces` the way a `pub` *field* of a private type does), only by explicit review. Fixed: added to the `pub use` list.
- **Missing/unreadable companion hash was tested; corrupted (present-but-invalid) content wasn't.** Behavior was already correct by construction (a string-equality check treats garbage the same as any other mismatch), but nothing asserted it. Added `a_corrupted_companion_hash_is_stale_not_an_error`.
- **Speculative Generality, adopted:** `TrustState::is_trusted()` (a `bool`-collapsing convenience method) had zero production call sites — every real call site matches on the three variants directly. Removed, along with its dedicated test, rather than keep unused public surface area.
- **Data Clump, considered and rejected:** `(root: &Path, config_file: &Path)` travels together across `ConfigTrust`/`ConfigService`'s `trust`/`is_trusted` (4 signatures), and `CandidateConfigFile` already bundles exactly this pair. Not adopted: `ConfigTrust` is a lower-level adapter that shouldn't depend on `CandidateConfigFile` (a builder-pipeline concept, layering violation), and `ConfigService::trust`'s public API — the surface issue 05's `traces trust <path>` CLI calls — needs to accept an arbitrary user-named directory with no discovered config file at all, which has no `CandidateConfigFile` to construct. Two explicit `&Path` params fit both the internal single call site (in `builder.rs`, not actually duplicated/scattered) and the public API's real use case better than a bundling type would.

## Reorganization (post-review, user-requested)

Three structural changes, no behavior/AC changes:

1. **Path hashing moved to `src/hash.rs`, now BLAKE3-based.** `ConfigFileStore`'s internal `hash_path` (the SHA-256-over-path-bytes function keying every entry filename in both `tracked-configs/` and `trusted-configs/`) moved out of `store.rs` into the shared `src/hash.rs` module as `hash_path_to_str`, and switched from SHA-256 to BLAKE3 — the same hashing library already added for content-hash staleness, so the crate no longer depends on both `sha2` and `blake3` for two different hashing needs. `sha2` dependency removed from `Cargo.toml`/`Cargo.lock` entirely (it had no other callers). `hash.rs` now hosts two distinct, clearly-named functions: `hash_file` (content, fallible — reads the file) and `hash_path_to_str` (the path string itself, infallible — no I/O). This changes the actual on-disk hash values for existing trust/tracking entries (SHA-256 hex → BLAKE3 hex), but nothing persists across this refactor in tests (all fixtures are recreated per test), and production users have no existing entries yet (unreleased).
2. **`TrustState` moved from `domain.rs` to `trust.rs`.** It's a trust-specific result type, not a general config domain type — colocating it with `ConfigTrust`/`TrustError` (which produce and consume it) reads better than domain.rs, which is otherwise about `Config`/`TemplateConfig`/template resolution. `mod.rs`'s re-export changed from `pub use domain::{..., TrustState}` to `pub use trust::TrustState`.
3. **New `src/config/error.rs`** houses every error type across `config`'s submodules: `StoreError` (was in `store.rs`), `ConfigBuilderError` (was in `builder.rs`), `DiscoveryError` (was in `discovery.rs`), `ResolutionError` and `ConfigError` (were in `domain.rs`), `TrustError` (was in `trust.rs`). Each submodule now imports its error type(s) from `super::error` instead of defining them locally; `domain.rs` is left holding only `Config`/`TemplateConfig`/`ResolvedTemplate` and the template-resolution logic. The three error-type unit tests that constructed a `ConfigError` variant directly and asserted on its `Display`/`help`/`source` (not exercising any actual domain logic) moved from `domain.rs`'s test module to `error.rs`'s; every other test stayed put, since it's testing the *behavior* of the function under test, which happens to return that error type — not the error type itself.

**Verification after reorganization:** `cargo test`/`cargo nextest run` (100 tests, up from 97 — +3 for `hash_path_to_str`'s determinism/distinctness/formula tests), `cargo clippy --workspace --all-targets -- -D warnings` (clean), `cargo fmt --check` (clean), `cargo doc --no-deps` (clean — one more `rustdoc::private_intra_doc_links` warning caught, from `TrustState`'s doc comment referencing the now-private `super::builder` path directly instead of through `ConfigBuilder::trust`). `detect_changes()` reported `risk_level: low` across 12 changed files.

**Flagging, not fixing:** ADR 0002 still describes the trust/tracking store as SHA-256-keyed (its original, accepted decision) — now inaccurate given this reorganization's switch to BLAKE3. Left unchanged pending explicit instruction, matching this session's established pattern of only touching the ADR when asked to.
