# Reviewing and Refactoring a Test Suite

> Audit against the source, not just against the test file — a well-styled suite can still have real coverage holes.

## When to Use

You've been asked to review, audit, or refactor an existing test suite (a `#[cfg(test)] mod tests` block, a `tests/` file, or "are these tests good?" in general). Two failure modes to avoid: reviewing only the test file's style and missing that whole error variants have no test, or fixating on missing coverage and ignoring that the tests which do exist violate naming/structure/determinism conventions. Do both passes.

## Step 1: Build the Case Surface from the Source

Read the implementation under test and enumerate its case surface using the exact method in [`writing-a-test-suite.md` Step 2](writing-a-test-suite.md#step-2-enumerate-the-case-surface-mechanically) — every error variant, every match arm, every `None` path, every panic site, numeric/collection boundaries, state transitions. Build the same table:

```
get_user(id: u64) -> Result<User, UserError>
  - Ok: user exists
  - Err(UserError::NotFound)
  - Err(UserError::DbError(_))
```

Do this from the source, before looking at the test file — if you build the list by skimming the existing tests first, you'll only rediscover the cases someone already thought of, which defeats the point of the audit.

## Step 2: Map Existing Tests to Cases

Now read the test file. For each `#[test]`/`#[rstest]` function, identify which row from Step 1's table it exercises (use the test name and body, not just the name — a misleadingly-named test is itself a finding).

## Step 3: Report the Gap List

Anything in Step 1's table with zero tests mapped to it in Step 2 is a coverage gap. This is the review's primary deliverable:

```
Coverage gaps in `get_user`:
  - Err(UserError::DbError(_)) — no test. Repo failure path is untested.
  - id == 0 boundary — no test, and unclear if 0 is a valid id (check docs/callers).
```

An empty gap list is not automatically success — cross-check that Step 1 itself was thorough (did you actually walk every match arm, or stop at the obvious ones?).

## Step 4: Audit Each Existing Test Against Standards

For every test in the file, check:

| Check | Reference |
|---|---|
| Name follows the formula, no `test_foo`/`it_works`, one behavior per test | [`test-naming.md`](test-naming.md) |
| Module name is canonical (`validation`, `lookup`, etc.) if submodules are used | [`test-naming.md`](test-naming.md) |
| Clear Arrange/Act/Assert; no `unwrap`/`expect` outside Arrange | [`unit-testing.md`](unit-testing.md) |
| Equality assertions use `pretty_assertions`; enum checks use `matches!` | [`assertions.md`](assertions.md) |
| No hidden assertions buried in a helper function | [`unit-testing.md`](unit-testing.md) |
| No shared mutable state or uncontrolled time/randomness across tests | [`unit-testing.md`](unit-testing.md#determinism-and-speed) |
| Setup that must clean up on panic uses RAII, not a manual teardown line | [`fixtures-and-cleanup.md`](fixtures-and-cleanup.md) |
| `#[should_panic]` is only used for deliberate panics, not stand-in error handling | [`panics.md`](panics.md) |
| External dependencies (DB/HTTP/FS) are behind a trait/mock, not a real call | [`mocking.md`](mocking.md) |

## Step 5: Look for Tool-Fit Refactors

These aren't bugs, but they're worth flagging as improvements:

- **3+ near-duplicate tests differing only in a literal input/output** → collapse into one [`rstest`](table-driven-testing.md) block.
- **A hand-picked set of examples standing in for a general property** (roundtrip, idempotence) → add a [`proptest`](property-based-testing.md) alongside the concrete boundary cases, don't replace them.
- **A large `assert_eq!` against a whole struct/JSON blob** → [`snapshot-testing.md`](snapshot-testing.md) instead — more readable diffs, explicit review-on-change.
- **A test that's really exercising two unrelated behaviors joined by `and`** → split into two named tests.

## Step 6: Produce the Refactor Plan

Combine Steps 3–5 into one concrete, actionable list — not vague praise/criticism. Each item states the finding and the exact fix:

```
1. Missing test: Err(UserError::DbError(_)) — add
   `returns_db_error_when_repo_fails` in mod get_user.
2. `test_user_stuff` (line 42) — rename to
   `returns_none_when_email_not_found`; currently bundles two
   assertions for two different behaviors, split into two tests.
3. `parse_a`, `parse_b`, `parse_c` (lines 60-90) — identical bodies,
   different literals; collapse into one #[rstest] block.
4. `save_user` (line 110) — asserts on the whole `User` struct with
   plain assert_eq!; either assert only the changed field or switch
   to assert_debug_snapshot!.
```

If you're executing the refactor (not just reviewing), apply the fixes, then re-run [Step 1–3](#step-1-build-the-case-surface-from-the-source) to confirm the gap list is now empty and re-run `cargo nextest run`.

**Completion criterion:** a gap list (possibly empty) plus a per-test audit finding list (possibly empty) — never just "looks fine," since that's not checkable by anyone reading the review afterward.

## See Also

- [`writing-a-test-suite.md`](writing-a-test-suite.md) — the same enumeration method, used to write new tests instead of auditing old ones
- [`test-naming.md`](test-naming.md) — the full naming/module standard being audited against
- [`running-tests.md`](running-tests.md) — verifying the suite after a refactor
