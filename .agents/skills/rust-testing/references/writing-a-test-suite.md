# Writing a Test Suite for Existing Code

> Derive test cases from the code's actual branch surface, not from guessing at "edge cases."

## When to Use

You've been asked to write tests for a function, struct, or module that already exists (or that you just wrote). Follow this end to end rather than writing tests ad hoc — ad hoc testing reliably misses error variants and boundaries that a mechanical pass over the code catches for free.

## Step 1: Pin Down the Contract

Read the implementation, not just the signature. For each public item under test, write down (mentally or literally, for anything non-trivial):

- What does "correct" mean here? What invariant is this code responsible for upholding?
- What does the caller own vs. what does this function own?
- Is failure expected (`Result`/`Option`) or a bug (`panic!`/`assert!`/`unwrap`/`expect`)?

If you can't state the contract in a sentence, you don't understand the code well enough yet to test it well — go read its callers, or the type it belongs to, until you can.

**Completion criterion:** you can state, in one sentence per public item, what it promises to do and what it promises never to do.

## Step 2: Enumerate the Case Surface Mechanically

Walk the implementation and pull out every one of these as a candidate test case. This is a checklist, not a suggestion list — go through all six categories for every function under test, even the ones that seem obviously not applicable, and note "n/a" rather than skipping silently.

| Source in the code | What it produces |
|---|---|
| Every `return Err(...)` / `?`-propagated error site | One case per distinct error variant reachable from this function |
| Every `match`/`if let` arm on an enum parameter or field | One case per arm |
| Every `Option::None` return path | One case for the "not found"/"absent" state |
| Every `panic!`/`assert!`/`unwrap`/`expect` in the implementation | Either a `#[should_panic]` case (if the panic is deliberate API contract — see [`panics.md`](panics.md)) or a flag that this should return `Result` instead |
| Numeric parameters | `0`, negative (if signed), the type's `MIN`/`MAX`, one past a documented bound, and the value that triggers overflow/underflow if arithmetic is involved |
| Collection/string parameters | empty, single element, the documented capacity/length boundary, and — for strings — unicode/multi-byte characters if the function does any byte-level indexing |
| State machines / stateful types | every valid transition, and at least one attempted invalid transition |

Build this as a literal list before writing any test code:

```
get_user(id: u64) -> Result<User, UserError>
  - Ok: user exists                              → returns_user_when_found
  - Err(UserError::NotFound): id has no user      → returns_not_found_when_missing
  - Err(UserError::DbError(_)): repo call fails    → returns_db_error_when_repo_fails
  - id == 0 (boundary, no documented meaning)      → n/a, 0 is a valid id here
```

**Completion criterion:** every row in the table above is either mapped to at least one planned case, or explicitly marked `n/a` with a one-line reason. An empty or skipped row is the single most common way a test suite quietly ships with a coverage hole.

## Step 3: Group Cases into Units of Work

Cluster the cases from Step 2 by the function/behavior they belong to, and name each group using the canonical module names in [`test-naming.md`](test-naming.md) (`constructor`, `validation`, `lookup`, `parse`, etc.). This becomes your `#[cfg(test)] mod tests { mod <group> { ... } }` structure — see [`unit-testing.md`](unit-testing.md) for the shape.

## Step 4: Pick a Test Type per Case

For each case, decide what kind of test it is using the skill's [decision tree](../SKILL.md#which-test-do-i-need):

- Plain `#[test]` — the default for a single case with a fixed input/output.
- Several cases differing only in input/output literals → collapse into one [`rstest`](table-driven-testing.md) block instead of writing them out one by one.
- A property that should hold across a *class* of inputs (roundtrip, idempotence, invariant) rather than fixed examples → [`proptest`](property-based-testing.md), in addition to, not instead of, the concrete boundary cases from Step 2.
- The case requires a DB/HTTP/filesystem dependency → extract a trait, inject a fake/mock — see [`mocking.md`](mocking.md).
- The case involves `async fn` → [`async-testing.md`](async-testing.md).
- The case's expected output is large/structured (rendered error, JSON) → [`snapshot-testing.md`](snapshot-testing.md) instead of a hand-written `assert_eq!`.

## Step 5: Write Each Test

- Name it with the formula in [`test-naming.md`](test-naming.md).
- Structure it Arrange/Act/Assert per [`unit-testing.md`](unit-testing.md) — `unwrap`/`expect` only in Arrange.
- Assert with `pretty_assertions::assert_eq!`/`matches!` per [`assertions.md`](assertions.md), with a message that states what was expected and why.
- If the case needs setup that must clean up even on panic, use an RAII guard — [`fixtures-and-cleanup.md`](fixtures-and-cleanup.md).

## Step 6: Verify

- Every row from Step 2's table now has a test with a name you can point to. If any row is still unmapped, go back to Step 4 — don't ship a suite with a known, silently-dropped case.
- Run `cargo nextest run` (and `cargo test --doc` if you touched doc examples) — see [`running-tests.md`](running-tests.md).
- Re-read the suite once as a reviewer would — see [`reviewing-a-test-suite.md`](reviewing-a-test-suite.md) — before calling it done. Writing and reviewing are different mental modes; do both even when you're the only one who touched the file.

**Completion criterion for the whole suite:** the Step 2 table has zero unmapped, non-`n/a` rows, every test name follows the naming formula, and `cargo nextest run` is green.

## See Also

- [`reviewing-a-test-suite.md`](reviewing-a-test-suite.md) — the same case-enumeration method, applied to an *existing* suite to find gaps
- [`unit-testing.md`](unit-testing.md) — suite shape and Arrange/Act/Assert detail
- [`test-naming.md`](test-naming.md) — the naming formula and canonical module names
