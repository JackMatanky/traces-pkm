# Task Plan: Config Typestate Design Grilling

## Goal
Implement the accepted config discovery/loading/trust typestate refactor in an isolated worktree: `ConfigFile<State>` for single-file lifecycle invariants, `DiscoveryEngine`/`DiscoveryContext` inside `src/config/discovery.rs`, `ConfigBuilderInput` for load precedence, and `ConfigBuilder<State>` for aggregate final `Config` construction.

## Current Phase
ConfigBuilderInput precedence encoding cleanup complete in `.worktrees/config-typestate`; the field remains `local`, docs and error variants now describe the full-load precedence policy, the fallback selection path was replaced with an explicit invariant error, and verification passed via formatter, focused tests, clippy/CI, and GitNexus change detection.
## Phases

### Phase 1: Establish Shared Design Questions
- [x] Capture the user's proposed split between `ConfigFile<State>` and `ConfigBuilder<State>`.
- [x] Capture constraints: discuss only; do not implement changes.
- [x] Identify the first design decision to grill: whether `ConfigFile<State>` should encode per-file lifecycle stages or only path/source validation.
- **Status:** complete

### Phase 2: Clarify Type Responsibilities
- [x] Decide what invariants belong to `ConfigSource` and source-specific constructors.
- [x] Decide what invariants belong to `ConfigFile<State>`: lifecycle states, with source-conditioned methods where needed.
- [x] Decide what remains in `ConfigBuilder<State>`: aggregate merge/domain construction with shape `Discovered -> LocalStored -> Merged`.
- [x] Decide whether `CandidateConfigFile` is replaced by initial `ConfigFile<State>`.
- [x] Decide `DiscoveryEngine`, `DiscoveryContext`, anchor, and discovery kind invariants.
- [x] Decide revised trust target shape after discovery-anchor design.
- **Status:** complete

### Phase 2b: External Config-Builder Research
- [x] Research `jdx/mise` config handling from primary source.
- [x] Research other Rust projects with multi-level configs.
- [x] Extract lessons for whether `ConfigBuilder` is a deep module.
- **Status:** complete

### Phase 3: Clarify Error Model
- [x] Evaluate `ConfigFileError` with `PathInaccessible`, `UnsupportedLocalConfigFile`, and `UnsupportedGlobalConfigFile`.
- [x] Decide whether absence belongs in `DiscoveryError`, `ConfigLookupError`, or a locator-level error.
- [x] Rethink trust routes to remove a separate trust-target resolution component.
- [x] Decide optional versus required nearest-local discovery APIs for init/trust/load.
- [x] Decide whether `DiscoveryOutcome` stores raw anchor or full `DiscoveryContext`.
- **Status:** complete

### Phase 4: Clarify Discovery Engine
- [x] Define the `DiscoveryEngine` interface inside `src/config/discovery.rs`.
- [x] Decide whether `DiscoveryEngine` should be a ZST type, stateful type, or plain module functions.
- [x] Define local config precedence when full discovery finds multiple locals.
- **Status:** complete

### Phase 5: Record Final Design Direction
- [x] Summarize accepted decisions.
- [x] List rejected alternatives and reasons.
- [x] Identify implementation order for a later task, without implementing now.
- **Status:** complete

### Phase 6: Planning Artifact Consistency Review
- [x] Run planning-with-files session catchup.
- [x] Review `task_plan.md`, `findings.md`, and `progress.md` for stale decisions.
- [x] Compare accepted design against current source shape.
- [x] Record implementation-readiness holes before any Rust source changes.
- **Status:** complete

### Phase 7: Implement Config Typestate Refactor
- [x] Create `.worktrees/config-typestate` implementation worktree.
- [x] Add `ConfigFile<State>` lifecycle type and remove `CandidateConfigFile`.
- [x] Implement `DiscoveryContext`, `DiscoveryType`, `DiscoveryAnchor`, `DiscoveryEngine`, and unified `DiscoveryOutcome`.
- [x] Add `ConfigBuilderInput` and load selection policy.
- [x] Collapse `ConfigBuilder` to `ConfigBuilderInput -> LocalStored -> Merged`.
- [x] Add `ConfigService::load(cwd)` and route trust target discovery through `DiscoveryEngine`.
- [x] Apply correction pass: complete `Parsed` lifecycle, remove generic `ConfigFile<State>::parse()`, replace lifecycle helper methods with `From`/`TryFrom` conversions over tuple inputs, make tracked conversion record through `ConfigTracker`, make `is_trusted` consume `TrustTarget`, remove `ResolvedTrustTarget`, validate discovery context combinations, remove outcome `cwd`, move helper logic into `DiscoveryEngine`, enforce nearest-local builder input selection, and remove redundant `ConfigFile<State>` phantom state storage.
- [x] Run `cargo test config::`, `mise run fmt`, `mise run test`, `mise run clippy`, `mise run ci`, and GitNexus change detection.
- **Status:** complete

## Implementation-Readiness Decisions
1. Global configs parse through `TryFrom<ConfigFile<Discovered>> for ConfigFile<Parsed>`; local discovered configs fail that conversion until tracked and trusted.
2. `TrustTarget` shape is settled as `File(&Path)`, `Directory(&Path)`, and `ConfigFile(&ConfigFile<Tracked>)`.
3. Unified `DiscoveryOutcome` is settled.
4. `ConfigBuilderInput { local: ConfigFile<Discovered>, global: Option<ConfigFile<Discovered>> }` is settled.
5. ZST `DiscoveryEngine` is settled.
6. Discovery type `LocalSubtree` is settled.
7. Error policy is settled: `LocalConfigAbsent` belongs in `DiscoveryError`; `ConfigBuilderInputError` only needs wrong-kind/invariant variants such as `WrongDiscoveryKindForBuild`.

## Decisions Made
| Decision | Rationale |
|----------|-----------|
| Use planning files for this design grilling session | User explicitly requested persistent planning files for session tracking. |
| Implement in isolated worktree after design decisions settled | User moved from design grilling into implementation; code changes live under `.worktrees/config-typestate`. |
| Treat `ConfigFile<State>` and `ConfigBuilder<State>` as separate proposed typestate patterns | User clarified they are intentionally distinct: single-file lifecycle vs aggregate domain config construction. |
| Make `ConfigFile<State>` lifecycle-oriented, not source-oriented | User agreed state should model per-file lifecycle; `ConfigSource` conditions some methods inside those states. |
| Collapse builder tracking/trust into one aggregate state | User observed `ConfigBuilder` no longer needs separate `Tracked`; it can delegate local file `Tracked` and `Trusted` transitions internally to `ConfigFile<State>`. |
| Remove builder-level `Parsed` state from the design | User clarified parsing can remain inside the final build/merge step; builder state should focus on aggregate storage readiness, not parsed-file lifecycle. |
| Name the collapsed builder state around storage, not parsing | User prefers `Stored` or `LocalStored` because the aggregate state is about `ConfigFileStore` interactions. |
| Keep a tracked lifecycle state in `ConfigFile<State>` | User wants the model to preserve that all trusted configs must first be tracked, while not all tracked configs become trusted. |
| Pause final builder-state naming pending external research | User questioned whether `ConfigBuilder` is a deep module after discussing `Completed`/`Merged`; research will compare against Rust projects with multi-level config systems. |
| Keep `ConfigBuilder` only for aggregate concerns | User agreed builder should center on aggregate merge/domain construction; likely shape is `ConfigBuilder<Discovered> -> ConfigBuilder<LocalStored> -> ConfigBuilder<Merged>`. |
| Add a single `ConfigService::load()` entry point or reduce discover/build visibility | User wants to prevent callers from running builder phases without discovery. |
| Make `ConfigService::load(cwd)` the only normal config-loading entry point | User agreed `discover()` and `build()` should become private implementation details to enforce discovery-before-build at the service seam. |
| Replace raw candidate constructors with source-specific validating constructors | User agreed constructors should derive root from path and make root/source mismatch unrepresentable. |
| Consider folding `CandidateConfigFile` into initial `ConfigFile<State>` | User questioned why a separate candidate type remains if `ConfigFile<Discovered/Candidate/Located>` can own the same constructors, methods, and invariants. |
| Keep locator/discovery responsibility inside `src/config/discovery.rs` | User prefers a clearly defined discovery module, possibly with a renamed type such as `ConfigDiscovery`, `ConfigDiscover`, or `ConfigExplorer`, rather than a separate `ConfigLocator` module. |
| Name the first config-file lifecycle state `Discovered` | User wants `ConfigFile<Discovered>` as the first state and also wants `Discovered` shared with `ConfigBuilder<Discovered>`. |
| Replace `CandidateConfigFile` with `ConfigFile<Discovered>` in `DiscoveryOutcome` | User decided `DiscoveryOutcome` should store discovered config files directly instead of a separate candidate type. |
| Keep `DiscoveryProcessor` and add a discovery engine | User clarified `DiscoveryProcessor` should remain the full discovery typestate processor, managed by a `DiscoveryEngine`/`ConfigDiscoveryEngine` rather than by `ConfigService`. |
| Explore `DiscoveryContext` with anchor and kind enums | User proposed `DiscoveryContext { anchor, kind }`, with `DiscoveryAnchor::{Directory, File}` and discovery kinds such as `Full`, `LocalOnly`, and `NearestLocal`; full discovery must be directory/cwd anchored, not file anchored. |
| Use private-field `DiscoveryContext` with smart constructors | User prefers the struct form over an enum because it can add context fields later, such as environment variables, while constructors preserve invariants like `Full` requiring a directory/cwd anchor. |
| Revisit `TrustTarget` so file targets do not redundantly carry root | User pointed back to Mise's `src/cli/trust.rs`, where trust resolves a config file to a trust root instead of storing both root and file in the target. |
| Design `TrustTarget` with directory, file, and tracked-config variants | User proposed `TrustTarget::{Directory(&Path), File(&Path), ConfigFile(&ConfigFile<Tracked>)}` so CLI path input and config-loading trust checks share one trust target vocabulary without duplicating root/file fields. |
| Use discovery kinds `Full`, `NearestLocal`, and `LocalSubtree` | User initially chose `AllLocalDescendents`, then renamed it to `LocalSubtree` before implementation. |
| Keep absence in `DiscoveryError`, not `ConfigFileError` | User agreed missing search results are discovery operation outcomes, not errors about a specific config-file path. |
| Have `DiscoveryError` wrap `ConfigFileError` | User agreed discovery should not duplicate file/path validation variants; specific config-file construction failures bubble through discovery. |
| Rethink trust routes instead of adding trust-target resolution errors | User observed nested discovery/config-file errors inside a trust-target-resolution error suggests the component boundary is wrong; discovery components should probably make a separate resolution layer unnecessary. |
| Route trust through discovery and config-file lifecycles, not trust resolution | User agreed the design is better when trust uses `ConfigFile` and discovery components directly, with no trust-specific resolution layer. |
| Keep nearest-local absence in discovery but consider optional search APIs | User agreed absence belongs to discovery, but noted `nearest_local` should not always error because init may use absence to create a new local config. |
| Let `ConfigService::load(cwd)` call discovery processing directly | User noted load can simply call a `process()` method that runs `DiscoveryProcessor`; this keeps full discovery hidden behind discovery components rather than decomposing load into nearest-local calls. |
| Use `DiscoveryContext` as discovery method input | User challenged passing raw anchors to focused discovery methods; context should be the input shape for each discovery operation. |
| Use unified `DiscoveryOutcome` for all discovery kinds | User observed `DiscoveryOutcome` can represent `Full`, `NearestLocal`, and `LocalSubtree` by varying local/global cardinality, avoiding a separate `DiscoveryOutput` enum. |
| Store discovery kind and anchor in `DiscoveryOutcome`, not full context | User decided only `DiscoveryType`, `DiscoveryAnchor`, local files, and global files remain relevant after discovery; `DiscoveryContext` may expose `into_parts()`/`into_parts_ref()`. |
| Define explicit precedence for multiple local configs in full discovery | User noted full discovery can theoretically find more than one local config, so merge precedence must be clear. |
| Full load selects nearest local plus optional global | User decided full config loading should not merge every discovered local config; it should use only the nearest local config, with optional global config. |
| Avoid the name `EffectiveConfigFiles` | User wants a better name, or possibly just a method on `DiscoveryOutcome`, for selecting the files used by config loading. |
| Name selected load files `ConfigBuilderInput` | User prefers `ConfigBuilderInput` or `ConfigBuilderContext`; the selected type should codify precedence policy before construction reaches `ConfigBuilder`. |
| Use `TryFrom<DiscoveryOutcome>` to parse discovery into builder input | User referenced `api-parse-dont-validate`; conversion should produce a validated type so `ConfigBuilder` cannot be constructed from invalid or unselected discovery data. |
| Commit to `ConfigBuilderInput` and builder-only construction | User agreed `ConfigBuilder::new` should accept only `ConfigBuilderInput`, so discovery selection/precedence is parsed before the builder boundary. |
| Make `DiscoveryEngine` a ZST owned by `ConfigService` for now | User agreed with the ZST collaborator shape, while noting context ownership may need reevaluation later. |
| Revisit whether `DiscoveryEngine` should hold `DiscoveryContext` | User flagged a possible future design where the engine owns context rather than accepting it per call. |
| Do not make `DiscoveryEngine` hold `DiscoveryContext` yet | User decided context remains per-call input for now; any context-owning discovery run can be considered later if needed. |
| Make `DiscoveryEngine::process(ctx)` the main discovery method | User agreed `process(ctx)` is the public-ish discovery seam, with kind-specific helper methods kept private. |
| Accept staged implementation order | User agreed the proposed ten-step implementation order is good. |

## Errors Encountered
| Error | Attempt | Resolution |
|-------|---------|------------|
| RustConfigPatterns subagent failed with empty Cloud Code Assist response | 1 | Mutate approach: replace one broad multi-project task with narrower per-project research agents. |
| `mise://tasks` resource unavailable in harness | 1 | Used already-known `mise run` project tasks and recorded the lookup failure. |
| GitNexus/LSP could not resolve new config error symbols | 1 | Used grep/read fallback for callsite mapping; GitNexus final `detect_changes` still reported low risk. |
| Initial error-hierarchy refactor produced recursive error types and stale tests | 1 | Boxed nested `TrustError` source inside `ConfigFileTrustError`, updated nested matches, and reran focused tests. |
| Full check failed on unused `PathBuf`/`TrustError` imports after builder simplification | 1 | Removed the imports, reran formatter, clippy, and CI successfully. |
| Renaming `ConfigBuilderInput.local` to `nearest_local` made the type less aligned with requested terminology | 1 | Restored `local`, moved the precedence description into docs, variable names, and explicit invariant errors. |
| Clippy rejected `ok_or_else` for an eager error value | 1 | Switched to `ok_or` and reran clippy/CI successfully. |
## Notes
- Worktree containing the prior implementation commit: `/Users/jack/Documents/41_personal/traces-pkm-init-cli`.
- Current planning files are in project root: `/Users/jack/Documents/41_personal/traces-pkm`.
- Follow grilling skill: ask one decision question at a time, recommend an answer, and wait for user feedback.
