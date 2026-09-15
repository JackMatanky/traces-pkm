# Commands

> Run the smallest command that verifies the branch you touched.

| Need | Command |
|---|---|
| Unit and integration suites with nextest | `cargo nextest run` |
| Doctests | `cargo test --doc` |
| Unit-only target | `cargo nextest run --lib` |
| Integration target | `cargo nextest run --test '<name>'` |
| All integration targets | `cargo nextest run --tests` |
| Clippy including suite code | `cargo clippy --all-targets --all-features -- -D warnings` |
| Criterion benchmarks | `cargo bench` |

`cargo nextest run` does not run doctests; use `cargo test --doc` whenever doctests are relevant.

Prefer existing project tasks over raw cargo commands when they cover the same check.

If a command is not run, state the reason in the final response or review output.
