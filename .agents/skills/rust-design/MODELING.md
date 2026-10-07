# Modeling

## Contents

1. [Overview & Semantic Starting Points](#overview--semantic-starting-points)
2. [Ordinary Functions & Newtypes](#ordinary-functions--newtypes)
3. [Validated Types: Parse, Don't Validate](#validated-types-parse-dont-validate)
4. [Structs & Enums](#structs--enums)
5. [Typestate & Compile-Time State Machines](#typestate--compile-time-state-machines)
6. [Traits, Variation, & Generic Costs](#traits-variation--generic-costs)
7. [Ownership, Borrowing, & Visibility](#ownership-borrowing--visibility)
8. [Error Types as Part of the Seam](#error-types-as-part-of-the-seam)
9. [Module & File Layout (POLS)](#module--file-layout-pols)
10. [Comparing Candidate Representations](#comparing-candidate-representations)

---

## Overview & Semantic Starting Points

Translate discovered concepts and seams into idiomatic Rust.
The type system is part of the interface. Stronger compile-time guarantees are useful only when their leverage exceeds the additional knowledge and ceremony imposed on callers.

### Start from Semantics

Before choosing a Rust construct, state:

- What concept exists and what invariant or policy it owns
- What operations belong to it and what states are legal
- Who constructs it and who observes it
- Which representation details must remain hidden behind the seam
- Whether real variation or substitutability exists

---

## Ordinary Functions & Newtypes

### Ordinary Functions

Prefer a free or associated function when:

- The operation is stateless or works entirely from its parameters
- No durable invariant requires ownership
- No new semantic identity is needed

Do not introduce a type merely to give one function a namespace.

### Newtypes

A newtype is promising when it:

- Prevents confusing semantically different values of the same primitive type
- Owns construction validation constraints
- Creates domain vocabulary at an architectural seam

A wrapper that enforces nothing, distinguishes nothing, and simplifies no caller has weak justification.

---

## Validated Types: Parse, Don't Validate

Prefer moving validity to construction ("parse, don't validate"):

- Validity has a stable, closed definition
- Downstream operations assume validity and can discard runtime checks
- Invalid values cannot enter the deeper module

Ensure every safe construction, deserialization, and mutation path preserves the invariant; private fields alone do not help if a public constructor or conversion bypasses validation.

---

## Structs & Enums

### Structs

Use a struct when several values form one concept and their relationship or invariant is meaningful. Methods must truly belong with the state they act upon.

### Enums

Enums replace correlated booleans, incompatible `Option` combinations, and tag-plus-payload structures. Variants must represent semantic alternatives rather than incidental execution steps.
Where states are enumerable, ensure $\text{ISR} = 0$.

---

## Typestate & Compile-Time State Machines

Typestate is strongest when states are well-defined, transitions are central to correctness, and different states expose materially different legal operations:

```rust
// Hiding internal typestate transitions from high-level callers
pub struct Session<State> {
    inner: SessionInner,
    _state: std::marker::PhantomData<State>,
}
```

Prefer an ordinary enum or runtime validation when typestate would leak marker types, generic parameters, and conversion ceremony through unrelated caller interfaces.

---

## Traits, Variation, & Generic Costs

Introduce a trait because a seam needs meaningful behavioural variation or external dependency isolation, not as an ornamental pattern:

- Multiple legitimate implementations exist
- An owned external dependency requires a test or environment adapter
- A stable behavioural contract is needed independent of implementation details

Treat generic parameters and trait bounds as interface cost. Prefer concrete types until variation produces demonstrable leverage.

---

## Ownership, Borrowing, & Visibility

- Code enforcing an invariant must have direct authority over the state involved.
- Avoid shared ownership (`Arc`, `Rc`) or interior mutability (`Mutex`, `RefCell`) to mask unclear responsibility.
- Visibility implements the intended seam. Use scoped visibility (`pub(crate)`, `pub(super)`, `pub(in crate::ancestor)`) to prevent internal child details from leaking into public crates.

---

## Error Types as Part of the Seam

Error types are part of the interface because callers must understand and handle failure modes:

- Design errors at the abstraction level of the seam.
- Do not mechanically mirror every child error variant through every parent.
- Hide child implementation errors behind cohesive domain error classifications.

---

## Module & File Layout (POLS)

Structure file and directory layouts to match the cognitive map of the domain:

- Follow the **Principle of Least Surprise (POLS)**: locate state and types where semantic ownership implies.
- A single file exceeding 500 SLoC is a candidate for decomposing into cohesive child modules.
- Eliminate "junk drawer" files (`utils.rs`, `helpers.rs`, `common.rs`).

---

## Comparing Candidate Representations

For non-trivial choices, compare candidate representations against:

1. Semantic precision
2. Invalid states prevented ($\text{ISR} = 0$, $\text{ITE} = 0$)
3. Caller knowledge and cognitive load ($\text{IKL}$)
4. Seam depth and parent simplification
5. Compile-time vs runtime trade-offs

Prefer the least elaborate representation that preserves the needed semantics and recursive depth.
