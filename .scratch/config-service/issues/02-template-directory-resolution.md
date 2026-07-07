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

## Blocked by

- `.scratch/config-service/issues/01-config-discovery-and-merge.md`
