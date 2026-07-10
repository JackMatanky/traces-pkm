# Template directory resolution (exact -> local -> global)

Status: implemented

## Parent

`.scratch/config-service/PRD.md`

## What to build

Add template resolution to `ConfigService`: given a template identifier, resolve in priority order — exact filesystem path → path within the local template directory → path within the global template directory. First match wins. When multiple files match at the same priority level, error with the candidate paths listed so the user can disambiguate. Returns the resolved file path (and the directory it came from, for later trust checking).

## Acceptance criteria

- [x] Resolution follows exact → local dir → global dir, first match wins
- [x] Returns the resolved path plus its source directory
- [x] Multiple matches at the same priority level error with candidates listed (miette)
- [x] Not-found produces a clear error
- [x] Tests cover each priority level, override behavior, ambiguous match, and not-found — using temp dirs

## Rust guidance

Relevant skills: `m06-error-handling`, `m05-type-driven`, `m13-domain-error`.

- **Return type (m05):** don't return a bare `PathBuf`. Return a small struct/tuple carrying both the resolved path **and** its source template directory, so issue tmpl-01 can trust-check the origin without re-deriving it. Consider a `ResolvedTemplate { path, source_dir }` type.
- **Three-outcome result (m06):** resolution has three outcomes — found-one, found-many (ambiguous), not-found. Model found-many and not-found as distinct `thiserror` variants (`AmbiguousTemplate { candidates: Vec<PathBuf> }`, `TemplateNotFound { name }`), not a generic string error, so callers and miette can render each differently.
- **Ambiguity is per-priority-level (m13):** two matches at the *same* level is the error; a local match shadowing a global one is normal resolution, not ambiguity. Enforce first-match-wins across levels, ambiguity check within a level.
- **miette:** the `AmbiguousTemplate` error should list candidate paths as help text so the user can disambiguate; the `TemplateNotFound` error should name the directories searched.

## Implementation notes

- **Resolution lives on `Config`, not `ConfigService`.** The method `Config::resolve_template(name)` owns the three-level iteration (exact path → local dir → global dir). It uses `self.root` (set to the project root, or cwd fallback) to resolve relative paths for the exact-path check. This avoids threading a `cwd` parameter through every call.
- **TODO on `resolve_template`:** marked for eventual move to `TemplateService` — the method is a documentation-level stub that should migrate when TemplateService exists, taking `config.root()` and `config.templates()` as inputs.
- **`TemplateConfig` grouping:** `local_dir`, `global_dir`, `default_output_dir` are grouped in a `TemplateConfig` struct embedded in `Config`, rather than as flat fields.
- **`root: PathBuf` added** to `Config` — set from the project config layer's root directory, falling back to `cwd`.
- **File matching:** uses stdlib `fs::read_dir` + `file_stem` comparison (no glob dependency). Ambiguity detected within a single directory when multiple files share the same stem. Skips directories, only matches regular files.
- **Renames during development:** `DiscoveredConfig` → `DiscoveredConfigFile`, `TemplatesConfig` → `RawTemplateConfig`, `ConfigSource::Project` → `ConfigSource::Local`.
- **Public API changes:** `Config::template_directory()` → `Config::local_template_dir()`, `Config::global_template_directory()` → `Config::global_template_dir()`.

## Code review fixes (2026-07-08)

- **`matching_files_in_dir`:** now filters by `file_type().is_file()` to prevent directories with matching stems from being included as candidates.
- **`RawGlobalConfig`:** added separate struct without `output_dir` + `#[serde(deny_unknown_fields)]` — global config files now reject `output_dir` at parse time.
- **`read_config` + `parse_config` merged** into single generic `read_and_parse<T>` where `T: DeserializeOwned + Default`.
- **`#[must_use]` removed** from `Result`-returning functions (`load`, `load_from`, `load_from_paths`, `resolve_template`) — `Result` already carries `#[must_use]` at the type level, avoiding `clippy::double_must_use`. Kept on `Config::resolve()` which returns `Config` (not `Result`).
- **`resolve_exact_path`:** parameter renamed `cwd` → `root` to match the value passed in (`config.root()`).
- **`ResolutionError` help fields:** changed from `Vec<PathBuf>` to `String`, formatted as newline-separated paths for human-readable miette output.
- **Test lints:** added `#![allow(clippy::panic_in_result_fn, unreachable, unwrap_used)]` to test module (standard test convention, denied in production).
- **Test patterns:** changed `match { ... _ => unreachable!() }` to `let ... else` paired with `return Err(...)`.
- **`#[inline]` kept** on trivial constructors/getters. Removed from non-hot config-loading functions (`load`, `load_from`, `load_from_paths`, `resolve_template`) — these run at most once per session. Generates `clippy::missing_inline_in_public_items` warnings which are acceptable per the user's preference.

## Config module refactor design (2026-07-08)

### Status

Accepted (implemented)

### Context

`src/config.rs` grew to 571 lines (400 code + 170 tests) with mixed responsibilities — config file discovery, parsing, type definitions, merging, resolution — all in one file. The discovery pipeline read and parsed config files before trust validation, with no structural enforcement of the pipeline order. The `figment` crate provided a standardised config-merge pipeline.

### Decision

Refactor the single `src/config.rs` into a `src/config/` module with 7 files, using typestate-pattern wrappers for discovery and config building, and `figment` for provider-based merging.

```
src/config/
├── mod.rs        — Re-exports (pub use), module-level docs
├── raw.rs        — RawConfig, RawTemplateConfig
├── candidate.rs  — CandidateConfigFile, ConfigSource
├── discovery.rs  — DiscoveryProcessor<State> typestate
├── builder.rs    — ConfigBuilder<State> typestate
├── domain.rs     — Config, TemplateConfig, ConfigError, DiscoveryError
└── service.rs    — ConfigService { discover(), build() }
```

**Visibility:** Only `Config`, `ConfigService`, and `ConfigError` are public from the module root. Everything else is `pub(super)` or private.

### File Details

#### `raw.rs`

One `RawConfig` type shared by both local and global layers. `output_dir` in global config is silently accepted (it is a known `Option` field defaulting to `None`) and ignored during merge — no separate `RawGlobalConfig` type needed.

```rust
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(super) struct RawConfig {
    #[serde(default)]
    templates: RawTemplateConfig,
}

#[derive(Clone, Debug, Default, Deserialize)]
struct RawTemplateConfig {
    directory: Option<PathBuf>,
    #[serde(default)]
    output_dir: Option<PathBuf>,
}
```

#### `candidate.rs`

A candidate represents a discovered config file with its origin metadata. No `path` field — `ConfigSource` already carries it.

```rust
pub(super) struct CandidateConfigFile {
    root: PathBuf,
    source: ConfigSource,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum ConfigSource {
    Local(PathBuf),
    Global(PathBuf),
    Default,
}
```

#### `discovery.rs` — Typestate DiscoveryProcessor

Three states for the discovery pipeline: `Init`, `LocalCollected`, `GlobalCollected`. Each state transition consumes self and returns the next state. File discovery is separated from file reading — candidates are only file paths and source metadata, not parsed content.

```rust
struct Init;
struct LocalCollected;
struct GlobalCollected;

struct DiscoveryProcessor<State> {
    anchor: PathBuf,
    candidates: Vec<CandidateConfigFile>,
    _state: PhantomData<State>,
}

impl DiscoveryProcessor<Init> {
    fn new(anchor: &Path) -> Self;
    fn collect_local(self) -> Result<DiscoveryProcessor<LocalCollected>, DiscoveryError>;
}

impl DiscoveryProcessor<LocalCollected> {
    fn collect_global(
        self,
        global_config_path: Option<&Path>,
    ) -> Result<DiscoveryProcessor<GlobalCollected>, DiscoveryError>;
}

impl DiscoveryProcessor<GlobalCollected> {
    fn finish(self) -> Box<[CandidateConfigFile]>;
}
```

- `collect_local` walks up the directory tree from `anchor` looking for `.traces/config.toml`.
- `collect_global` checks `global_config_path` for `traces/config.toml`.
- Candidates accumulate in priority order (global first, then local) for figment's "last provider wins" merge.
- `collect_*` always succeeds in terms of state transition — a missing file is not an error, it just adds no candidate. Actual I/O errors (permission denied, etc.) are `DiscoveryError`.

#### `builder.rs` — Typestate ConfigBuilder

Five states for the build pipeline. `Tracked` is a no-op pass-through (reserved for future trust validation).

```rust
struct Discovered;
struct Tracked;
struct Trusted;
struct Parsed;
struct Merged;

struct ConfigBuilder<State> {
    candidates: Box<[CandidateConfigFile]>,
    state: State,
}
```

**State transitions:**

```
Discovered ──track()──→ Tracked ──trust()──→ Trusted ──parse()──→ Parsed ──merge()──→ Merged ──build()──→ Config
```

- `Discovered → Tracked → Trusted`: No-op state changes (reserved for trust validation pipeline). Data unchanged.
- `Trusted → Parsed`: Each candidate is read and parsed into its own `Figment`. The provenance-sensitive field `templates.directory` is extracted from each per-file figment via `extract_inner` to populate `local_dir`/`global_dir`.
- `Parsed → Merged`: All per-file figments are merged into one combined figment via `.merge()`. The merged figment extracts the remaining config fields.
- `Merged → Config`: Assembles `Config { root, templates, sources }`.

#### `domain.rs`

```rust
#[derive(Clone, Debug)]
pub struct Config {
    root: PathBuf,
    templates: TemplateConfig,
    sources: Box<[ConfigSource]>,
}

#[derive(Clone, Debug)]
pub struct TemplateConfig {
    local_dir: Option<PathBuf>,
    global_dir: Option<PathBuf>,
    default_output_dir: PathBuf,
}

#[derive(Debug, Diagnostic, Error)]
pub enum ConfigError { ... }

#[derive(Debug, Error)]
pub(super) enum DiscoveryError { ... }
```

`TemplateConfig` keeps both `local_dir` and `global_dir` separately. Resolution (try local first, fall back to global) is the caller's concern.

`ConfigError` carries the same variants as today: `Read { path, source }` and `Parse { path, src, span, source }`.

`DiscoveryError` covers file-walking errors (I/O not related to reading/parsing a specific config file).

#### `service.rs`

No internal state — `ConfigService` is a stateless coordinator. The global config path is discovered internally, not stored.

```rust
pub struct ConfigService;

impl ConfigService {
    pub fn discover(&self, cwd: &Path) -> Result<Box<[CandidateConfigFile]>, DiscoveryError>;
    pub fn build(&self, candidates: Box<[CandidateConfigFile]>) -> Result<Config, ConfigError>;
    pub fn load(&self, cwd: &Path) -> Result<Config, ConfigError>;
}
```

- `discover()` delegates to `DiscoveryProcessor<Init>::new(cwd).collect_local()?.collect_global(...)?.finish()`
- `build()` delegates to `ConfigBuilder::new(candidates).track().trust().parse()?.merge()?.build()`
- `load()` is `self.discover(cwd).and_then(|c| self.build(c))`

### Figment Integration

Figment serves two roles:

1. **Per-file parsing** (in `Parsed` state): Each candidate config file is read into its own `Figment`. `extract_inner("templates.directory")` on each yields the provenance-sensitive value, which populates `local_dir`/`global_dir` on the final `TemplateConfig`.

2. **Merged extraction** (in `Merged` state): All per-file figments are chained via `.merge()` (global first, then local — later wins). The merged figment extracts the full `RawConfig`, providing the remaining fields (especially `templates.output_dir`).

This replaces the manual `toml::from_str` + custom `merge_raw` logic with a standardised provider pipeline. The `toml` crate dependency is replaced by figment's `toml` feature.

### Consequences

- **Positive:** File names as documentation — the module structure tells the story by itself.
- **Positive:** Pipeline stages are compiler-enforced — you cannot parse before discovering, or build before merging.
- **Positive:** Tracked/Trusted states are placeholders ready for trust validation when needed.
- **Positive:** Figment unifies config parsing, error reporting, and merge into one API.
- **Negative:** More total code (typestate wrappers, PhantomData, state marker types).
- **Negative:** New `figment` dependency (~68 KB).
- **Negative:** Existing test suite needs restructuring to match new file layout.

## Implementation plan (9 tasks)

All tasks completed on branch `config-02-template-resolution`, merged to `main` at `790562d`.

- [x] **Task 1:** Add figment dependency + Serialize on RawConfig
- [x] **Task 2:** Create module skeleton and raw.rs + candidate.rs
- [x] **Task 3:** Implement domain.rs (Config, TemplateConfig, ConfigError, DiscoveryError)
- [x] **Task 4:** Implement discovery.rs (DiscoveryProcessor typestate with Init/LocalCollected/GlobalCollected)
- [x] **Task 5:** Implement builder.rs (ConfigBuilder typestate with Disocvered/Tracked/Trusted/Parsed/Merged, figment merge)
- [x] **Task 6:** Implement service.rs (ConfigService entry point)
- [x] **Task 7:** Move resolve_template from old config.rs, wire up lib.rs
- [x] **Task 8:** Finalize tests — move resolution tests, run full suite
- [x] **Task 9:** Remove old config.rs, verify full CI (check + test + clippy)

Full implementation details (per-task code blocks, intermediate steps, test code): see `docs/superpowers/plans/2026-07-08-config-module-refactor.md`.

## Blocked by

- `.scratch/config-service/issues/01-config-discovery-and-merge.md`
