# Mutation Testing Setup

Mutation testing is a software testing methodology designed to evaluate the
quality and robustness of a test suite. By injecting small, artificial defects
("mutants") into the source code and running the test suite, mutation testing
determines whether the tests are capable of detecting these changes.

---

## 1. How Mutation Testing Works

When mutation testing runs, the tool automatically generates modified versions
of the codebase:

- **Mutant**: A single, synthetic bug injected into a function (e.g., changing
  `x > y` to `x >= y`, replacing a return value, or deleting a function body).
- **Caught**: The test suite fails when run against the mutated code. This is
  the desired outcome.
- **Survived**: The test suite passes despite the injected bug. This indicates
  a gap in test coverage or weak assertions.
- **Unviable**: The mutated code fails to compile. These mutants are ignored.

Unlike standard code coverage, which only measures if lines of code are
executed, mutation testing measures the **effectiveness** and assertiveness of
your tests.

---

## 2. Tooling: `cargo-mutants` & `cargo-nextest`

To implement mutation testing in this Rust project, we use:

1. **`cargo-mutants`**: An actively maintained mutation testing tool built
   specifically for Rust. It runs mutations in separate, isolated
   copy-on-write scratch directories.
2. **`cargo-nextest`**: A high-performance test runner for Rust. Since
   mutation testing requires repeatedly running tests, `nextest` provides
   critical speedups through its fail-fast mechanism and parallel execution
   model.

Both tools are managed via **`mise`** in `mise.toml`.

---

## 3. Implementation Details

### Step 3.1: Project-Level Configuration (`.cargo/mutants.toml`)

A configuration file is placed at the project root under `.cargo/mutants.toml`
to define default arguments and tool integrations:

```toml
# .cargo/mutants.toml
test_tool = "nextest"
features = ["test-utils"]
additional_cargo_args = ["--all-targets"]
```

### Step 3.2: Configure `mise` Tasks (`mise.toml`)

`cargo-mutants` is declared in `mise.toml` under `[tools]`:

```toml
"cargo:cargo-mutants" = "latest"
```

A `mutants` task is defined to run the analysis:

```toml
[tasks.mutants]
description = "Run mutation testing with cargo-mutants"
depends = ["check"]
run = "cargo mutants"
```

---

## 4. CI/CD Integration

Because mutation testing is resource-intensive, running a full scan on every
commit in CI can be slow. To integrate mutation testing efficiently in GitHub
Actions, use **differential mutation testing**:

```yaml
- name: Install tools via mise
  uses: jdx/mise-action@v2

- name: Run mutation testing on changed files
  run: mise run mutants -- --in-diff
```

### Key CI Options

- **`--in-diff`**: Limits mutation testing only to the lines/files modified in
  the current pull request or commit range.
- **`--baseline=skip`**: Speeds up runs by skipping the baseline check (running
  tests on clean code) if the CI suite has already passed.

---

## 5. Local Usage & Performance Optimization

Run mutation testing locally using `mise`:

```bash
mise run mutants
```

### Speed Optimization Flags

For larger crates or faster feedback loops during development:

1. **Test Only Uncommitted Changes**

   ```bash
   mise run mutants -- --in-diff
   ```

2. **Iterative Mode** (only retry mutants that survived the previous run)

   ```bash
   mise run mutants -- --iterate
   ```

3. **Limit to Specific Files/Modules**

   ```bash
   mise run mutants -- --file src/schema/resolver.rs
   ```

4. **Control Parallel Jobs**

   ```bash
   mise run mutants -- --jobs 4
   ```
