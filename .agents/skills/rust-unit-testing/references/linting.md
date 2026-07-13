# Linting

> `#[allow]` and `#[expect]` in unit suite code are reviewable decisions.

Prefer narrow `#[expect(..., reason = "...")]` when a lint is intentionally triggered. Treat broad `#[allow(...)]`, missing reasons, and module-wide suppressions as suspicious unless justified.

For each suppression report file/line, lint name, scope, reason presence, acceptable/suspicious classification, and smallest fix: keep with reason, narrow scope, rewrite suite code, or delete suppression.
