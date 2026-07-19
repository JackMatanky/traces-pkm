# Findings & Decisions

## Requirements
- Track this session in persistent planning files: `task_plan.md`, `findings.md`, and `progress.md`.
- Implement the accepted config typestate refactor in `.worktrees/config-typestate` after design grilling settled the open questions.
- Explore two separate typestate patterns:
  - `ConfigFile<State>`: lifecycle/invariants for a single config file.
  - `ConfigBuilder<State>`: aggregate builder that consumes all discovered config files and produces final domain `Config`.
- Preserve consideration of `ConfigSource` as useful domain enum and path-validation discriminator.
- Keep config lookup/discovery responsibility inside `src/config/discovery.rs`; the earlier `ConfigLocator` idea was rejected as a separate module.

## Research Findings
- Implemented state: `src/config/file.rs` owns `ConfigFile<State>`, `ConfigSource`, and source-specific `local`/`global` constructors.
- Implemented state: `src/config/discovery.rs` owns `DiscoveryContext`, `DiscoveryType`, `DiscoveryAnchor`, ZST `DiscoveryEngine`, `DiscoveryOutcome`, and full/nearest-local/local-subtree discovery.
- Implemented state: `src/config/builder.rs` consumes `ConfigBuilderInput` and builds through aggregate states `Discovered -> LocalStored -> Merged`.
- Implemented state: `src/config/service.rs` exposes `load(cwd)` and routes trust-target discovery through discovery, while `src/config/trust.rs` keeps trust-store recording/checking only.
- Rust typestate guidance applies when invalid transitions should become compile errors; enum-state guidance applies when a value is one of mutually exclusive runtime states.
- New research question resolved: `ConfigBuilder<State>` is useful only if it remains a deep aggregate merge/build module after `ConfigFile<State>` and discovery own per-file lifecycle and lookup behavior.
- Required external primary-source research: `github.com/jdx/mise/blob/main/src/config/mod.rs` and other Rust projects with multi-level config handling.
- Initial Mise source read: `Config::load()` in `src/config/mod.rs` orchestrates loading idiomatic/default config filenames, config paths, config files, vars, aliases, shell aliases, project root, plugins, validation, and tool aliases in one `Config` construction flow; no obvious separate typestate builder in the visible top-level flow.
- Initial Cargo docs read: Cargo documents hierarchical `.cargo/config.toml` discovery from cwd ancestors plus `$CARGO_HOME`, deterministic merge precedence, env var overrides, and `--config` CLI overrides; this is a primary-source example where the public concept is hierarchical config merging rather than a visible typestate builder.
- Mise `load_config_paths()` collects config paths from `all_dirs()`, extends with global and system config files, deduplicates by desymlinked path, and filters ignored paths; this is a locator-style function rather than a builder state machine.
- Mise `load_all_config_files()` parses each config path, tracks it best-effort with `Tracker::track(f)` and inserts parsed `Arc<dyn ConfigFile>` into a `ConfigMap`; tracking is an implementation detail of loading, not a public typestate.
- Mise `get_tracked_config_files()` pre-checks trust for tracked configs before parsing to avoid interactive prompts, but falls through for trust-exempt/parseable config types; this suggests trust and tracking can be store/load policy rather than an aggregate builder state visible to callers.
- Mise agent conclusion: Mise represents sources as `Arc<dyn ConfigFile>` in a `ConfigMap`, with a large `Config::load()` orchestration rather than a separate builder; lesson is to avoid a monolithic final `Config` that also performs loading, but keep path location and config-file representation explicit.
- Cargo agent conclusion: Cargo's `GlobalContext` loads hierarchical config by walking ancestors, merges into a `ConfigValue` tree carrying `Definition` metadata (`BuiltIn`, path, environment, CLI), and applies env/CLI precedence at value access time; no typestate builder, but strong source metadata.
- Helix agent conclusion: Helix merges default/global/workspace TOML values before deserializing to final typed config; workspace config is trust-conditioned. Lesson: a builder can be thin if raw/partial merge logic is centralized.
- rustfmt agent conclusion: rustfmt uses direct discovery, `PartialConfig` with `Option<T>` fields, and final `Config` value metadata per field; no builder, but rich per-value metadata supports diagnostics.
- Cross-project pattern so far: mature Rust config systems usually make discovery, source metadata, and merge precedence deep; they rarely expose a typestate builder as a public concept. A builder is useful here only if it concentrates sequencing/trust/storage decisions better than simple functions would.

## Technical Decisions
| Decision | Rationale |
|----------|-----------|
| Keep `ConfigSource` in the design discussion | It models mutually exclusive source states and can support source-specific validation. |
| Reconsider scope-only typestate | User argued local/global typestate is shallow; `ConfigSource` may already cover that dimension better. |
| Evaluate lifecycle typestate for `ConfigFile<State>` | States like discovered, tracking attempted, trust checked, parsed may encode meaningful transitions. |
| Keep `ConfigBuilder<State>` as aggregate construction concept | Builder still owns final merge/precedence/domain `Config` creation across all files. |
| Make `ConfigFile<State>` lifecycle-oriented, not source-oriented | User agreed state should model per-file lifecycle; `ConfigSource` conditions some methods inside those states. |
| Keep a tracked lifecycle state in `ConfigFile<State>` | User wants the model to preserve that all trusted configs must first be tracked, while not all tracked configs become trusted. |
| Global configs bypass local tracking/trust states | User settled on `ConfigFile<Discovered>::try_into_global_parsed()` for global configs; local configs must pass tracking/trust lifecycle before parsing. |
| Collapse builder tracking/trust into one aggregate state | User observed `ConfigBuilder` no longer needs separate `Tracked`; it can delegate local file `Tracked` and `Trusted` transitions internally to `ConfigFile<State>`. |
| Remove builder-level `Parsed` state from the design | User clarified parsing can remain inside the final build/merge step; builder state should focus on aggregate storage readiness, not parsed-file lifecycle. |
| Name the collapsed builder state around storage, not parsing | User prefers `Stored` or `LocalStored` because the aggregate state is about `ConfigFileStore` interactions. |
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
| Use discovery kinds `Full`, `NearestLocal`, and `LocalSubtree` | User initially chose `AllLocalDescendents`, then renamed the subtree discovery kind to `LocalSubtree` before implementation. |
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
| Resolve select-effective versus merge-all policy | User decided full config loading should pass only nearest local plus optional global to the builder, not all discovered local configs. |
| Avoid the name `EffectiveConfigFiles` | User wants a better name, or possibly just a method on `DiscoveryOutcome`, for selecting the files used by config loading. |
| Name selected load files `ConfigBuilderInput` | User prefers `ConfigBuilderInput` or `ConfigBuilderContext`; the selected type should codify precedence policy before construction reaches `ConfigBuilder`. |
| Use `TryFrom<DiscoveryOutcome>` to parse discovery into builder input | User referenced `api-parse-dont-validate`; conversion should produce a validated type so `ConfigBuilder` cannot be constructed from invalid or unselected discovery data. |
| Commit to `ConfigBuilderInput` and builder-only construction | User agreed `ConfigBuilder::new` should accept only `ConfigBuilderInput`, so discovery selection/precedence is parsed before the builder boundary. |
| Make `DiscoveryEngine` a ZST owned by `ConfigService` for now | User agreed with the ZST collaborator shape, while noting context ownership may need reevaluation later. |
| Revisit whether `DiscoveryEngine` should hold `DiscoveryContext` | User flagged a possible future design where the engine owns context rather than accepting it per call. |
| Do not make `DiscoveryEngine` hold `DiscoveryContext` yet | User decided context remains per-call input for now; any context-owning discovery run can be considered later if needed. |
| Make `DiscoveryEngine::process(ctx)` the main discovery method | User agreed `process(ctx)` is the public-ish discovery seam, with kind-specific helper methods kept private. |

## Accepted Final Design Direction
- `ConfigFile<Discovered>` replaces `CandidateConfigFile`; source-specific constructors derive root from path and prevent root/source mismatch.
- `ConfigFile` lifecycle is per-file: discovered, tracked, trusted, parsed; global configs can bypass local tracking/trust transitions.
- `DiscoveryEngine` is a ZST collaborator owned by `ConfigService` for now. It receives `DiscoveryContext` per call and exposes `process(ctx)` as the main discovery method.
- `DiscoveryContext` has private fields and smart constructors. It uses `DiscoveryType::{Full, NearestLocal, LocalSubtree}` and `DiscoveryAnchor::{Directory, File}`.
- `DiscoveryOutcome` stores `kind`, `anchor`, `local: Box<[ConfigFile<Discovered>]>`, and `global: Box<[ConfigFile<Discovered>]>`.
- Full config loading selects nearest local plus optional global; it does not merge every discovered local config.
- `ConfigBuilderInput` is parsed from `DiscoveryOutcome` with `TryFrom`, codifying selection/precedence before reaching the builder.
- `ConfigBuilder::new` accepts only `ConfigBuilderInput`.
- `ConfigService::load(cwd)` is the normal load entry point; `discover()` and `build()` become private implementation details.
- Trust routes use discovery plus config-file lifecycle directly; no separate trust-target-resolution component.


## Planning Artifact Review — 2026-07-19
- Session catchup reported no unsynced context.
- `task_plan.md` had stale top-level goal/current-phase text that still mentioned a separate `ConfigLocator`; the accepted direction keeps discovery in `src/config/discovery.rs`.
- `task_plan.md` key questions were stale/resolved and have been replaced with implementation-readiness questions.
- `progress.md` had Phase 2 marked `in_progress` even though final design and implementation order were recorded; mark that phase complete when normalizing the progress log.
- Current source still matches the pre-refactor state:
  - `src/config/candidate.rs` defines `CandidateConfigFile`.
  - `src/config/discovery.rs` returns `DiscoveryOutcome { cwd, local: Box<[CandidateConfigFile]>, global: Box<[CandidateConfigFile]> }`.
  - `src/config/builder.rs` still uses builder states `Discovered -> Tracked -> Trusted -> Merged`.
  - `src/config/service.rs` still exposes `discover(cwd)` and `build(&DiscoveryOutcome)` instead of `load(cwd)`.
  - `src/config/trust.rs` still has `TrustTargetError`, `ResolvedTrustTarget`, and trust-target resolver functions.
- Implementation-order repair: do not delete `CandidateConfigFile` before migrating all consumers. Add `ConfigFile<State>` and constructors first, migrate discovery/builder/trust call sites, then remove `CandidateConfigFile`.
- Design clarifications to grill before coding:
  - Keep global config trust bypass via `ConfigFile<Discovered>::try_into_global_parsed()`.
  - Keep the decided `TrustTarget::{Directory, File, ConfigFile(&ConfigFile<Tracked>)}` shape; the remaining design question is how the service routes raw path variants through discovery without adding a trust-target-resolution module.
  - Keep unified `DiscoveryOutcome`; the remaining requirement is documenting and enforcing per-kind cardinality/ordering at construction and at the `ConfigBuilderInput` conversion.
  - Keep `ConfigBuilderInput { local: ConfigFile<Discovered>, global: Option<ConfigFile<Discovered>> }`; builder construction is the selected-file boundary, not the trust/parse boundary.
  - Reconfirm whether the ZST `DiscoveryEngine` earns its collaborator slot; current accepted decision keeps the ZST for now.

## Follow-up Answers — 2026-07-19
- Recommended global route: keep direct global parsing as `ConfigFile<Discovered>::try_into_global_parsed()`, and make it return a source-specific `ConfigFileError` if called on a local file. This keeps local trust semantics out of globals.
- Settled `TrustTarget` shape remains:
  ```rust
  enum TrustTarget<'a> {
      File(&'a Path),
      Directory(&'a Path),
      ConfigFile(&'a ConfigFile<Tracked>),
  }
  ```
- Unified `DiscoveryOutcome` remains acceptable if construction owns cardinality and ordering invariants, and `TryFrom<DiscoveryOutcome> for ConfigBuilderInput` is the only load-selection parser.
- Settled `ConfigBuilderInput` shape remains:
  ```rust
  struct ConfigBuilderInput {
      local: ConfigFile<Discovered>,
      global: Option<ConfigFile<Discovered>>,
  }
  ```
- `DiscoveryEngine` earns its seam if it hides discovery orchestration, owns `DiscoveryProcessor`, and gives `ConfigService` a small collaborator interface; a ZST is acceptable because it can become stateful later without changing callers.
- Preferred discovery kind name: `LocalSubtree`.
- `TryFrom<DiscoveryOutcome>` needs an explicit wrong-kind error because `DiscoveryOutcome` is unified across discovery kinds while `ConfigBuilderInput` is valid only for load/full discovery; `LocalConfigAbsent` remains a `DiscoveryError` produced before a full `DiscoveryOutcome` exists.

## Recommended Defaults Before Implementation — 2026-07-19
1. Global parse transition: use `ConfigFile<Discovered>::try_into_global_parsed()`.
2. Trust target routing: keep `TrustTarget::{File, Directory, ConfigFile}` at the `ConfigService` seam; do not let the lower `ConfigTrust` store adapter resolve raw paths.
3. Unified discovery output: keep unified `DiscoveryOutcome` with private fields and constructor-owned cardinality/ordering invariants; only `ConfigBuilderInput::try_from` should parse it for loading.
4. Builder input: keep the decided owned shape `ConfigBuilderInput { local: ConfigFile<Discovered>, global: Option<ConfigFile<Discovered>> }`; do not add `DiscoveryAnchor` unless a real builder use appears.
5. Discovery engine: keep the ZST `DiscoveryEngine` as the discovery orchestration module object; do not add a trait or extra abstraction.
6. Discovery kind name: use `LocalSubtree`.
7. Builder input errors: keep `LocalConfigAbsent` in `DiscoveryError`; use `ConfigBuilderInputError` only for wrong-kind or impossible-output invariants, preferably named `WrongDiscoveryKindForBuild { actual: DiscoveryType }` rather than generic `UnsupportedDiscoveryKind`.

## Rejected Alternatives
- `CandidateConfigFile` as a separate candidate type: redundant once `ConfigFile<Discovered>` owns path/source/root invariants.
- Scope-only `ConfigFile<Local>`/`ConfigFile<Global>` typestate: `ConfigSource` already models source; lifecycle states carry stronger invariants.
- Builder-level `Tracked` and `Parsed` states: per-file lifecycle and `ConfigBuilderInput` make these unnecessary.
- Merging all discovered local configs during full load: rejected in favor of nearest-local plus optional global.
- `EffectiveConfigFiles`: rejected as vague; use `ConfigBuilderInput`.
- `TrustTargetResolutionError`/resolution component: rejected as a bad boundary that would only wrap discovery/config-file errors.
- General `DiscoveryOutput` enum: unnecessary because `DiscoveryOutcome` can represent all discovery kinds through cardinality.
- `DiscoveryEngine` holding context now: deferred; context stays a per-call input.

## Implementation Order
1. Introduce `ConfigFile<State>` lifecycle markers and source-specific validating constructors while keeping `CandidateConfigFile` temporarily.
2. Add `DiscoveryContext`, `DiscoveryType`, `DiscoveryAnchor`, `ConfigFileError`, and updated `DiscoveryError` variants.
3. Add `DiscoveryEngine::process(ctx)` returning `DiscoveryOutcome`, with private helpers for `Full`, `NearestLocal`, and `LocalSubtree`.
4. Migrate `DiscoveryOutcome` to store `ConfigFile<Discovered>` and remove `CandidateConfigFile` only after all consumers are migrated.
5. Add `ConfigBuilderInput` and `TryFrom<DiscoveryOutcome>`, including the nearest-local-plus-optional-global precedence policy.
6. Collapse `ConfigBuilder` to aggregate construction concerns and change `ConfigBuilder::new` to accept only `ConfigBuilderInput`.
7. Add `ConfigService::load(cwd)` with a load-level error wrapper; make `discover()` and `build()` private implementation details.
8. Rework trust routes to use `DiscoveryEngine` and `ConfigFile` lifecycle values; remove trust-specific target resolution once discovery covers the routes.
9. Update CLI error wrappers and config module exports.
10. Update focused tests, then run project checks through mise tasks.

## Implementation Results — 2026-07-19
- Worktree: `/Users/jack/Documents/41_personal/traces-pkm/.worktrees/config-typestate`.
- `CandidateConfigFile` and `src/config/candidate.rs` were removed after all discovery/builder/trust consumers migrated to `ConfigFile<Discovered>`.
- Correction pass completed the file lifecycle through conversions: discovered locals become tracked through `From<(ConfigFile<Discovered>, &ConfigTracker)>`, tracked locals become trusted through `TryFrom<(ConfigFile<Tracked>, &ConfigTrust)>`, trusted locals parse through `TryFrom<ConfigFile<Trusted>>`, and discovered globals parse through `TryFrom<ConfigFile<Discovered>>`.
- `ConfigBuilder<ConfigBuilderInput>` now uses the validated builder input itself as the initial builder state, then transitions to `LocalStored -> Merged`.
- `DiscoveryContext::new(kind, anchor)` validates kind/anchor combinations; full discovery rejects file anchors before `DiscoveryEngine::process()`.
- `DiscoveryOutcome` now stores `kind`, `anchor`, `local`, and `global`; the redundant `cwd` field/accessor was removed.
- `ConfigBuilderInput::try_from(DiscoveryOutcome)` now contains the full-load nearest-local precedence policy inline before `ConfigBuilder::new`; there is no separate `select_nearest_local` helper.
- `ConfigBuilderInput` keeps the selected local field named `local`; precedence is encoded by `TryFrom<DiscoveryOutcome>` accepting only full discovery, requiring local candidates, choosing the discovered local config with the deepest root containing the discovery anchor, and storing optional `global` separately so merge can apply global before local.
- The generic `ConfigFile<State>::parse()` and lifecycle `transition()` helper were removed; state-specific `From`/`TryFrom` conversions share only a private `read_raw_config(path)` helper.
- `ConfigTrust::is_trusted` and `ConfigService::is_trusted` now take `TrustTarget` as their sole target input, eliminating root/config-file positional parameter ambiguity.
- `ResolvedTrustTarget` was removed; CLI trust target resolution now returns discovered local `ConfigFile` values directly.
- `DiscoveryAnchor::path()` replaced the standalone `anchor_path()` helper, and discovery helper routines live inside the `DiscoveryEngine` impl.
- `ConfigService::load(cwd)` performs full discovery, parses `ConfigBuilderInput`, stores/trust-checks the selected local config, merges optional global config first, and builds final `Config`.
- `ConfigTrust` still supports `TrustTarget::Directory` for bare-root trust-store behavior, but production path resolution now reaches trust through discovered local config files.
- A `cfg_attr(not(test), expect(dead_code))` documents that config loading is implemented before a render command consumes it; clippy is otherwise clean.
- `ConfigFile<State>` now stores only the concrete `state: State`; the earlier `state` plus `_state: PhantomData<State>` duplicated type information and was removed.
- `ConfigFileError` is now the primary error type for `ConfigFile<State>` lifecycle transitions. It embeds `ConfigFileParseError` and `ConfigFileTrustError`; `ConfigFileParseError` no longer embeds `ConfigFileError`, and `ConfigBuilderError` now wraps config-file failures through a single transparent `ConfigFile` variant.
- Verification passed:
  - `cargo test config::`: 87 passed.
  - `MISE_EXPERIMENTAL=1 mise run fmt`: passed.
  - `MISE_EXPERIMENTAL=1 mise run test`: 157 passed.
  - `MISE_EXPERIMENTAL=1 mise run clippy`: passed.
  - `MISE_EXPERIMENTAL=1 mise run ci`: passed with non-failing existing `cargo deny` duplicate/unmatched-license warnings.
  - GitNexus `detect_changes(scope=all, worktree=.worktrees/config-typestate)`: low risk, 9 changed files, no indexed symbol/process impact.


## Issues Encountered
| Issue | Resolution |
|-------|------------|
| Prior proposal may have overused scope typestate | Reframe typestate around lifecycle stages, not local/global source. |
| `TrustTargetError` duplicated discovery-style path errors | Consider shared `ConfigFileError` plus locator/discovery-level absence error. |

## Resources
- `src/config/candidate.rs`
- `src/config/discovery.rs`
- `src/config/builder.rs`
- `/Users/jack/Documents/41_personal/traces-pkm-init-cli/src/config/trust.rs`
- `.agents/skills/rust-skills/rules/api-typestate.md`
- `.agents/skills/rust-skills/rules/type-enum-states.md`
- `skill://codebase-design`
- `skill://m05-type-driven`
- `skill://grilling`
- GitHub source target: https://github.com/jdx/mise/blob/main/src/config/mod.rs

## Visual/Browser Findings
- None.
