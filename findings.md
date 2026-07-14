# Findings & Decisions — Config Error Redesign

## Requirements
- Produce a complete from-scratch redesign of `src/config/error.rs`
- Review ALL files in `src/config/` to map every error production point
- Research Rust error design best practices (subagents)
- Do NOT implement anything — analysis only
- Previous attempt was rejected for being too shallow (kept miette, kept old names, kept megatype)

## Research Findings

### From Rust error design guides (subagent research)
- **Library vs Application errors**: Libraries use `thiserror` for typed errors; applications use `anyhow`. Never return `anyhow::Error` from library APIs.
- **Per-function error types**: Define an enum next to the function that returns it. Avoids global 54-variant enums.
- **miette**: For CLI tools that need source-code snippets, labeled spans, error codes, and help text. A presentation concern.
- **Hexagonal architecture**: Domain errors are DATA, not presentation. Adapters (CLI, HTTP) map domain errors to user-facing messages.
- **Error categorization**: Domain outcomes (expected business states) ≠ infrastructure failures (I/O broke) ≠ application errors (bad input). Each has different recovery semantics.
- **`#[from]` anti-pattern in public types**: Creates hidden coupling. Explicit `map_err` is grep-able and safer.
- **`#[non_exhaustive]`**: For library public enums to allow backward-compatible variant additions. Not needed for application-internal types.

### From guide URLs (subagent research)
- The Definitive Guide to Rust Error Handling: thiserror for libraries, anyhow for apps, miette for CLIs. Use context at boundaries.
- Ultimate Guide to Rust Newtypes: Parse-don't-validate principle — convert at boundaries.
- Master Hexagonal Architecture in Rust: Domain layer defines what errors MEAN. Adapters define what users SEE.

### From codebase analysis
- `config/mod.rs` re-exports 4 error types: `ConfigBuilderError`, `ConfigError`, `DiscoveryError`, `ResolutionError`
- `StoreError` and `TrustError` are `pub` (needed for field visibility) but NOT re-exported
- `HashError` is `pub(crate)` in `hash.rs`
- No external consumers of config errors exist yet (main.rs is a stub)
- `dialog/error.rs` uses `thiserror` only (no miette) — good precedent

### Current design problems identified
1. `miette::Diagnostic` on types that are never user-facing (`StoreError`)
2. CLI concerns (`run `traces trust``) embedded in library types
3. One `ConfigError` megatype mixing 3 categories of error across 6 operations
4. All error definitions in one file (`error.rs`) rather than co-located
5. Imprecise return types on admin operations (return `ConfigError` when only 1 variant possible)
6. `#[from]` creating hidden coupling
7. Poor variant naming (`Untrusted`, `Stale`, `Access`, `Load`, `Tracking`)
8. `DiscoveryError::NoLocalConfig` lacks actionable help text (but this is a CLI concern)

## Technical Decisions
| Decision | Rationale |
|----------|-----------|
| Strip miette from all config errors | Library shouldn't know about CLI presentation |
| Split ConfigError into DiscoveryError, ConfigBuilderError, ResolutionError | Each operation returns exactly what it can produce |
| Co-locate error defs with owning modules | Developer edits store.rs without opening error.rs |
| Name the build-pipeline error `ConfigBuilderError` | All errors originate from `ConfigBuilder` methods — the type should match the owning struct |
| Admin ops return TrustError/StoreError directly | Preci se types for precise operations |
| Zero #[from] on public types | Explicit conversions are grep-able and don't couple types |
| New variant names | Clear, self-documenting, domain-oriented |
| Keep infra types pub but not re-exported | Rust field-visibility rule requires pub; consumers shouldn't match directly |

## Issues Encountered
| Issue | Resolution |
|-------|------------|
| First redesign was too shallow | Started over with first-principles: strip miette, per-operation types, new names |

## Complete Proposed Hierarchy

### Layer 1 — Infrastructure Types (thiserror only, no miette)

```
hash.rs:
  pub enum HashError {
      Read { path: PathBuf, source: io::Error },
  }

store.rs:
  pub(super) enum StoreError {
      Canonicalize { path: PathBuf, source: io::Error },
      StoreIo { path: PathBuf, source: io::Error },
  }

trust.rs:
  pub enum TrustError {
      Store(#[from] StoreError),
      Hash(#[from] HashError),
      CompanionWrite { path: PathBuf, source: io::Error },
  }
```

All three are `pub` (required by Rust field-visibility for error wrapping) but NOT re-exported from `mod.rs`. Consumers see them through `.source()` chains only.

### Layer 2 — Operation-Specific Errors (thiserror only, no miette)

Co-located with the modules that produce them:

```
discovery.rs:
  pub enum DiscoveryError {
      LocalConfigAbsent { cwd: PathBuf },
      PathInaccessible { path: PathBuf, source: io::Error },
  }

builder.rs:
  pub enum ConfigBuilderError {
      RootNotTrusted { root: PathBuf },
      StaleConfigContent { root: PathBuf },
      TrustCheckFailed { root: PathBuf, source: TrustError },
      ConfigParseFailed { path: PathBuf, source: Box<figment::Error> },
  }

domain.rs:
  pub enum ResolutionError {
      AmbiguousTemplate { name: PathBuf, candidates: Vec<PathBuf> },
      TemplateNotFound { name: PathBuf, directories: Vec<PathBuf> },
  }
```

All three are re-exported from `mod.rs`.

### Layer 3 — Service API Return Types

```
service.rs:
  fn discover(&self, cwd: &Path)            -> Result<DiscoveryOutcome, DiscoveryError>;
  fn build(&self, outcome)                  -> Result<Config, ConfigBuilderError>;
  fn trust(&self, root, config)             -> Result<(), TrustError>;
  fn is_trusted(&self, root, config)        -> Result<TrustState, TrustError>;
  fn list_tracked(&self)                    -> Result<Vec<PathBuf>, StoreError>;
  fn clean_tracked_store(&self)             -> Result<usize, StoreError>;
```

Every function returns exactly what it can produce. No wildcard arms needed.

### Layer 4 — Future CLI Layer (outside this module)

```
cli/ or bin/:
  enum CliError {
      Discover(DiscoveryError),   → adds miette::Diagnostic with help text
      Build(ConfigBuilderError), → adds error codes, help text
      Resolve(ResolutionError),   → adds help text for templates
      Store(StoreError),          → adds user-facing messages for I/O errors
      Trust(TrustError),          → adds "run traces trust" guidance
      ...
  }
```

The CLI adds `miette::Diagnostic` at this boundary. The library remains CLI-agnostic.


## Resources
- How to Code It — Definitive Guide to Rust Error Handling: https://www.howtocodeit.com/guides/the-definitive-guide-to-rust-error-handling
- Ultimate Guide to Rust Newtypes: https://www.howtocodeit.com/guides/ultimate-guide-rust-newtypes
- Master Hexagonal Architecture in Rust: https://www.howtocodeit.com/guides/master-hexagonal-architecture-in-rust
- Rust error handling skill: m06-error-handling
- Domain error strategy skill: m13-domain-error
- Rust best practices skill: rust-skills (err- rules)
- Planning templates: ~/.config/opencode/skills/planning-with-files/templates/
