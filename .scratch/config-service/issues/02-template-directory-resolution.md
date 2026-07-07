# Template directory resolution (exact -> local -> global)

Status: ready-for-agent

## Parent

`.scratch/config-service/PRD.md`

## What to build

Add template resolution to `ConfigService`: given a template identifier, resolve in priority order — exact filesystem path → path within the local template directory → path within the global template directory. First match wins. When multiple files match at the same priority level, error with the candidate paths listed so the user can disambiguate. Returns the resolved file path (and the directory it came from, for later trust checking).

## Acceptance criteria

- [ ] Resolution follows exact → local dir → global dir, first match wins
- [ ] Returns the resolved path plus its source directory
- [ ] Multiple matches at the same priority level error with candidates listed (miette)
- [ ] Not-found produces a clear error
- [ ] Tests cover each priority level, override behavior, ambiguous match, and not-found — using temp dirs

## Rust guidance

Relevant skills: `m06-error-handling`, `m05-type-driven`, `m13-domain-error`.

- **Return type (m05):** don't return a bare `PathBuf`. Return a small struct/tuple carrying both the resolved path **and** its source template directory, so issue tmpl-01 can trust-check the origin without re-deriving it. Consider a `ResolvedTemplate { path, source_dir }` type.
- **Three-outcome result (m06):** resolution has three outcomes — found-one, found-many (ambiguous), not-found. Model found-many and not-found as distinct `thiserror` variants (`AmbiguousTemplate { candidates: Vec<PathBuf> }`, `TemplateNotFound { name }`), not a generic string error, so callers and miette can render each differently.
- **Ambiguity is per-priority-level (m13):** two matches at the *same* level is the error; a local match shadowing a global one is normal resolution, not ambiguity. Enforce first-match-wins across levels, ambiguity check within a level.
- **miette:** the `AmbiguousTemplate` error should list candidate paths as help text so the user can disambiguate; the `TemplateNotFound` error should name the directories searched.

## Blocked by

- `.scratch/config-service/issues/01-config-discovery-and-merge.md`
