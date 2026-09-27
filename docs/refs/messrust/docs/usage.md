# Using messrust

Point messrust at Rust source, pick a report format, pick a policy, and read the
findings. It never builds your crate, never runs your tests, and never needs
your project dependencies installed.

```console
messrust <path[,path...]> <format> <ruleset[,ruleset...]> [options]
```

Examples:

```console
messrust src text rust --ignore-tests
messrust app,lib json rust,opinionated --reportfile messrust.json
messrust . github path/to/team-policy.xml --exclude generated --ignore-tests
```

There is no implicit default ruleset. Pass `rust`, a component name, or a path
to an XML file.

## Choose a format

| Format | Use it when |
| --- | --- |
| `text` | You are reading findings in a terminal. |
| `ansi` | Same as `text`, always with color. |
| `json` / `xml` | You are writing a custom consumer or storing a full machine report. |
| `html` | You want a simple browsable table. |
| `github` | GitHub Actions should annotate the relevant lines. |
| `gitlab` | GitLab Code Quality should show findings on the merge request. |
| `checkstyle` | An existing Checkstyle-compatible CI step will ingest the file. |
| `sarif` | You are uploading to code scanning or another SARIF consumer. |

Structured formats carry stable path, line, rule, priority, message, context,
and suppression fields. Findings sort by file and source line.
`--reportfile <path>` writes the report to a file and leaves stdout empty.
`--color` colorizes text output.

## Choose a policy

| Ruleset | Intent |
| --- | --- |
| `rust` | Recommended default. Low noise on ordinary Rust. |
| `opinionated` | The stricter checks `rust` deliberately omits. Combine as `rust,opinionated`. |
| `codesize` | Size and complexity only. |
| `naming` | Name length, constants, boolean getters. |
| `unusedcode` | Unused locals, parameters, and private members. |
| `cleancode` | Boolean flags, terminal `else`, static calls, assignment-in-condition, duplicate struct fields. |
| `design` | Exits, empty error arms, coupling, globals, cohesion, development leftovers. |
| `controversial` | PascalCase types and snake_case identifiers. |
| `explicitness` | Implicit inputs and outputs of functions. Opt-in; not part of `rust` or `opinionated`. |

Comma-separated values may mix built-ins and custom XML paths:

```console
messrust src text rust,path/to/extra.xml --ignore-tests
```

What each rule catches, and which ones live in `rust` versus `opinionated`:

- [Code size](codesize.md)
- [Naming](naming.md)
- [Unused code](unusedcode.md)
- [Clean code](cleancode.md)
- [Design](design.md)
- [Rust style names](controversial.md)
- [Implicit inputs and outputs](explicitness.md)

## Options

| Option | Meaning |
| --- | --- |
| `--help`, `-h` | Show command help. |
| `--version` | Show the package version. |
| `--suffixes LIST` | Replace the default `.rs` suffix list. |
| `--exclude LIST` | Skip paths that contain any listed substring. |
| `--ignore-tests` | Skip test files, test directories, `#[cfg(test)]` modules, and `#[cfg(test)]` `impl` blocks and methods. Use this for a production-code gate. |
| `--only LIST`, `--enable LIST` | Keep only named rules already present in the loaded policy. Useful for bisecting a noisy run. |
| `--disable LIST` | Remove named loaded rules without writing new XML. |
| `--minimumpriority N` | Keep priorities `<= N`. Priority `1` is highest. |
| `--maximumpriority N` | Keep priorities `>= N`. |
| `--reportfile PATH` | Write the report to a file instead of stdout. |
| `--color` | Colorize text output. |
| `--strict` | Include findings hidden by source suppressions so exceptions stay auditable. |
| `--verbose`, `-v` | Print ruleset load diagnostics when a policy is not loading as expected. |
| `--ignore-errors-on-exit` | Return success despite operational or processing errors. Report contents still include the errors. |
| `--ignore-violations-on-exit` | Return success despite findings. Useful while adopting; report contents stay complete. |

## Exit status

Wire these into CI the same way you would any other quality gate.

| Code | Meaning |
| ---: | --- |
| `0` | Clean, or every relevant failure was explicitly ignored. |
| `1` | Command, configuration, discovery, report-write, or source processing error. Errors take precedence over findings. |
| `2` | Selected findings and no non-ignored processing error. |

Ignore-on-exit flags change only the process status. They never remove rows from
the report.

## What gets scanned

- Paths are comma-separated files or directories. Directories are walked
  recursively for matching suffixes.
- Default suffix is `.rs`. `--suffixes` replaces that list.
- Discovery skips `.git`, `target`, and `node_modules`.
- Tests are included unless `--ignore-tests` is set, so excluding test quality
  is an explicit choice.
- With `--ignore-tests`, a run with no source files to scan reports a discovery
  error instead of returning a clean result.
- With `--ignore-tests`, a method that only compiles with `test` on does not
  count toward the metrics of its type. A type then has the same code size,
  complexity, coupling, and cohesion as it has in a production build.
- With `--ignore-tests`, the rules do not see `#[cfg(test)]` modules. An
  `impl` block in a test module does not add to a production type. A read,
  call, or write in a test module still counts as a use of production code.
- A malformed or unreadable file becomes a processing error. Other valid files
  still analyze.

## Custom XML policy

Keep team thresholds next to the code:

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

References may name a built-in, one rule, `rulesets/name.xml`, or another XML
file relative to the current file. Rulesets can nest. Later references override
earlier priority and property values. `<exclude name="..."/>` removes a rule
from the referenced set.

Filters run after composition:

```console
messrust src text team-policy --only LongVariable --disable ShortVariable
messrust src json rust --minimumpriority 2 --maximumpriority 3
messrust src text rust --strict
messrust src text rust --suffixes .rs --exclude generated,vendor
```

`--only` / `--enable` cannot import rules absent from the selected policy.

## Suppressions in source

Waive one intentional finding without weakening the whole gate:

```rust
// messrust-disable-next-line CyclomaticComplexity,NPathComplexity
fn intentionally_dense() { /* ... */ }

// messrust-disable LongVariable
let deliberately_named_variable = value;
// messrust-enable LongVariable
```

Names are case-insensitive and may be separated by commas or spaces.
`disable-next-line` applies only to the following physical line. `disable`
starts a region on the following line; `enable` closes only the named rules.
Malformed directives are ignored. Normal reports omit suppressed findings;
`--strict` keeps them marked suppressed.

## Exploratory testing

- [2026-09-26 report](exploratory-testing/2026-09-26-messrust/REPORT.md)
