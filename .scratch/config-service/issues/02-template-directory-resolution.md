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

Refactor `src/config.rs` into a `src/config/` module with 7 files using typestate wrappers for discovery and building, and `figment` for config merging.

### Module structure

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

### raw.rs

One `RawConfig` for both local and global. No separate `RawGlobalConfig` — `output_dir` in global config is silently accepted (known `Option` field defaulting to `None`) and ignored during merge.

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

### candidate.rs

```rust
pub(super) struct CandidateConfigFile {
    root: PathBuf,
    source: ConfigSource,  // carries path
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum ConfigSource {
    Local(PathBuf),
    Global(PathBuf),
    Default,
}
```

### discovery.rs — Typestate DiscoveryProcessor

States: `Init`, `LocalCollected`, `GlobalCollected`. Transitions consume `self`.

```rust
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
    fn collect_global(self, global_config_path: Option<&Path>)
        -> Result<DiscoveryProcessor<GlobalCollected>, DiscoveryError>;
}

impl DiscoveryProcessor<GlobalCollected> {
    fn finish(self) -> Box<[CandidateConfigFile]>;
}
```

Candidates accumulate in priority order (global first, then local) for figment's "last wins" merge. Missing file ≠ error — just adds no candidate. I/O errors (permission, etc.) are `DiscoveryError`.

### builder.rs — Typestate ConfigBuilder

States: `Discovered`, `Tracked`, `Parsed`, `Merged`.

Transitions:

```
Discovered ──track()──→ Tracked ──parse()──→ Parsed ──merge()──→ Merged ──build()──→ Config
```

- **`Discovered → Tracked`:** No-op (reserved for trust pipeline).
- **`Tracked → Parsed`:** Each candidate read into its own figment. `extract_inner("templates.directory")` per file extracts provenance-sensitive values → `local_dir`/`global_dir`.
- **`Parsed → Merged`:** All per-file figments merged via `.merge()`. Combined figment extracts remaining fields.
- **`Merged → Config`:** Assembles final `Config`.

### domain.rs

```rust
pub struct Config {
    root: PathBuf,
    templates: TemplateConfig,
    sources: Box<[ConfigSource]>,
}

pub struct TemplateConfig {
    local_dir: Option<PathBuf>,
    global_dir: Option<PathBuf>,
    default_output_dir: PathBuf,
}
```

`ConfigError` keeps today's `Read { path, source }` and `Parse { path, src, span, source }` variants. `DiscoveryError` covers file-walking errors.

### service.rs

```rust
pub struct ConfigService;  // stateless

impl ConfigService {
    pub fn discover(&self, cwd: &Path) -> Result<Box<[CandidateConfigFile]>, DiscoveryError>;
    pub fn build(&self, candidates: Box<[CandidateConfigFile]>) -> Result<Config, ConfigError>;
    pub fn load(&self, cwd: &Path) -> Result<Config, ConfigError>;
}
```

No stored `global_config_path` — discovery handles it internally.

### Figment roles

1. **Per-file** (`Parsed` state): Each candidate → separate figment → `extract_inner("templates.directory")` for provenance data.
2. **Merged** (`Merged` state): All figments chained via `.merge()` → extract full `RawConfig` for remaining fields.

Replaces manual `toml::from_str` + `merge_raw`. `toml` dependency is replaced by figment's `toml` feature.

### Consequences

- Pipeline stages compiler-enforced — can't parse before discover, can't build before merge.
- Tracked/Trusted placeholders ready for trust validation.
- More code (typestate wrappers, PhantomData, state markers).
- New `figment` dependency (~68 KB).

## Blocked by

- `.scratch/config-service/issues/01-config-discovery-and-merge.md`
