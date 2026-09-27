# messrust

Catch maintainability problems in Rust before they calcify: oversized functions
and types, tangled dependencies, dead private code, muddy naming, and other
mess that reviews keep rediscovering.

`messrust` is a local CLI. It parses Rust source, never builds or runs your
project, and needs no project dependencies installed.

## Quick start

```console
cargo install messrust
messrust src text rust --ignore-tests
```

That scans `src` with the recommended low-noise policy and prints findings on
stdout. Exit `0` is clean, `2` means findings, `1` means the tool or a source
file failed.

Common next steps:

```console
messrust src text rust,opinionated --ignore-tests
messrust src sarif rust --ignore-tests --reportfile reports/messrust.sarif
messrust src github rust --ignore-tests
```

Full command syntax, options, and discovery: [docs/usage.md](docs/usage.md).

What each rule checks:

- [Code size and complexity](docs/codesize.md)
- [Name length and intent](docs/naming.md)
- [Unused code](docs/unusedcode.md)
- [Control flow and direct dependencies](docs/cleancode.md)
- [Design, errors, and cohesion](docs/design.md)
- [Rust style names](docs/controversial.md)
- [Implicit inputs and outputs](docs/explicitness.md)

## Install

With Homebrew on macOS:

```console
brew install quality-gates/tap/messrust
messrust --version
```

With Cargo:

```console
cargo install messrust
messrust --version
```

From a local checkout:

```console
cargo build --release
./target/release/messrust src text rust --ignore-tests
```

## Tune the gate

Start with `rust`. Add `opinionated` when you want the stricter checks the
recommended set leaves out. Point at a custom XML ruleset when thresholds or
membership need to live in the repo:

```xml
<ruleset name="team policy">
  <rule ref="rust">
    <exclude name="DevelopmentCodeFragment" />
  </rule>
  <rule ref="LongVariable">
    <priority>2</priority>
    <properties>
      <property name="maximum" value="50" />
    </properties>
  </rule>
</ruleset>
```

```console
messrust src text path/to/team-policy.xml --ignore-tests
```

## Suppress one intentional exception

```rust
// messrust-disable-next-line LongVariable
let deliberately_long_variable_name_for_a_fixture = 1;
```

Region form: `messrust-disable` / `messrust-enable`. Names are case-insensitive.
`--strict` keeps suppressed findings visible in the report.

## Drop it into CI

```yaml
# GitHub Actions
- run: cargo install messrust --locked
- run: messrust src github rust --ignore-tests
```

```yaml
# GitLab Code Quality
script: messrust src gitlab rust --reportfile gl-code-quality-report.json
artifacts:
  reports:
    codequality: gl-code-quality-report.json
```

This repository also self-checks after building the release binary. A finding
fails the job with exit code `2`.

## Maintainers

Command reference and report formats: [docs/usage.md](docs/usage.md).
Homebrew release and recovery steps:
[docs/homebrew-release.md](docs/homebrew-release.md).

Mutation measurement uses `mutarust` with the committed `mutarust.yml` policy
(`min_msi: 75`, `min_covered_msi: 80`). Thresholds do not move down. On machines
with 16 GB RAM or less, cap workers and compile jobs:

```console
CARGO_BUILD_JOBS=4 mutarust --config mutarust.yml --coverage --workers 1 \
  --min-msi 75 --min-covered-msi 80 \
  --test-flags "-- --skip pack_install_smoke" .
```

On macOS, point `TMPDIR` at a real path under `$HOME` before a coverage run so
LCOV paths resolve correctly.

Development checks:

```console
cargo test --all-targets --locked
```
