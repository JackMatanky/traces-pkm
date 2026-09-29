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

Collapse a seam whose interface costs roughly as much knowledge as the
behaviour it hides.

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

For consequential interfaces, use `codebase-design`'s Design It Twice process.

Compare alternatives by depth, locality, seam placement, caller knowledge, and
how well they compose upward.

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

Then inspect the parent.

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

For a substantial redesign, use the metrics selected during discovery from
[`METRICS.md`](METRICS.md).

Typical relationships are:

```text
deeper child seam
→ parent IKL decreases
→ KC becomes positive

implementation detail contained
→ L decreases

seam repaired
→ BR decreases

change localized
→ PD decreases for the frozen scenario

fragmented architecture replaced
→ DD records superseded structure
```

Do not require every redesign to affect every metric.

The design claim determines the evidence required.

A transformation that introduces a regression can still be correct. Make the
trade-off explicit instead of hiding it inside an aggregate score.

For example:

```text
ITE: 1.0 → 0.0
IKL:   3 → 5
```

may be justified when eliminating dangerous transitions is worth two additional
caller-visible concepts.

The comparison should explain whether the same benefit could be obtained with
less interface burden.

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

## Validate during the transformation

Validation protects the design claim; it does not independently define quality.

Use [`METRICS.md`](METRICS.md) for the comparison and [`TOOLING.md`](TOOLING.md)
for mechanical evidence.

Check, as appropriate:

- behaviour through the intended seam
- dependency direction
- visibility
- sibling coupling
- vertical leakage
- caller simplification
- coverage of moved behaviour
- mutation strength for important policy
- public API compatibility
- obsolete files and dependencies

When a deeper interface supersedes shallow ones, follow `codebase-design`'s
replace-don't-layer testing rule.

Retain lower-level tests only when the lower-level module has an independent
behavioural contract worth preserving.

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

## Recurse

After each meaningful transformation:

### Downward

Inspect whether the new implementation still contains another coherent concept
whose knowledge deserves containment.

### Upward

Reconsider the parent now that lower-level knowledge has disappeared.

The parent may now:

- collapse operations
- expose a smaller semantic interface
- consolidate with a sibling
- shed dependencies
- reveal another concept
- become unnecessary

Recursive deepening moves in both directions.

## Stop conditions

Stop deepening a region when:

- further decomposition reveals no independently meaningful knowledge
- further consolidation would merge responsibilities that should evolve
  independently
- another seam would expose as much knowledge as it hides
- another type would add representation without semantic leverage
- a trait or generic abstraction has no demonstrated need
- a child abstraction does not simplify its caller or protect an invariant
- remaining complexity is intrinsic to the responsibility that owns it
- behaviour is tested through meaningful seams
- lower-level implementation knowledge no longer leaks upward materially

Mechanical extractability is not a reason to continue.

## Completion criterion

A candidate is resolved when the resulting design explains:

- why each responsibility belongs where it does
- what each seam requires callers to know
- what knowledge it hides
- what no longer leaks upward
- how its parent becomes simpler or more coherent
- what was consolidated, replaced, relocated, or removed
- what deletion dividend was realized or intentionally deferred
- what Rust representation is justified
- what before/after evidence supports the result
- what trade-offs remain
