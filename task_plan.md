# Task Plan: Config Typestate Design Grilling

## Goal
Stress-test and refine the proposed config architecture without implementing changes: `ConfigFile<State>` for single-file lifecycle invariants, `ConfigBuilder<State>` for building the final domain `Config`, and a `ConfigLocator` module for config path lookup.

## Current Phase
Phase 2: Clarify Type Responsibilities — External Config-Builder Research

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
- [ ] Decide optional versus required nearest-local discovery APIs for init/trust/load.
- **Status:** in_progress

### Phase 4: Clarify Locator Module
- [ ] Define the `ConfigLocator` interface.
- [ ] Decide whether `ConfigLocator` should be a ZST type, stateful type, or plain module functions.
- [ ] Decide how ascending and descending walks are tested in isolation.
- **Status:** pending

### Phase 5: Record Final Design Direction
- [ ] Summarize accepted decisions.
- [ ] List rejected alternatives and reasons.
- [ ] Identify implementation order for a later task, without implementing now.
- **Status:** pending

## Key Questions
1. What exact single-file lifecycle states should `ConfigFile<State>` encode?
2. Should `ConfigFile<State>` encode tracking/trust states when global configs skip those stages?
6. Should global config files bypass tracking/trust states and parse directly from `Located`?
3. Should parsing produce `ConfigFile<Parsed>` carrying `RawConfig`, or should parsing remain inside `ConfigBuilder`?
4. Does `ConfigBuilder<State>` own a collection of typed config files, or does it remain a pipeline wrapper around `DiscoveryOutcome`?
5. Is `ConfigLocator` a deep module with config-specific semantics, or a shallow wrapper over filesystem walks?

## Decisions Made
| Decision | Rationale |
|----------|-----------|
| Use planning files for this design grilling session | User explicitly requested persistent planning files for session tracking. |
| Do not implement code changes during this session | User explicitly said to consider/respond and now to track grilling, not edit implementation. |
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
| Use discovery kinds `Full`, `NearestLocal`, and `AllLocalDescendents` | User chose these names for the discovery context kind enum. |
| Keep absence in `DiscoveryError`, not `ConfigFileError` | User agreed missing search results are discovery operation outcomes, not errors about a specific config-file path. |
| Have `DiscoveryError` wrap `ConfigFileError` | User agreed discovery should not duplicate file/path validation variants; specific config-file construction failures bubble through discovery. |
| Rethink trust routes instead of adding trust-target resolution errors | User observed nested discovery/config-file errors inside a trust-target-resolution error suggests the component boundary is wrong; discovery components should probably make a separate resolution layer unnecessary. |
| Route trust through discovery and config-file lifecycles, not trust resolution | User agreed the design is better when trust uses `ConfigFile` and discovery components directly, with no trust-specific resolution layer. |
| Keep nearest-local absence in discovery but consider optional search APIs | User agreed absence belongs to discovery, but noted `nearest_local` should not always error because init may use absence to create a new local config. |

## Errors Encountered
| RustConfigPatterns subagent failed with empty Cloud Code Assist response | 1 | Mutate approach: replace one broad multi-project task with narrower per-project research agents. |

## Notes
- Worktree containing the prior implementation commit: `/Users/jack/Documents/41_personal/traces-pkm-init-cli`.
- Current planning files are in project root: `/Users/jack/Documents/41_personal/traces-pkm`.
- Follow grilling skill: ask one decision question at a time, recommend an answer, and wait for user feedback.
