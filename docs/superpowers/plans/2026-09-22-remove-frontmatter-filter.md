# Remove Frontmatter Filter Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Delete the `frontmatter` template filter so `src/template/engine/yaml.rs` contains only `to_yaml` and `from_yaml`.

**Architecture:** Pure deletion across two files: remove the filter function, its fence-scanning helper, its registration, its tests, and every doc reference. No new code; `src/note/` remains the sole owner of frontmatter extraction. Spec: `docs/superpowers/specs/2026-09-22-remove-frontmatter-filter-design.md`.

**Tech Stack:** Rust, minijinja, mise tasks (`test`, `verify`), hk pre-commit checks.

---

### Task 1: Strip `frontmatter` from `yaml.rs`

**Files:**
- Modify: `src/template/engine/yaml.rs` (module docs lines 1-15, `register` lines 30-34, filter fns lines 68-127, tests `mod frontmatter` lines 318-498)

- [x] **Step 1: Rewrite the module doc comment**

Replace the entire doc comment at the top of `src/template/engine/yaml.rs` with:

```rust
//! Register YAML serialization and deserialization filters for templates.
//!
//! [`YamlOps`] adds two stateless filters:
//!
//! - `to_yaml`: serializes a template value to a YAML string.
//! - `from_yaml`: parses a YAML string into a template value.
//!
//! Each filter is a plain function registered through
//! [`Environment::add_filter`]. None carry shared state, so there is no
//! [`Object`] dispatch.
//!
//! [`Object`]: minijinja::value::Object
//! [`Environment::add_filter`]: minijinja::Environment::add_filter
```

- [x] **Step 2: Remove the registration line**

In `YamlOps::register`, change:

```rust
    pub(super) fn register(env: &mut Environment<'static>) {
        env.add_filter("to_yaml", to_yaml);
        env.add_filter("from_yaml", from_yaml);
        env.add_filter("frontmatter", frontmatter);
    }
```

to:

```rust
    pub(super) fn register(env: &mut Environment<'static>) {
        env.add_filter("to_yaml", to_yaml);
        env.add_filter("from_yaml", from_yaml);
    }
```

- [x] **Step 3: Delete the filter implementation**

Remove the `extract_frontmatter_str` function (doc comment + body) and the `frontmatter` function (doc comment + body) — everything between the end of `from_yaml` and the `#[cfg(test)]` line. `from_yaml` is the last remaining item before the test module.

- [x] **Step 4: Delete the `mod frontmatter` test module**

Remove the entire `mod frontmatter { ... }` block (the last submodule inside `mod tests`), from `mod frontmatter {` through its closing brace just before `mod tests`'s closing brace. Keep `mod to_yaml` and `mod from_yaml` intact.

- [x] **Step 5: Run the template unit tests**

Run: `mise run test -m template`
Expected: PASS except `evaluates_frontmatter_filter` and `evaluates_frontmatter_accessor` (both in `engine.rs`, Task 2's scope — they fail with `UnknownFilter: frontmatter` until Task 2 removes them). The yaml module's own tests must pass: `mise run test -m template -- template::engine::yaml` → 21/21 PASS.

- [x] **Step 6: Commit**

```bash
git add src/template/engine/yaml.rs
git commit -m "refactor(template): drop frontmatter filter from yaml ops"
```

### Task 2: Purge `frontmatter` references from `engine.rs`

**Files:**
- Modify: `src/template/engine.rs` (module doc lines 20-21, tests lines 618-651)

- [x] **Step 1: Update the module doc bullet**

In the helper-modules list, replace:

```rust
//! - [`yaml`] registers YAML serialization (`to_yaml`), parsing (`from_yaml`),
//!   and frontmatter extraction (`frontmatter`).
```

with:

```rust
//! - [`yaml`] registers YAML serialization (`to_yaml`) and parsing
//!   (`from_yaml`).
```

- [x] **Step 2: Delete the two frontmatter tests**

Remove the entire `evaluates_frontmatter_filter` test function (including its `#[test]` attribute) and the entire `evaluates_frontmatter_accessor` test function. `evaluates_from_yaml_filter` immediately after them stays.

- [x] **Step 3: Run the engine tests**

Run: `mise run test -m template`
Expected: PASS

- [x] **Step 4: Commit**

```bash
git add src/template/engine.rs
git commit -m "refactor(template): remove frontmatter filter engine tests and docs"
```

### Task 3: Full verification

**Files:** none modified

- [x] **Step 1: Run the full gate**

Run: `mise run verify`
Expected: PASS (fmt, check, lint, test — 0 failures)

- [x] **Step 2: Confirm no stray references**

Run: `rg 'frontmatter' src/template/`
Expected: no matches for the removed filter — i.e. no `| frontmatter` usages, no `add_filter("frontmatter"`, no references to `extract_frontmatter_str` or the filter function. Matches about *note* frontmatter metadata (e.g. in `engine/query.rs`) are legitimate and expected to remain.
