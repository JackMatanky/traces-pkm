# Change Risk Anti-Patterns (CRAP) Metric Setup

The **CRAP** (Change Risk Anti-Patterns) metric is a software quality metric
designed to identify code that is both complex and poorly tested, making it
risky to modify.

---

## 1. The CRAP Formula

For any given function or method $m$:

$$\text{CRAP}(m) = \text{comp}(m)^2 \times (1 - \text{cov}(m)/100)^3 + \text{comp}(m)$$

Where:

- **$\text{comp}(m)$**: The **cyclomatic complexity** of the function.
- **$\text{cov}(m)$**: The **test coverage** percentage of the function
  (between $0$ and $100$).

### Interpretation

- **$\text{CRAP} < 5$**: Low risk. Well-tested or extremely simple.
- **$\text{CRAP} \in [5, 20]$**: Medium risk. Acceptable, but monitor for
  complex parts.
- **$\text{CRAP} \in (20, \infty)$**: High risk. Danger zone; requires
  refactoring to reduce complexity or adding targeted unit tests to increase
  coverage.

---

## 2. Tooling: `cargo-crap` & `cargo-llvm-cov`

To calculate CRAP metrics for this Rust codebase, we combine two tools:

1. **`cargo-llvm-cov`** (v0.9.1, via `cargo llvm-cov nextest`): Generates
   line coverage reports in the standard `Lcov` format.
2. **`cargo-crap`**: Parses Rust source code to calculate cyclomatic
   complexity and consumes the `lcov.info` file to compute individual
   function CRAP scores.

Both tools are managed by **`mise`** in `mise.toml`.

---

## 3. Implementation Details

### Step 3.1: Project-Level Configuration (`.cargo-crap.toml`)

A `.cargo-crap.toml` file is located at the project root:

```toml
# .cargo-crap.toml
threshold = 20.0
fail-above = false      # Set to true to enforce gating in CI
missing = "pessimistic" # Treat missing coverage as 0%
exclude = [
    "tests/**",
    "benches/**",
]
```

### Step 3.2: Configure `mise` Tasks (`mise.toml`)

The tool is declared in `mise.toml` under `[tools]`:

```toml
"cargo:cargo-crap" = "latest"
```

Coverage collection and CRAP analysis are two `mise` tasks — `crap` depends
on `coverage:lcov`:

```toml
[tasks."coverage:lcov"]
description = "Generate LCOV coverage report for CRAP analysis"
sources = ["@group:rust"]
outputs = ["lcov.info"]
run = 'cargo llvm-cov nextest --workspace --all-features --lcov --output-path lcov.info --ignore-filename-regex "$COVERAGE_IGNORE_REGEX"'

[tasks.crap]
description = "Run Change Risk Anti-Patterns (CRAP) analysis"
depends = ["coverage:lcov"]
sources = ["@group:rust", ".cargo-crap.toml"]
outputs = ["lcov.info"]
run = "cargo crap --lcov lcov.info"
```

Filename exclusions come from `COVERAGE_IGNORE_REGEX` in `mise.toml`'s
`[env]` (a POSIX ERE passed as `--ignore-filename-regex`); llvm-cov's own
default ignores apply on top.

---

## 4. CI/CD Integration

To gate pull requests based on the CRAP score, add a step to the GitHub Actions
workflow. Since tools are managed by `mise`, use the `mise` action or command
runner:
```yaml
- name: Install tools via mise
  uses: jdx/mise-action@v2

- name: Run CRAP Gating
  run: mise run crap -- --fail-above
```

---

## 5. Local Usage

Run the analysis locally with:

```bash
mise run crap
```
