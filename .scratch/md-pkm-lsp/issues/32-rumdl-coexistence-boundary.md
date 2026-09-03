# rumdl coexistence: explicit capability-ownership boundary

Type: grilling
Blocked by: 03, 25, 27

## Question

Synthesize the final coexistence contract once diagnostics (25) and structural/document-link capabilities (27) are settled, grounded in the rumdl research (ticket 03) — which already confirms rumdl ships `enableLinkCompletions`/`enableLinkNavigation` settings specifically intended to cede link intelligence to "another language server (for example a PKM/notes LSP)".

Decide and document as the shipped product contract:

- The complete ownership table: linting/formatting/fence-language-completion → rumdl; PKM link/wikilink/tag/schema/query/template intelligence → Traces; generic structural intelligence (folding, symbols, selection ranges per ticket 27) → whichever server's research (tickets 03, 05) shows is the stronger/more complete implementation, or split by sub-capability if warranted.
- The concrete recommended client configuration: both servers registered for the `markdown` language, with rumdl's `enableLinkCompletions`/`enableLinkNavigation` set to `false` documented as a required companion setting for a clean joint setup — decide whether Traces should detect rumdl's presence and warn/guide the user if these aren't disabled (possible, but check what's actually detectable from inside an LSP server — likely nothing, since servers don't see each other; more likely this becomes user-facing setup documentation rather than runtime detection).
- Diagnostic-code namespacing confirmed non-colliding (cross-check ticket 25's decision against rumdl's `MDxxx`/`inline-config` codes).
- Formatting ownership: confirm Traces registers zero `documentFormattingProvider`/`documentRangeFormattingProvider` capability at all, ceding 100% of formatting to rumdl, per the product goal's clean split.
