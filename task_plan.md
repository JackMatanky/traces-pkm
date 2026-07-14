# Task Plan: Full Redesign of `src/config/error.rs`

## Goal
Produce a complete, from-scratch redesign of the config module's error type hierarchy — eliminating miette from library code, splitting by operation boundary, co-locating with owning modules, and using precise variant naming — and present it as a documented analysis (no implementation).

## Current Phase
Phase 2

## Phases

### Phase 1: Exhaustive Error Surface Mapping
- [x] Map every fallible function across all 12 config files to its error production point
- [x] Categorize each error by audience (user/dev/ops) and recovery semantics
- [x] Identify current-design problems (derive hygiene, layering, naming, co-location)
- **Status:** complete

### Phase 2: Full Redesign From First Principles
- [x] Decide: should library types carry miette? → **No**, miette is a CLI concern
- [x] Decide: one mega-enum or per-operation types? → **Per-operation** for precision
- [x] Decide: new variant naming from scratch
- [x] Decide: which types to merge, split, add, or eliminate
- [x] Document the full proposed hierarchy with rationale
- [x] Present final analysis
- **Status:** in_progress

### Phase 3: Delivery
- [ ] Ensure all planning files are up to date
- [ ] Deliver final analysis to user
- **Status:** pending

## Key Questions
1. Should errors distinguish domain outcomes from infrastructure failures at the type level? → Yes, `RootNotTrusted`/`StaleConfigContent` (outcomes) vs `TrustCheckFailed`/`ConfigParseFailed` (actual failures)
2. Should the CLI layer have its own error type? → Yes, when the CLI exists, it wraps library errors with miette
3. Should `ConfigBuilderError` remain as a separate re-exported type? → No, absorb into `BuildError` as a direct variant
4. Should infra types (`StoreError`, `TrustError`, `HashError`) be pub or pub(super)? → `pub` (required by Rust field-visibility rule) but NOT re-exported from mod.rs
5. Should `DiscoveryError` and `ResolutionError` remain as separate types? → Yes, they are returned by separate operations (`discover()` and `resolve_template()` respectively)

## Decisions Made
| Decision | Rationale |
|----------|-----------|
| Strip `miette::Diagnostic` from ALL config error types | Library shouldn't know about CLI presentation; hexagonal architecture principle |
| Replace single `ConfigError` megatype with `DiscoveryError`, `ConfigBuilderError`, `ResolutionError` | Each operation produces a disjoint error set; precision without wildcards |
| Co-locate error definitions with owning modules | Developer edits `store.rs` without opening `error.rs` |
| Keep `ConfigBuilderError` as the build-pipeline error type | All errors originate from `ConfigBuilder` methods — the type matches the owning struct |
| Admin ops (`trust`, `list_tracked`) return `TrustError`/`StoreError` directly | These operations produce exactly one category of failure |
| Rename variants for clarity | `Untrusted` → `RootNotTrusted`, `Stale` → `StaleConfigContent`, `Access` → `PathInaccessible`, `Load` → `ConfigParseFailed`, `TrustIo` → `TrustCheckFailed`, `NoLocalConfig` → `LocalConfigAbsent`, `Tracking` → eliminated |
| Keep `TrustError`, `StoreError`, `HashError` as `pub` but NOT re-exported | Rust requires pub for struct fields; consumers shouldn't match directly |
| Zero `#[from]` on public error types | Explicit `map_err` is grep-able and avoids coupling between error types |
| `DiscoveryError` and `ResolutionError` remain separate | Returned by distinct operations, not part of the build pipeline |

## Errors Encountered
| Error | Attempt | Resolution |
|-------|---------|------------|
| Initial design was too shallow — kept miette, kept old names, didn't re-think hierarchy | 1 | Started over from first principles (this plan) |
| Initial design kept `ConfigError` as a megatype | 1 | Split into per-operation types |

## Notes
- This is an analysis-only task — NO implementation
- The CLI layer does not exist yet (main.rs is a stub); miette decisions affect future work
- All error variants need Display messages that make sense without miette help text
- Template files: `~/.config/opencode/skills/planning-with-files/templates/`
