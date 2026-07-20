# Progress Log

## Session: 2026-07-19

### Planning Artifact Review
- **Status:** complete
- Actions taken:
  - Ran planning-with-files session catchup for `/Users/jack/Documents/41_personal/traces-pkm`; no unsynced context was reported.
  - Read `task_plan.md`, `findings.md`, and `progress.md`.
  - Read design guidance: `planning-with-files`, `codebase-design`, `ponytail`, `m05-type-driven`, `m06-error-handling`, and Rust rules for parse-don't-validate, typestate, and enum states.
  - Began comparing the artifacts against current config/trust source summaries.
- Initial review findings:
  - `task_plan.md` still says the goal includes a `ConfigLocator` module, but later decisions reject a separate locator and keep discovery in `src/config/discovery.rs`.
  - `task_plan.md` current phase is stale: it says Phase 2 research, while every phase is marked complete and `progress.md` says final design recorded.
  - `progress.md` keeps Phase 2 as `in_progress` even though the same section records completed final design and implementation order.
  - Current source still matches the handoff's pre-refactor state: `CandidateConfigFile`, `DiscoveryProcessor`, two-step `ConfigService::discover/build`, and trust-target resolution remain present.
  - Spawned three read-only review scouts for artifact consistency, source alignment, and design holes.
  - Recorded artifact repairs in `task_plan.md` and `findings.md`.
  - Confirmed the implementation plan needed an ordering repair: introduce `ConfigFile<State>` alongside `CandidateConfigFile`, migrate consumers, then delete the old candidate type.
  - Confirmed unresolved design holes before follow-up grilling: global parse transition naming, trust target routing, unified `DiscoveryOutcome` cardinality, builder input stage, ZST `DiscoveryEngine`, local ordering, wrong-kind errors, and discovery kind naming.
  - Answered follow-up questions and recorded settled decisions: `try_into_global_parsed()`, decided `TrustTarget` and `ConfigBuilderInput` shapes, unified `DiscoveryOutcome`, ZST `DiscoveryEngine`, `LocalSubtree`, and `ConfigBuilderInputError` only for wrong-kind/invariant variants.
  - Recorded recommended defaults in `findings.md`.
  - User settled follow-up decisions: use `try_into_global_parsed()`, keep `TrustTarget`, keep unified `DiscoveryOutcome`, keep `ConfigBuilderInput`, keep ZST `DiscoveryEngine`, rename discovery kind to `LocalSubtree`, and keep local absence in `DiscoveryError` rather than `ConfigBuilderInputError`.


### Config Typestate Implementation
- **Status:** complete
- Worktree: `/Users/jack/Documents/41_personal/traces-pkm/.worktrees/config-typestate`
- Actions taken:
  - Added `src/config/file.rs` with `ConfigFile<State>` lifecycle markers and source-specific local/global constructors.
  - Completed the lifecycle transitions through `From`/`TryFrom`: discovered locals become tracked through `TrackConfigFile` plus `ConfigTracker`, tracked locals become trusted through `TrustConfigFile` plus `ConfigTrust`, trusted locals parse into `Parsed`, and discovered globals parse directly while local discovered configs are rejected.
  - Migrated discovery to `DiscoveryContext::new`, `DiscoveryScope::{Full, NearestLocal, LocalSubtree}`, `DiscoveryAnchor`, ZST `DiscoveryEngine`, and unified `DiscoveryOutcome`.
  - Replaced `CandidateConfigFile` with `ConfigFile<Discovered>` and removed `src/config/candidate.rs`.
  - Removed `DiscoveryOutcome.cwd`; `DiscoveryAnchor` now carries the post-discovery location context.
  - Added `ConfigBuilderInput`, parsed it from `DiscoveryOutcome`, and used it as the constructor input for `ConfigBuilder<Discovered>`.
  - Collapsed builder states to aggregate construction: `Discovered -> LocalStored -> Merged`.
  - Added `ConfigService::load(cwd)` and made discovery/build helpers private.
  - Reworked trust target routing so CLI path resolution returns discovered local `ConfigFile` values through discovery helpers.
  - Removed `ResolvedTrustTarget` and obsolete trust-specific resolver/error code.
  - Moved discovery path helper behavior onto `DiscoveryAnchor::path()` and moved discovery helper functions into `DiscoveryEngine`.
  - Removed the generic `ConfigFile<State>::parse()` method and the lifecycle `transition()` helper; only explicit conversion impls can change lifecycle state.
  - Changed trust status checks so `ConfigTrust::is_trusted` and `ConfigService::is_trusted` accept `TrustTarget` as the only target input.
- Verification:
  - `cargo test config::`: 87 passed.
  - `MISE_EXPERIMENTAL=1 mise run fmt`: passed.
  - `MISE_EXPERIMENTAL=1 mise run test`: 157 passed.
  - `MISE_EXPERIMENTAL=1 mise run clippy`: passed.
  - `MISE_EXPERIMENTAL=1 mise run ci`: passed; `cargo deny` emitted existing duplicate/unmatched-license warnings but did not fail.
  - `gitnexus detect_changes(scope=all, worktree=.worktrees/config-typestate)`: low risk, 9 changed files, no indexed symbol/process impact.
- Errors encountered:
  - `Blake3FileHash::new` was called with `PathBuf` instead of `&Path`; fixed by borrowing the path.
  - Several tests wrote `.traces/config.toml` before creating the parent directory; fixed test setup.
  - Old trust resolver tests kept exercising deleted resolver functions; moved routing coverage to discovery/service tests and retained pure trust-store tests.
  - `mise run ci` initially failed on clippy dead-code and style lints because config loading is implemented before a render command consumes it; fixed style lints, removed obsolete resolver code, and added targeted `expect` attributes for intentionally pre-wired config loading seams.
  - `cargo test` was invoked with multiple test filters in one command; Cargo accepts only one test-name filter. Re-ran the focused regression as `cargo test config::`.
  - `mise run clippy` initially failed after the correction pass because the non-rendered config loading seam again triggered dead-code lints and helper methods had clippy style issues. Fixed with a `cfg_attr(not(test), expect(dead_code))` module attribute, associated helper functions inside `DiscoveryEngine`, and a borrowed `DiscoveryAnchor` in builder input selection.
  - GitNexus impact lookup could not find the newly added `ConfigFile`, `TrustTarget`, or `ConfigTrust` symbols in the stale/small index; used grep/LSP fallback for callsite mapping, then ran `detect_changes`.
  - Rust-analyzer failed to initialize for symbol references in this worktree (`LSP reader stopped; client torn down`); grep callsite mapping covered the API change.
  - `mise run fmt` failed once after a large edit because `impl ConfigFile<Discovered>` was left unclosed; inserted the missing brace and reran fmt successfully.
  - Tuple/phantom cleanup initially left duplicate braces/expect calls during a surgical edit; removed the duplicates, reran `mise run fmt`, and focused tests passed.

### Tuple and PhantomData Follow-up
- **Status:** complete
- Actions taken:
  - Replaced `TrackConfigFile` and `TrustConfigFile` wrapper structs with tuple conversion inputs.
  - Removed `ConfigFile<State>::_state: PhantomData<State>`; the concrete `state: State` field already carries the type parameter.
  - Updated builder and tests to call `ConfigFile::<Tracked>::from((file, &tracker))` and `ConfigFile::<Trusted>::try_from((file, &trust))`.
- Verification:
  - `cargo test config::`: 87 passed.
  - `MISE_EXPERIMENTAL=1 mise run fmt`: passed.
  - `MISE_EXPERIMENTAL=1 mise run test && MISE_EXPERIMENTAL=1 mise run clippy && MISE_EXPERIMENTAL=1 mise run ci`: passed.
  - GitNexus `detect_changes(scope=all, worktree=.worktrees/config-typestate)`: low risk, 9 changed files, no indexed symbol/process impact.

### Builder Input Selection Follow-up
- **Status:** complete
- Actions taken:
  - Moved nearest-local selection logic into `ConfigBuilderInput::try_from(DiscoveryOutcome)`.
  - Removed the standalone `select_nearest_local` helper and its now-unused `DiscoveryAnchor` import.
- Verification:
  - `cargo test config::`: 87 passed.
  - `MISE_EXPERIMENTAL=1 mise run fmt`: passed.
  - `MISE_EXPERIMENTAL=1 mise run clippy`: passed after removing the unused `DiscoveryAnchor` import.
  - GitNexus `detect_changes(scope=all, worktree=.worktrees/config-typestate)`: low risk, 9 changed files, no indexed symbol/process impact.

### Config File Error Hierarchy Follow-up
- **Status:** complete
- Actions taken:
  - Made `ConfigFileError` the primary error type for config-file lifecycle transitions.
  - Changed parse and trust transition `TryFrom` impls to return `ConfigFileError`.
  - Kept `ConfigFileParseError` and `ConfigFileTrustError` as nested source-detail errors.
  - Simplified `ConfigBuilderError` to `Input` plus transparent `ConfigFile`.
  - Updated builder/service/file tests to match nested `ConfigFileError` variants.
- Errors encountered:
  - `mise://tasks` resource was unavailable; continued with known mise tasks.
  - LSP and GitNexus symbol lookups could not resolve the new/stale config error symbols; grep/read fallback mapped callsites.
  - First focused test compile exposed recursive error types through `ConfigFileError -> ConfigFileTrustError -> TrustError -> ConfigFileError`; boxed the nested `TrustError` source to break the cycle.
  - Full check initially failed on unused `PathBuf` and `TrustError` imports after removing builder trust variants; removed both imports.
- Verification:
  - `cargo test config::`: 87 passed.
  - `MISE_EXPERIMENTAL=1 mise run fmt`: passed.
  - `MISE_EXPERIMENTAL=1 mise run clippy`: passed.
  - `MISE_EXPERIMENTAL=1 mise run ci`: passed with existing non-failing `cargo deny` duplicate/unmatched-license warnings.
  - GitNexus `detect_changes(scope=all, worktree=.worktrees/config-typestate)`: low risk, 9 changed files, no indexed symbol/process impact.

### ConfigBuilderInput Precedence Encoding Follow-up
- **Status:** complete
- Actions taken:
  - Restored the selected local field name to `local`.
  - Added `ConfigBuilderInput` docs that state the full-load precedence policy: selected local config plus optional global config merged before local.
  - Replaced fallback-to-deepest-local behavior with `FullDiscoveryWithoutAnchorLocal { anchor }`.
  - Added a regression test for full discovery output whose locals do not contain the anchor.
- Errors encountered:
  - Initial `nearest_local` field rename did not match the requested terminology; restored `local`.
  - `mise run clippy` rejected `ok_or_else` for an eager error value; switched to `ok_or`.
- Verification:
  - `cargo test config::`: 88 passed.
  - `MISE_EXPERIMENTAL=1 mise run fmt`: passed.
  - `MISE_EXPERIMENTAL=1 mise run clippy`: passed.
  - `MISE_EXPERIMENTAL=1 mise run ci`: passed with existing non-failing `cargo deny` duplicate/unmatched-license warnings.
  - GitNexus `detect_changes(scope=all, worktree=.worktrees/config-typestate)`: low risk, 9 changed files, no indexed symbol/process impact.

### Config Organization Follow-up
- **Status:** complete
- Actions taken:
  - Left `src/config/domain.rs` and `src/config/mod.rs` unchanged per user instruction.
  - Reordered public-ish declarations, marker states, impl blocks, helpers, and tests across `raw.rs`, `dirs.rs`, `store.rs`, `tracker.rs`, `trust.rs`, `file.rs`, `builder.rs`, `discovery.rs`, and `service.rs`.
  - Kept behavior unchanged; edits were structural/readability-oriented.
- Verification:
  - `MISE_EXPERIMENTAL=1 mise run fmt`: passed.
  - `cargo test config::`: 88 passed.
  - `MISE_EXPERIMENTAL=1 mise run clippy`: passed.
  - `MISE_EXPERIMENTAL=1 mise run ci`: passed; 158 tests passed, with existing non-failing `cargo deny` duplicate/unmatched-license warnings.
  - GitNexus `detect_changes(scope=all, worktree=.worktrees/config-typestate)`: low risk, 9 changed files, no indexed symbol/process impact.

### User Correction Follow-up
- **Status:** complete
- Actions taken:
  - Moved `ConfigFileError`, `ConfigFileParseError`, and `ConfigFileTrustError` below the `ConfigFile<State>` impl/conversion blocks so they no longer separate the type from its core interface.
  - Added the builder aggregate `Discovered` state and changed the initial impl to `impl ConfigBuilder<Discovered>`.
  - Added a regression for `traces trust PATH` where `PATH` is a directory with no `.traces/config.toml`; the red test reproduced the collapse into `LocalConfigAbsent`.
  - Replaced `ResolvedTrustTarget`/`ResolvedTrustInput` with borrowed `TrustInput` values visited synchronously by `visit_trust_inputs`.
  - Renamed `TrustTarget` to `TrustInput`; `TrustTargetType` was rejected because the enum is data-bearing operation input, not a pure kind tag.
  - Removed trust-target resolution from `ConfigService`; `cli::trust` now borrows each resolved input and immediately calls store-backed service operations.
- Errors encountered:
  - `cargo test config:: cli::trust::` is invalid because Cargo accepts one test filter; reran as `cargo test config:: && cargo test cli::trust::`.
  - E0382 after moving `start` into a file anchor; cloned the small `PathBuf` into `DiscoveryAnchor::File`.
  - GitNexus impact and LSP could not resolve `TrustTarget`/`resolve_trust_targets`; used grep/read callsite inspection and final GitNexus `detect_changes`.
- Verification:
  - `MISE_EXPERIMENTAL=1 mise run fmt`: passed.
  - `cargo test config::`: 88 passed.
  - `cargo test cli::trust::`: 17 passed.
  - `MISE_EXPERIMENTAL=1 mise run clippy`: passed.
  - `MISE_EXPERIMENTAL=1 mise run ci`: 158 passed, with existing non-failing `cargo deny` duplicate/unmatched-license warnings.
  - GitNexus `detect_changes(scope=all, worktree=.worktrees/config-typestate)`: low risk, 14 changed files, no indexed symbol/process impact.






## Session: 2026-07-17

### Phase 1: Establish Shared Design Questions
- **Status:** complete
- **Started:** 2026-07-17
- Actions taken:
  - Ran planning-with-files session catchup for `/Users/jack/Documents/41_personal/traces-pkm`; no unsynced context was reported.
  - Read planning-with-files templates for task plan, findings, and progress files.
  - Read grilling skill; must ask one design decision question at a time and wait for user feedback.
  - Captured user proposal: keep `ConfigSource`, introduce `ConfigFile<State>` for single-file lifecycle, keep `ConfigBuilder<State>` for aggregate final `Config` construction, and reconsider `ConfigLocator` as a testable module.
  - Resolved first grilling decision: `ConfigFile<State>` state is lifecycle-oriented; `ConfigSource` can condition methods within states.
- Files created/modified:
  - `task_plan.md` created and updated.
  - `findings.md` created and updated.
  - `progress.md` created and updated.

### Phase 2: Clarify Type Responsibilities
- **Status:** complete
- **Started:** 2026-07-17
- Actions taken:
  - Preparing next grilling question: which lifecycle states should exist for one config file.
  - Resolved second grilling decision: keep a tracked lifecycle state because trusted configs must be tracked first, but tracked configs may remain untrusted.
  - Captured open correction: global config files likely bypass `Tracked` and trust-gate states, transitioning directly from `Located` to `Parsed`.
  - Resolved third grilling decision: `ConfigBuilder` can remove its standalone `Tracked` state by combining local track/trust orchestration into one aggregate builder transition while `ConfigFile` still models both per-file transitions.
  - Resolved fourth grilling decision: no builder-level `Parsed` state; collapsed builder state should be storage-oriented (`Stored`/`LocalStored`) because parsing can remain internal to build/merge.
  - User requested subagent research into other Rust multi-level config systems and Mise config handling before deciding whether `ConfigBuilder` is a deep module.
  - Replaced failed broad research agent with two narrower agents: Helix config and rustfmt config.
  - Completed external research via subagents: Mise, Cargo, Helix, and rustfmt. Logged synthesis in `findings.md`.
  - Resolved fifth grilling decision: keep `ConfigBuilder` only for aggregate merge/domain construction; likely states are `Discovered -> LocalStored -> Merged`.
  - Captured service-level caveat: expose `ConfigService::load()` or reduce `discover()`/`build()` visibility to avoid builder usage without discovery.
  - Resolved sixth grilling decision: `ConfigService::load(cwd)` should be the only normal config-loading entry point; `discover()` and `build()` become private implementation details.
  - Resolved seventh grilling decision: source-specific validating constructors should derive root from path and replace raw root/source construction.
  - Captured open design question: whether `CandidateConfigFile` should disappear into the first `ConfigFile<State>`.
  - Captured module-shape preference: keep config location/discovery responsibilities in `src/config/discovery.rs` rather than adding a separate locator module.
  - Resolved eighth and ninth decisions: first config-file state is `Discovered`, and `DiscoveryOutcome` stores `ConfigFile<Discovered>` instead of `CandidateConfigFile`.
  - Captured revised discovery architecture: keep `DiscoveryProcessor`, add `DiscoveryEngine`/`ConfigDiscoveryEngine`, and consider `DiscoveryContext { anchor, kind }`.
  - Resolved discovery-context representation: use private fields plus smart constructors, not an enum of cases, so future fields such as environment can be added without reshaping all variants.
  - Reopened trust-target shape: file trust targets likely should not store root redundantly if root can be derived from the config file/trust-root algorithm.
  - Resolved trust-target shape: `TrustTarget` should support directory input, file input, and tracked config-file input: `Directory(&Path)`, `File(&Path)`, and `ConfigFile(&ConfigFile<Tracked>)`.
  - Resolved discovery kind names: `Full`, `NearestLocal`, and later-renamed `LocalSubtree`.
  - Resolved error ownership decision: absence belongs to `DiscoveryError`, not `ConfigFileError`.
  - Resolved file/discovery error layering: `DiscoveryError` wraps `ConfigFileError` instead of duplicating file/path validation variants.
  - Rejected separate trust-target-resolution error/component as likely wrong; next step is to map all trust routes and use discovery components directly.
  - Resolved trust routing direction: trust should route through `ConfigFile` and discovery components, not a trust-specific resolution layer.
  - Captured optional-search concern: nearest-local discovery may need both optional and required APIs because init can treat absence as useful information.
  - Refined load route: `ConfigService::load(cwd)` should call a discovery `process()` method that runs `DiscoveryProcessor` for `Full` discovery, instead of manually composing nearest-local calls.
  - Refined discovery API: discovery methods should take `DiscoveryContext`, and all discovery kinds can return `DiscoveryOutcome` instead of a separate output enum.
  - Resolved outcome shape: store `kind: DiscoveryScope` and `anchor: DiscoveryAnchor`, not the full `DiscoveryContext`; consider `DiscoveryContext::into_parts()`/`into_parts_ref()`.
  - Opened precedence decision: full discovery may find multiple local configs and needs a clear merge order.
  - Opened policy question: full load may need an effective-config selection step so `ConfigBuilder` receives only the chosen local/global files instead of all discovered files.
  - Resolved full-load selection policy: use nearest local plus optional global, not every discovered local config.
  - Captured naming concern: avoid `EffectiveConfigFiles`; consider a clearer name or a direct `DiscoveryOutcome` method.
  - Refined selected load-file type: use `ConfigBuilderInput` or `ConfigBuilderContext`, with preference leaning toward an input type that codifies precedence.
  - Captured parse-don't-validate enforcement: implement `TryFrom<DiscoveryOutcome>` so `ConfigBuilder` accepts only a validated builder input.
  - Resolved selected input naming: commit to `ConfigBuilderInput`; `ConfigBuilder::new` accepts only that type.
  - Resolved discovery engine storage for now: use a ZST `DiscoveryEngine` owned by `ConfigService`; revisit later whether it should hold `DiscoveryContext`.
  - Resolved context ownership: do not make `DiscoveryEngine` hold `DiscoveryContext` yet; context remains a per-call input.
  - Resolved discovery method seam: `DiscoveryEngine::process(ctx)` is the main public-ish method; kind-specific helpers stay private.
  - Resolved implementation ordering: user accepted the ten-step implementation sequence from `ConfigFile<Discovered>` through tests.
  - Recorded final design direction, rejected alternatives, and implementation order in `findings.md`.
- Files created/modified:
  - `task_plan.md`
  - `findings.md`
  - `progress.md`

## Test Results
| Test | Input | Expected | Actual | Status |
|------|-------|----------|--------|--------|
| External config research | Mise, Cargo, Helix, rustfmt primary-source/subagent studies | Identify patterns for config builder depth | Research completed; one broad agent failed and was replaced by narrower agents | Passed |
| Planning catchup | `session-catchup.py $(pwd)` | Identify prior unsynced planning context if any | No output; no unsynced context reported | Passed |

## Error Log
| Timestamp | Error | Attempt | Resolution |
|-----------|-------|---------|------------|
| 2026-07-17 | None | 0 | N/A |
| 2026-07-17 | RustConfigPatterns subagent failed with empty Cloud Code Assist response | 1 | Replace broad multi-project task with narrower per-project agents. |

## 5-Question Reboot Check
| Question | Answer |
|----------|--------|
| Where am I? | Final design recorded. |
| Where am I going? | Design grilling is complete; implementation can be started in a later task using the recorded order. |
| What's the goal? | Stress-test the proposed config typestate architecture without implementing changes. |
| What have I learned? | The final design centers on `ConfigFile<Discovered>`, `DiscoveryEngine::process(ctx)`, `DiscoveryOutcome`, `ConfigBuilderInput`, and `ConfigService::load(cwd)`. |
| What have I done? | Created planning files, completed external research, recorded final design direction, rejected alternatives, and accepted implementation order. |

---

### Trust State Store Refactor
- **Status:** implementation complete; verification in progress
- Worktree only: `/Users/jack/Documents/41_personal/traces-pkm/.worktrees/config-typestate`
- Actions taken:
  - Added `src/file_store.rs` as the crate-level `FileStateStore` and moved hash-keyed symlink/path-file mechanics out of `config/store.rs`.
  - Moved path canonicalization into `FileStateStore::entry_for`, leaving `Blake3PathHash` as a pure path-byte hash.
  - Added `ConfigStateStore`, `TrustSubject`, `DiscoveryScope`, workspace/config trust status types, and config-state errors in `src/config/store.rs`.
  - Routed trust resolution through `DiscoveryEngine::trust_subjects(path, scope)` using a single path, not cwd/path pairs.
  - Updated `ConfigService`, `ConfigBuilder`, `ConfigFile` transitions, and `traces trust` CLI to use the unified state store.
  - Deleted obsolete `src/config/store.rs`, `src/config/tracker.rs`, and `src/config/trust.rs`.
- Focused verification so far:
  - `cargo test config::`: passed.
  - `cargo test cli::trust::`: passed.
  - `cargo test hash::`: passed.
  - `MISE_EXPERIMENTAL=1 mise run fmt`: passed.

- Final verification:
  - `MISE_EXPERIMENTAL=1 mise run fmt`: passed after reconciliation.
  - `cargo test config::`: 47 passed.
  - `cargo test cli::trust::`: 17 passed.
  - `cargo test hash::`: 7 passed.
  - `MISE_EXPERIMENTAL=1 mise run clippy`: passed with `-D warnings`.
  - `MISE_EXPERIMENTAL=1 mise run ci`: passed; 139 tests passed, with existing non-failing `cargo deny` duplicate/unmatched-license warnings.
  - GitNexus `detect_changes(scope=all, worktree=.worktrees/config-typestate)`: low risk, 14 changed files, no changed indexed symbols, no affected processes.
- Reconciliation notes:
  - Removed obsolete `config/store.rs`, `config/tracker.rs`, and `config/trust.rs` from module graph and filesystem.
  - Kept I/O-bearing lifecycle conversions using `ConfigStateStore`.
  - Added clippy `unused_self` expectations only for intentional service/ZST seams.

### Follow-up Corrections
- Renamed `src/file_state_store.rs` to `src/file_store.rs`.
- Renamed `src/config/state.rs` to `src/config/store.rs`.
- Moved state directory roots to crate-level `src/dirs.rs`; `FileStateStore` now stores a `StateDirRoot`.
- Moved path canonicalization out of `Blake3PathHash`; `FileStateStore::entry_for` now canonicalizes once and carries the lint `#[expect]`.
- `traces trust` default target now reads `Cwd` instead of passing `.`.
- Renamed `DiscoveryType` to `DiscoveryScope` and removed separate `TrustScope`; trust uses `NearestLocal`/`LocalSubtree`, while `Full` is rejected for trust target resolution.
- Simplified `TrustSubject` to one owned path plus root/config kind; root is derived for config subjects.
- Replaced callback-based trust traversal with `TrustSubjects`, an `IntoIterator` wrapper returned by discovery/service.
- Verification after corrections so far: focused `config::`, `cli::trust::`, `file_store::`, and `hash::` tests passed; `mise run fmt`, `mise run clippy`, and `mise run ci` passed.

- Final correction verification:
  - GitNexus `detect_changes(scope=all, worktree=.worktrees/config-typestate)`: low risk, 14 changed indexed files, no changed indexed symbols, no affected processes.
  - Working tree status after renames: 17 unstaged tracked changes and 2 untracked new files (`src/dirs.rs`, `src/file_store.rs`).

### File Store Deepening Refactor
- Made `Blake3PathHash` pure: it hashes the supplied path bytes, returns no `Result`, exposes `as_str()`, and no longer implements `AsRef<Path>`.
- Moved canonicalization into private `FileStateStore::entry_for`, which returns a private `StoreEntry { canonical_target, path }` and canonicalizes exactly once per operation.
- Removed caller-facing path helpers (`entry_path`, `companion_path_for`, companion path methods); companion path construction is now private file-store implementation.
- Deepened companion operations: config store now uses `write_companion`, `read_companion`, `remove_with_companions`, and `clean_with_companions` instead of assembling entry paths.
- Moved path canonicalization errors from `HashError` to `FileStateStoreError::Canonicalize`; `HashError` is now file-content hashing only.

- Follow-up cleanup for file-store shape:
  - Replaced `FileStateStore::entry_for()` with `StoreEntry: TryFrom<&Path>`; `StoreEntry` now owns canonical target + hash, and projects into a store root with `path_in()`.
  - Inlined one-use removal helpers (`remove_optional_file`, `remove_required_file`) and removed the duplicate optional companion helper; removal/error mapping now lives at the operation that needs it.

- CleanMode follow-up:
  - Added `FileStoreCleanMode` to make companion cleanup an explicit `FileStateStore::clean(...)` policy.
  - Removed `clean_with_companions()` and updated config store/tests to use `EntriesOnly` or `WithCompanions`.
