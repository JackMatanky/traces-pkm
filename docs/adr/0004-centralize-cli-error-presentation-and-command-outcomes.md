---
number: 4
title: Centralize CLI error presentation and command outcomes
status: proposed
date: 2026-07-27
tags:
  - error-handling
  - cli
  - templates
  - domain-model
---

# Centralize CLI error presentation and command outcomes

## Context

Traces preserves most source chains with typed errors, but configuration failures are translated separately by Template and completions command modules, interaction cancellation is misidentified by the Template CLI diagnostic, and error meaning can be lost while crossing minijinja or filesystem adapters. The CLI needs stable diagnostic identifiers for support and automation without coupling domain modules to terminal presentation.

Interactive Commands need one coherent meaning for a User Abort. This decision applies to Template Browser selection, Interactive Functions during Template instantiation, Template Output Path collision prompts, and `traces init`.

## Decision Drivers

- Preserve typed domain failure meaning and source chains.
- Give each user-correctable failure one stable diagnostic identity.
- Treat User Abort as control flow, not failure.
- Keep all interactive Commands transactional from the User's perspective.
- Remove duplicate and type-erased error translations.

## Considered Options

- Let each command module own its diagnostic types and exit policy.
- Put miette diagnostics directly on domain errors.
- Use one CLI presentation module over typed domain errors and Command Outcome.

## Decision

Use one CLI presentation module. Domain modules retain typed errors and source chains; the CLI seam alone owns miette codes, help text, stderr presentation, and process exit behavior. Configuration-load failures have one stable diagnostic identity regardless of which Command triggered them.

The CLI presentation module has one external interface. Its private implementation may split into diagnostic-family submodules only when that improves locality; such a split must preserve the facade and must not recreate command-specific error modules or presentation seams.

Introduce Command Outcome as a first-class non-failure result. Completed and User Abort are distinct outcomes. Across every interactive Command, Escape aborts the whole Command with no side effect, no diagnostic, and exit 0; Ctrl-C aborts the whole Command with terminal-conventional exit 130. Other recoverable failures retain the generic non-zero failure outcome.

Template Resolution distinguishes genuine absence, unsafe identifiers, ambiguity, and inaccessible Template Directories. Unsafe identifiers are explicit validation failures. Ambiguity carries every matching Template Directory-relative candidate. Custom Function failures retain minijinja location and detail in their source chain while the Template module classifies their domain origin before the CLI seam.

Perform a clean cutover: remove type-erased and single-variant internal error modules when they add no recovery meaning, retain concrete sources until the CLI seam, and migrate every caller and test.

## Consequences

Good, because:

- Diagnostic meaning, help, and exit behavior have one owner.
- Command modules stop duplicating configuration translation.
- User Abort is never presented as a failure.
- Template authors receive actionable resolution and Custom Function diagnostics.
- Tests cross the same interfaces as callers.

Bad, because:

- The refactor spans command dispatch, dialog orchestration, Template Resolution, Template rendering, and error tests.
- Existing command-specific diagnostic codes are replaced by shared stable identities.
- Every interactive Command must prove no side effect follows User Abort.

No new numeric exit-code taxonomy is introduced beyond 0 for completion or User Abort, 130 for Ctrl-C, and the existing generic non-zero failure outcome.

## Confirmation

The implementation will pass the project test task and add contract tests for each stable diagnostic code, each User Abort origin, Ctrl-C exit 130, and the absence of a Note or init scaffold after User Abort. Review confirms domain modules do not implement miette diagnostics and that command-specific presentation modules no longer exist.

## More Information

Complements ADR-0001, which establishes lazy Interactive Functions, and ADR-0003, which establishes index-returning dialog selection.
