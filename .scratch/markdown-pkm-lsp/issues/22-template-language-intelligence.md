# Template-language intelligence

Type: grilling
Blocked by: 08

## Question

Grounding — this is the sharpest constraint in the whole map: **templates cannot be safely executed for analysis purposes.** `TemplateEngine` owns one shared, `debug`-enabled minijinja `Environment` (`src/template/engine.rs:71,117`); helpers are registered as `Object`-trait globals (e.g. `ui` at `src/template/engine/ui.rs:61-63`); the `ui.*` namespace's "No-Declaration Format" means templates call interactive prompts *synchronously mid-render*, blocking on a live `DialogProvider` (`src/template/engine/ui.rs:82-87`) — attempting to render a template with no human present either hangs or immediately errors (`DialogError::NotInteractive`). **There is no existing static-analysis capability**: no code path extracts "which helpers/variables does this template reference" without a full execution pass, and there is no root-template AST caching today (every render re-parses via `env.template_from_named_str`, `src/template/engine.rs:172-175`). Minijinja's own parser (`env.parse()`) is available but unused for this purpose today — confirmed via rust-docs-mcp against the `minijinja` crate if deeper API confirmation is needed.

Decide:
- The static-analysis approach: build a new (LSP-only, or shared-and-reusable) code path using minijinja's `Environment::parse`/AST inspection to answer "what helper calls, variables, and includes does this template reference, and where (spans)" without executing `ui.*`/`query.*`/etc. side effects at all.
- Completion: helper-namespace and function-name completion (`ui.`, `file.`, `date.`, `query.`, `tasks.`, `schema.` → member completion) — does this require typed knowledge of each namespace's method signatures (hand-maintained metadata table, since minijinja `Object`s don't expose a reflectable schema) or can it be derived some other way?
- Hover: showing a helper function's signature/doc on hover — same "hand-maintained metadata" question.
- Diagnostics: minijinja's own parse errors already preserve spans (`env.set_debug(true)`, `TemplateError::Render` preserves the source `minijinja::Error`, `src/template/error.rs:89-96`) — decide whether these get surfaced live (on every keystroke, via the static-parse-only path) as LSP diagnostics, distinct from the CLI's render-time error diagnostics.
- Whether `{% include %}`-referenced templates get "go to definition"/completion for template names, reusing `TemplateLoader::find`'s existing local→global resolution precedence (`src/template/loader.rs:85-99`).
