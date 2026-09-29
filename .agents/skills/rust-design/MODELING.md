# Modeling

Translate discovered concepts and seams into Rust.

Normally start after discovery and deepening establish the responsibility and
seam. If these are already established, confirm their contract and model them
directly.

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

A type-system improvement is strongest when it removes invalid states or caller
knowledge without exporting equivalent complexity elsewhere. Compare concrete
state spaces and caller obligations with [`METRICS.md`](METRICS.md) only when
that evidence clarifies a design decision.

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
type where that preserves a cleaner seam. Check that every safe construction,
deserialization, and mutation path preserves the invariant; private fields
alone do not help if a public conversion or deserializer bypasses validation.

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

Where the state space is exactly enumerable, compare valid and representable
states. A review can enumerate proposed variants but labels their effect a
prediction; an implementation checks the actual public construction paths.

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

Where transitions are exactly enumerable, compare which illegal operations
callers can express. Balance the gain against the knowledge and ceremony the new
interface demands.

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

Trace a generic through parent interfaces when implementation variation appears
to leak through levels that do not semantically use it.

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

Check the modules that legitimately call an item and the paths that expose it.
Rust also supports scoped visibility such as `pub(in crate::ancestor)` in
addition to `pub(super)` and `pub(crate)`. A `pub` item can be re-exported from
a private module, but a restricted item cannot be publicly re-exported merely to
widen its visibility. Evaluate the reachable public path as well as the item's
declaration. Choose visibility from the module tree and intended API, not a
numeric ranking.

Broadening visibility to make internal decomposition convenient may indicate
misplaced ownership. [`METRICS.md`](METRICS.md) describes how to inspect
effective reachability.

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
- caller and parent simplification where applicable
- ownership clarity
- error clarity
- compile-time and runtime cost
- cognitive cost
- future variation supported by evidence
- implementation knowledge hidden

Prefer the least elaborate representation that preserves the needed semantics
and depth.

## Completion criterion

For a proposal, explain how each proposed abstraction represents established
semantics, whether invalid states need prevention and at what cost, why traits
and generics serve real variation or a seam, and how ownership, visibility, and
errors fit the intended callers. State how implementation representation remains
private where possible and how callers or an existing parent become simpler.
Label unimplemented guarantees as predictions.

For implemented code, verify these claims against its actual constructors,
public paths, operations, and callers.
