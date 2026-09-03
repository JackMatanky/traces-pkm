# Config integration: .traces/config.toml vs LSP initializationOptions/workspace configuration

Type: grilling
Blocked by: 09

## Question

Decide how the existing two-tier `Config File` system (`.traces/config.toml` local, `~/.config/traces/config.toml` global — `src/config/`) integrates with LSP-native configuration surfaces, once the framework choice (ticket 09) is settled:

- Does the LSP read `.traces/config.toml` directly via the existing `ConfigService`/discovery pipeline (treating LSP-mode as just another `Traces` entrypoint that reuses config loading verbatim, per the "reuse existing services" constraint), with LSP `initializationOptions`/`workspace/configuration` limited to purely editor-integration concerns (e.g. `enableLinkNavigation`-style per-feature toggles, mirroring rumdl's own settings surface per ticket 03) that have no `.traces/config.toml` equivalent?
- `workspace/didChangeConfiguration` handling: does a client-side settings change (LSP-only toggles) get applied live without a restart, separately from `.traces/config.toml` changes (which go through the existing Config File Store's out-of-band-change detection, `Companion Hash`)?
- Does editing `.traces/config.toml` itself while the LSP is running need special handling (it's a TOML file the LSP could also treat as "just another open document" — decide if any language intelligence for the config file itself is in scope, or explicitly excluded)?
