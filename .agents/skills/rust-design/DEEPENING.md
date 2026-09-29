# Deepening

Deepening concentrates coherent behaviour, policy, state, and invariants behind
a simpler seam.

It may require splitting one thing, joining several things, moving
responsibility, replacing an abstraction, or deleting it.

Choose the transformation from the concern.

## Decompose to discover

Use decomposition as a **probe** when separating behaviour can expose
independently meaningful knowledge.

Begin with a hypothesis such as:

- separate invariants may exist
- distinct state or lifecycle may exist
- abstraction levels are mixed
- independent policies are entangled
- a hidden state machine may exist
- one orchestration contains several semantic operations
- only part of the behaviour needs a dependency

Separate conceptually before committing to permanent code structure where
practical.

Observe whether the provisional pieces converge around:

- shared state
- one invariant
- one lifecycle
- one policy
- one domain vocabulary
- one transformation
- one reason for change

The desired process is:

```text
behaviour
→ provisional decomposition
→ coherent knowledge becomes visible
→ latent concept
→ deliberate consolidation behind a seam
```

If decomposition reveals no independent concept and does not improve knowledge
placement, it has not justified another module.

## Consolidate fragmented responsibility

Consolidate when callers currently assemble one concern from several shallow
pieces.

Evidence includes:

- fixed call sequences
- shared invariants spread across helpers
- repeated orchestration
- several modules changing together
- callers coordinating internal details
- intermediate types whose purpose is crossing shallow seams
- sibling modules exposing one another's representations
- policy duplicated across multiple call sites

The result should remove knowledge from callers.

Moving functions into one file without changing responsibility or seams is not
deepening.

## Collapse shallow seams

Collapse a seam whose interface costs roughly as much knowledge as the behaviour
it hides.

Typical candidates include:

- forwarding wrappers
- one-to-one delegation layers
- redundant facades
- helper modules that merely rename operations
- traits with no meaningful variation
- adapters that exist without an actual substitution need

Use `codebase-design`'s deletion test:

> If the module disappeared, would its complexity reappear in callers, or would
> the abstraction simply vanish?

A useful module concentrates knowledge. A shallow layer often only redistributes
names.

## Redesign a real seam

Keep the responsibility but change the seam when the module is meaningful and
its callers learn the wrong things.

A redesign may:

- raise parameters or results to a more semantic level
- absorb ordering requirements
- hide storage or serialization representation
- enforce invariants internally
- replace several mechanism-level operations with one semantic operation
- narrow visibility
- relocate dependency injection
- present domain failures instead of child implementation failures

Use `codebase-design`'s Design It Twice process when the user requests
alternative interface designs. Otherwise, compare remaining viable choices
locally; explain a rejected choice only when it changes the decision.

Compare by depth, locality, caller knowledge, and upward composition where
a parent exists.

## Relocate ownership

Move behaviour when another module owns the state, invariant, lifecycle, or
policy that gives it meaning.

Prefer designs where the code enforcing an invariant has direct authority over
the state involved.

A utility location is weak ownership when behaviour actually belongs to a domain
concept elsewhere.

## Model an implicit concept

Make a concept explicit when doing so:

- concentrates an invariant
- eliminates invalid states
- gives a seam semantic vocabulary
- reduces caller coordination
- makes a meaningful transition explicit
- distinguishes values currently easy to confuse

Read [`MODELING.md`](MODELING.md) before choosing the Rust representation.

The concept justifies the type. The availability of a Rust pattern does not
justify the concept.

## Replace the wrong abstraction

Replace rather than incrementally repair when the current abstraction preserves
the wrong responsibility or seam.

A replacement plan must identify:

- callers to migrate
- behaviour to preserve
- tests whose seam changes
- types or layers superseded
- compatibility constraints
- dependencies that become unnecessary

Do not leave the superseded design beside the replacement without a concrete
migration reason.

## Remove what no longer earns its cost

Deletion is a design operation.

After deepening, search for:

- obsolete helpers
- shallow wrappers
- redundant modules
- parallel representations
- unnecessary conversions
- superseded traits
- former test seams
- compatibility scaffolding
- dependencies used only by removed architecture

A deeper module should often pay a **deletion dividend**.

## Evaluate local depth

Ask what the immediate caller gains for what it must learn.

A deep seam tends to:

- hide policy or mechanism
- own invariants
- absorb ordering knowledge
- expose semantic rather than incidental representation
- make correct use natural
- provide several callers one implementation of shared knowledge

Interface size is semantic.

Method count, source lines, and file count do not measure it.

## Evaluate depth propagation

For a lower-level candidate with a parent, inspect that parent.

Ask what disappeared from the parent's required knowledge:

- child concepts
- child dependencies
- child invariants
- ordering rules
- representation details
- child-specific failures
- reasons to modify the parent when the child changes

Good lower-level design should often raise the abstraction level available to
the parent.

A locally elegant child that leaves its parent equally complicated may not earn
an independent seam.

## Compare the transformation

For a concrete design claim, compare current caller knowledge and change paths
with the proposed seam. Ask which facts leave callers or a parent, which policy
finds an owner, and what becomes removable, if anything. Carry forward the
gauges, raw evidence, and frozen scenario selected in discovery; when a new
claim arises, screen it with [`METRICS.md`](METRICS.md) and select an
evidence source from [`TOOLING.md`](TOOLING.md).

For a review, state expected effects as predictions. For an implementation,
compare observed results against the same current evidence. Explain material
trade-offs, including additional caller burden accepted to eliminate invalid
transitions. No design has to improve every applicable dimension.

## Protect sibling isolation

When a parent contains several child modules, inspect whether their knowledge is
actually independent.

Frequent cross-sibling changes may indicate:

- responsibility split at the wrong place
- a shared invariant without an owner
- leaked representation
- orchestration that belongs in the parent
- children that should be consolidated

Separate files do not establish separate responsibilities.

## Contain vertical leakage

Look for lower-level knowledge escaping upward:

- descendant types in ancestor interfaces
- storage or serialization details
- child-specific error structures
- execution order
- internal state constructors
- lower-level configuration
- imports bypassing the intended owner

A seam earns its existence partly by containing such knowledge.

## Validate an implemented transformation

When code changes, verify behaviour through the intended seam and check the
dependency direction, visibility, parent simplification, and superseded
structure relevant to the design claim. Select evidence with
[`TOOLING.md`](TOOLING.md). Follow `codebase-design`'s replace-don't-layer
testing rule when a deeper interface supersedes shallow ones. Retain lower-level
tests when they protect an independent behavioural contract.

## Size is an indicator, never a verdict

Large functions or files can be useful places to inspect because they may
contain more opportunities for mixed responsibility.

Small files or many modules can also be useful places to inspect because they
may reveal fragmentation.

Neither fact decides the design.

A cohesive 2,000-line implementation with a deep seam may be the correct shape.
Five cohesive, clearly named files may be much clearer when five distinct
responsibilities exist.

Prefer the shape implied by knowledge ownership and seams.

## Reconsider the boundary

After a meaningful proposal or transformation, inspect downward for a newly
visible coherent concern and upward for a simpler parent or sibling where one
exists. Revisit discovery when that inspection exposes a consequential new
candidate.

Stop a pass when it adds no material candidate or changes no design decision.
Record what was inspected and what remains uncertain; a review need not prove
there is no further improvement elsewhere.

## Completion criterion

A candidate is resolved when current and proposed caller knowledge or change
paths are compared using the selected evidence, and the design explains why
responsibilities belong where they do, what each seam hides and asks callers
to know, how any parent changes, what becomes removable or why nothing does,
and which trade-offs remain. Record each selected gauge or reason it is
inapplicable or not measured. A review separates observed current evidence
from predicted effects; an implementation additionally verifies behaviour
and observes the claimed effects.
