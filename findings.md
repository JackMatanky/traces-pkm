# Findings & Decisions

## Requirements
- User wants design grilling only; do not implement changes.
- Track this session in persistent planning files: `task_plan.md`, `findings.md`, and `progress.md`.
- Explore two separate typestate patterns:
  - `ConfigFile<State>`: lifecycle/invariants for a single config file.
  - `ConfigBuilder<State>`: aggregate builder that consumes all discovered config files and produces final domain `Config`.
- Preserve consideration of `ConfigSource` as useful domain enum and path-validation discriminator.
- Consider `ConfigLocator` as a separately testable module that owns ascending walk, descending walk, and nearest-local-config behavior.

## Research Findings
- `ConfigSource` currently distinguishes `Local(PathBuf)` and `Global(PathBuf)` in `src/config/candidate.rs`.
- `CandidateConfigFile` currently stores `root: PathBuf` plus `source: ConfigSource`; the raw constructor can create inconsistent root/source combinations.
- `DiscoveryProcessor<Init>::collect_local()` currently performs an ancestor walk looking for `.traces/config.toml`.
- The trust-target resolver in the implemented worktree also performs ancestor and descendant config lookup; this duplicates discovery/locator responsibilities.
- Rust typestate guidance applies when invalid transitions should become compile errors; enum-state guidance applies when a value is one of mutually exclusive runtime states.
- New research question: is `ConfigBuilder<State>` a deep module or an unnecessary component once `ConfigFile<State>` and `ConfigLocator` own more behavior?
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
| Global configs should not transition through tracked/trust states | User challenged the unified lifecycle: global configs can parse directly from `Located` after verifying `ConfigSource::Global`; only local configs need tracking/trust. |
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
| Use discovery kinds `Full`, `NearestLocal`, and `AllLocalDescendents` | User chose these names for the discovery context kind enum. |
| Keep absence in `DiscoveryError`, not `ConfigFileError` | User agreed missing search results are discovery operation outcomes, not errors about a specific config-file path. |
| Have `DiscoveryError` wrap `ConfigFileError` | User agreed discovery should not duplicate file/path validation variants; specific config-file construction failures bubble through discovery. |
| Rethink trust routes instead of adding trust-target resolution errors | User observed nested discovery/config-file errors inside a trust-target-resolution error suggests the component boundary is wrong; discovery components should probably make a separate resolution layer unnecessary. |
| Route trust through discovery and config-file lifecycles, not trust resolution | User agreed the design is better when trust uses `ConfigFile` and discovery components directly, with no trust-specific resolution layer. |
| Keep nearest-local absence in discovery but consider optional search APIs | User agreed absence belongs to discovery, but noted `nearest_local` should not always error because init may use absence to create a new local config. |
| Let `ConfigService::load(cwd)` call discovery processing directly | User noted load can simply call a `process()` method that runs `DiscoveryProcessor`; this keeps full discovery hidden behind discovery components rather than decomposing load into nearest-local calls. |
| Use `DiscoveryContext` as discovery method input | User challenged passing raw anchors to focused discovery methods; context should be the input shape for each discovery operation. |
| Use unified `DiscoveryOutcome` for all discovery kinds | User observed `DiscoveryOutcome` can represent `Full`, `NearestLocal`, and `AllLocalDescendents` by varying local/global cardinality, avoiding a separate `DiscoveryOutput` enum. |
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
- `DiscoveryContext` has private fields and smart constructors. It uses `DiscoveryType::{Full, NearestLocal, AllLocalDescendents}` and `DiscoveryAnchor::{Directory, File}`.
- `DiscoveryOutcome` stores `kind`, `anchor`, `local: Box<[ConfigFile<Discovered>]>`, and `global: Box<[ConfigFile<Discovered>]>`.
- Full config loading selects nearest local plus optional global; it does not merge every discovered local config.
- `ConfigBuilderInput` is parsed from `DiscoveryOutcome` with `TryFrom`, codifying selection/precedence before reaching the builder.
- `ConfigBuilder::new` accepts only `ConfigBuilderInput`.
- `ConfigService::load(cwd)` is the normal load entry point; `discover()` and `build()` become private implementation details.
- Trust routes use discovery plus config-file lifecycle directly; no separate trust-target-resolution component.

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
1. Introduce `ConfigFile<Discovered>` and remove `CandidateConfigFile`.
2. Add `DiscoveryContext`, `DiscoveryType`, and `DiscoveryAnchor`.
3. Add `DiscoveryEngine::process(ctx)` returning `DiscoveryOutcome`.
4. Change `DiscoveryOutcome` to store `ConfigFile<Discovered>`.
5. Add `ConfigBuilderInput` and `TryFrom<DiscoveryOutcome>`.
6. Change `ConfigBuilder::new` to accept `ConfigBuilderInput` only.
7. Add `ConfigService::load(cwd)` and make `discover()`/`build()` private.
8. Rework trust routes to use `DiscoveryEngine` and `ConfigFile<Tracked>`.
9. Update errors and CLI wrappers.
10. Update tests.

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
