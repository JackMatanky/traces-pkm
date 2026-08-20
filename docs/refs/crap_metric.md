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

## 2. Tooling: `cargo-crap` & `cargo-tarpaulin`

To calculate CRAP metrics for this Rust codebase, we combine two tools:

1. **`cargo-tarpaulin`**: Generates line coverage reports in the standard
   `Lcov` format.
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

A `crap` task is defined to automate coverage collection and analysis:

```toml
[tasks.crap]
description = "Run Change Risk Anti-Patterns (CRAP) analysis"
depends = ["check"]
run = [
  "cargo tarpaulin --workspace --out Lcov",
  "cargo crap --lcov lcov.info"
]
outputs = ["lcov.info"]
sources = ["src/**/*.rs", "Cargo.toml", "Cargo.lock"]
```

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
