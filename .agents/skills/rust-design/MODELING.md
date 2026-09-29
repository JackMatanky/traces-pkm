# Modeling

Translate discovered concepts and seams into Rust.

This step begins **after** discovery and structural deepening have established
what semantic responsibility should exist.

Use `rust-skills` as the Rust-specific reference. Load only the categories
relevant to the current candidate.

The type system is part of the interface. Stronger compile-time guarantees are
useful only when their leverage exceeds the additional knowledge imposed on
callers.

## Start from semantics

Before choosing a construct, state:

- what concept exists
- what invariant or policy it owns
- what operations belong to it
- what states are legal
- who should construct it
- who should observe it
- which representation details should remain hidden
- whether real implementation variation exists

Then compare Rust representations.

## Use metrics selectively

Some representation decisions admit stronger comparisons. Their definitions live
in [`METRICS.md`](METRICS.md).

Use them only when they test the actual design claim:

```text
enum / closed-state redesign
→ ISR

typestate or state-machine redesign
→ ITE + IKL

trait seam
→ V + IKL

generic abstraction propagated through parents
→ GPD

visibility redesign
→ VE

new child abstraction
→ IKL + KC

error redesign
→ IKL error atoms + L
```

A type-system improvement is strongest when it removes invalid states or caller
knowledge without exporting equivalent complexity elsewhere.

Do not introduce an elaborate representation merely to improve a metric.

## Ordinary function

Prefer an ordinary function when:

- the operation is stateless or works entirely from its parameters
- no durable invariant needs ownership
- no new semantic identity is needed
- callers gain little from another type

A free or associated function can still be a deep Module under
`codebase-design`.

Do not introduce a type merely to give one function a namespace.

## Newtype

Consult the relevant `rust-skills` newtype rules when distinct semantics are
currently represented by the same underlying type.

A newtype is promising when it:

- prevents confusing semantically different values
- owns validation or construction constraints
- creates useful domain vocabulary at a seam
- enables behaviour specific to the concept

A wrapper that enforces nothing, distinguishes nothing, and simplifies no seam
has weak justification.

## Validated type

Consult `rust-skills`' parse-don't-validate guidance when callers repeatedly
carry raw values plus knowledge about whether validation has happened.

Prefer moving validity to construction when:

- validity has a stable definition
- downstream operations assume it
- repeated runtime checks can disappear
- invalid values should not enter the deeper module

Keep external parsing or transport representation outside the validated domain
type where that preserves a cleaner seam.

## Struct

Use a struct when several values form one concept and their relationship,
lifecycle, or invariant is meaningful.

Ask whether methods truly belong with the state they act upon.

A struct is not automatically superior to functions. Its value comes from
ownership of coherent state and behaviour.

## Enum

Consult `rust-skills` enum and pattern-matching guidance for closed alternatives
or mutually exclusive states.

Enums are especially useful when they replace:

- correlated booleans
- incompatible `Option` combinations
- tag-plus-payload structures with invalid combinations
- conditionals that repeatedly rediscover the same closed state space

Make variants represent semantic alternatives rather than incidental execution
steps.

Where the state space is exactly countable, compare `ISR` before and after.

## Typestate

Consult `rust-skills` typestate guidance when legal operations materially depend
on state and invalid transitions are both meaningful and worth preventing at
compile time.

Typestate is strongest when:

- states are stable and well-defined
- transitions are central to correctness
- different states expose materially different legal operations
- callers benefit from compiler enforcement

Prefer an ordinary enum or validated runtime state when typestate would spread
generic parameters, marker types, or conversion ceremony through unrelated
callers.

Where transitions are exactly enumerable, compare `ITE`. Compare the gain
against `IKL` so compile-time safety is not purchased with disproportionate
caller complexity.

## Trait

Introduce a trait because a seam needs meaningful behavioural variation, not
because traits are an available abstraction mechanism.

Look for:

- multiple legitimate implementations
- an owned external dependency requiring an adapter
- a test substitute justified by `codebase-design`'s dependency categories
- a stable behavioural contract independent of implementation details

A single implementation can still justify a trait where a real external or
deployment seam exists. A speculative implementation does not.

Consult `rust-skills` for:

- associated type vs generic parameter
- static vs dynamic dispatch
- object safety
- sealed traits
- default methods
- coherence constraints

Keep trait vocabulary at the lowest level that needs the variation.

Record `V` when variation is part of the justification. A test double counts
only when the dependency category makes that seam meaningful under
`codebase-design`.

## Generics

Use generics where callers genuinely need parametric variation or static
dispatch.

Treat generic parameters and bounds as interface cost.

A generic abstraction is weak when:

- only one concrete type exists
- every caller supplies the same type
- parameters flow unchanged through several layers
- callers must understand implementation variation they do not care about

Prefer concrete types until variation produces actual leverage.

Use `GPD` when a generic parameter appears to leak implementation variation
through architectural levels that do not semantically use it.

## Ownership and borrowing

Use ownership to reinforce responsibility.

Ask:

- Which module should own this state?
- Which values should be borrowed temporarily?
- Is cloning hiding unclear ownership?
- Is shared ownership exposing a responsibility problem?
- Does interior mutability reveal a seam that should move?
- Does a lifetime relationship belong in the caller-facing interface?

Consult relevant `rust-skills` ownership rules when answering these questions.

Do not optimize borrowing cleverness at the cost of a substantially harder
semantic interface.

## Visibility

Rust visibility should implement the intended seam.

Consult `rust-skills` project-structure and visibility guidance.

Prefer the narrowest visibility consistent with the real caller set:

- private
- `pub(super)`
- `pub(crate)`
- public

Use re-exports intentionally to present the chosen interface.

Broadening visibility to make an internal decomposition convenient is evidence
that ownership or seam placement may be wrong.

Use `VE` when unnecessarily broad visibility is part of the suspected seam
problem.

## Errors

Error types are part of the Interface because callers must understand their
failure modes.

Design errors at the abstraction level of the seam.

Ask:

- Which failures are meaningful to this caller?
- Which lower-level errors are implementation details?
- Which distinctions must callers react to?
- Which context should be preserved without leaking mechanisms?

Consult the relevant `rust-skills` error guidance after answering these
questions.

Do not mechanically mirror every child error variant through every parent.

## Collections and representation

Choose concrete representation from required semantics and measured workload.

Representation details should remain behind the seam when callers do not need
them.

Consult `rust-skills` collection, memory, serde, and performance guidance only
where the discovered design makes those choices material.

Do not distort a semantic seam for speculative micro-optimization.

## Module and file layout

Lay out files after responsibility is understood.

A file should have a concise name that predicts the coherent knowledge found
inside it.

Several cohesive files are preferable when they represent meaningful
responsibilities and improve navigation. A single large file is acceptable when
its contents remain one coherent responsibility and splitting would introduce
shallow seams or navigation without leverage.

Filesystem structure implements the design; it does not define it.

Consult `rust-skills` project-structure guidance where Rust module mechanics,
visibility, or re-exports matter.

## Compare candidate representations

For non-trivial choices, compare alternatives against:

- semantic precision
- invalid states prevented
- caller knowledge
- seam depth
- parent simplification
- ownership clarity
- error clarity
- compile-time and runtime cost
- cognitive cost
- future variation supported by evidence
- implementation knowledge hidden

Prefer the least elaborate representation that preserves the needed semantics
and depth.

## Completion criterion

Modeling is complete when:

- every introduced abstraction corresponds to discovered semantics
- invalid states are prevented at the cheapest useful level
- traits and generics correspond to real variation or seam requirements
- ownership matches responsibility
- visibility matches intended callers
- errors match the seam's abstraction level
- implementation representation remains private where possible
- the resulting Rust design makes the parent easier, not merely the child more
  sophisticated
