# Code size and complexity

Find functions and types that have grown hard to understand, test, or change.
These rules measure source structure only. They do not build or run the crate.

## Start here

```console
messrust src text rust --ignore-tests
```

The `rust` policy includes every code-size rule. To focus on the usual
function-level alarms:

```console
messrust src text rust --ignore-tests \
  --only CyclomaticComplexity,NPathComplexity,ExcessiveMethodLength
```

Start with the largest values. Do not split code only to make a number smaller.
First name the separate decisions or responsibilities, then extract a function
or type with a clear interface.

## What each rule catches

| Rule | Default | What it catches |
| --- | ---: | --- |
| `CyclomaticComplexity` | Report at 10 | A function has many decision points. |
| `NPathComplexity` | Report at 200 | A function has many independent paths. |
| `ExcessiveMethodLength` | Report at 100 lines | A function or method is large. |
| `ExcessiveClassLength` | Report at 1,000 lines | A type plus its inherent methods is large. |
| `ExcessiveParameterList` | Report at 10 parameters | A function needs too much input. |
| `ExcessivePublicCount` | Report at 45 public items | A type exposes a large public surface. |
| `TooManyFields` | Report over 15 fields | A struct or union stores many separate values. |
| `TooManyMethods` | Report over 25 methods | A type has many non-accessor methods. |
| `TooManyPublicMethods` | Report over 10 public methods | A type exposes many operations. |
| `ExcessiveClassComplexity` | Report at 50 | Methods on one type add up to high complexity. |

`ExcessiveMethodLength`, `ExcessiveClassLength`, `ExcessiveParameterList`,
`ExcessivePublicCount`, `CyclomaticComplexity`, `NPathComplexity`, and
`ExcessiveClassComplexity` fire when the value meets the threshold. The three
`TooMany...` rules fire only when the value is greater than the configured
maximum.

## Reading function complexity

`CyclomaticComplexity` lights up when one function owns many decisions. Error
handling, policy, and data conversion often pile into the same body.

`NPathComplexity` estimates independent paths. It grows faster than cyclomatic
complexity when independent branches sit in sequence, so a function can look
fine on CCN and still be hard to test.

Useful first moves:

1. Replace repeated conditions with one named predicate.
2. Return early for errors and special cases.
3. Move one independent operation into a small function.
4. Replace related booleans with an enum when the states are exclusive.
5. Prefer a `match` when it makes the state cases explicit.

## Size of functions and types

`ExcessiveMethodLength` counts the full source span of a free function or
method. Blank and comment-only lines count by default; set
`ignore-whitespace=true` in a custom ruleset to skip them. Comment detection
skips string literals, so a `/*` or `//` inside a quoted string does not start
comment state; string content still counts as code.

`ExcessiveClassLength` applies to `struct`, `union`, `enum`, and `trait`. It
adds the type span and the spans of methods attached to that type. A method
whose span sits fully inside the type span counts once; for a trait, the
default method bodies already sit inside the trait span and add nothing.

A long item is not always wrong. Generated tables, protocol maps, and direct
translations can stay clear at size. Raise the threshold or suppress the one
case instead of forcing a pointless split.

## Interface size

`ExcessiveParameterList` counts typed parameters. It skips `self` and typed
wildcards such as `_: Context`. A finding often means related values want a
small input struct—only group values that belong together.

`ExcessivePublicCount` counts public fields and public methods.
`TooManyPublicMethods` counts public methods only. A large public surface is
expensive because more callers can depend on it. Trait methods count as public
only when the trait itself is public.

## Type size and responsibility

`TooManyFields` counts struct and union fields. Enum variants are not fields.

`TooManyMethods` counts inherent methods and methods declared on a trait. It
does not count methods in `impl Trait for Type` blocks. Names that start with
`set`, `get`, `is`, `has`, or `with` are skipped by default.
`TooManyPublicMethods` uses the same name filter.

`ExcessiveClassComplexity` sums the cyclomatic complexity of methods on a type.
It can fire when a type owns too many decisions even if no single method is
huge.

## How complexity is counted

Cyclomatic complexity starts at 1 and adds 1 for each:

- `if` / `if let`
- `while` / `while let`
- `for`
- `loop`
- non-wildcard `match` arm
- match guard
- `&&` / `||`

A lone `_` match arm is the default and does not increase CCN.

NPath follows the usual independent-path formulas:

- `for`: expression paths + 1 + body paths
- `match`: scrutinee paths + sum of arm body paths, including `_`
- `loop`: 1 + body paths

Each free function and method gets its own values. Nested `fn` items and
closures add their decision points to the enclosing function for cyclomatic
complexity; NPath treats a nested item statement as linear code.

## Set thresholds for your project

```xml
<ruleset name="project code size">
  <rule ref="codesize/CyclomaticComplexity">
    <properties>
      <property name="reportLevel" value="12" />
    </properties>
  </rule>
  <rule ref="codesize/NPathComplexity">
    <properties>
      <property name="minimum" value="250" />
    </properties>
  </rule>
  <rule ref="codesize/ExcessiveMethodLength">
    <properties>
      <property name="minimum" value="120" />
      <property name="ignore-whitespace" value="true" />
    </properties>
  </rule>
</ruleset>
```

```console
messrust src text path/to/code-size.xml --ignore-tests
```

Commit the XML so local runs and CI share the same limits. Change one limit at
a time. A baseline the team trusts beats a strict policy everyone ignores.

## Suppress one intentional case

```rust
// messrust-disable-next-line ExcessiveMethodLength
fn generated_parser_table() {
    // Large generated body.
}
```

Use `--strict` when a review should still see suppressed findings.
