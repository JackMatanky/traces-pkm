# Metrics

Use architectural metrics to make comparisons explicit and repeatable.

Metrics support design reasoning; they do not define good design.

Prefer before/after comparisons over universal targets. Use a metric only when
its semantics apply to the candidate.

Never combine these metrics into an aggregate architecture score.

## Measurement discipline

For a concrete design claim, choose only gauges that could clarify the choice.
Keep the underlying caller facts, paths, or state sets beside each count. A
metric supports the semantic explanation; it does not define design quality. If
two reviewers cannot agree on its counting basis, compare the underlying
evidence instead of presenting a precise-looking number.

```text
responsibility and intended seam:
claim and current evidence:
selected gauge and counting basis (if useful):
frozen scenario (if applicable):
proposed effect (prediction until implemented):
observed effect (only after implementation):
trade-offs and uncertainty:
```

Freeze representative scenarios before comparing alternatives. Record available
current baselines. A review may estimate effects but labels them predictions;
only an implemented redesign yields observed after-state values. For an
implementation, recompute applicable gauges for equivalent callers under the
same counting basis and scenario. Account for new or removed callers separately,
then explain material regressions.

Use `N/A` for an inapplicable gauge or a zero denominator, `not measured` for
missing evidence, and `0` only for an observed zero. Counts refer to semantic or
architectural units unless specified otherwise. Source lines, file counts,
module counts, function counts, and similar size properties are indicators, not
architectural equations.

## Knowledge atoms

A **knowledge atom** is one independently necessary fact a legitimate caller
must know to use a seam correctly.

Classify atoms as:

- `type` — domain concepts or representations the caller must understand
- `invariant` — preconditions or validity rules the caller must preserve
- `order` — ordering, lifecycle, or transition requirements
- `error` — failure distinctions on which the caller must act
- `config` — configuration or environmental requirements
- `ownership` — ownership, borrowing, lifetime, concurrency, or aliasing
  constraints
- `performance` — performance characteristics required for correct use
- `leak` — lower-level implementation concepts exposed across the seam

Two facts are separate atoms when a caller can satisfy one while violating or
remaining ignorant of the other.

Assign each atom to one category and count it once per seam, not once per
caller. Classify an exposed implementation representation as `leak` rather than
also counting it as `type`; retain its name in the atom list.

When Rust makes an invariant impossible to violate, stop counting that invariant
atom. The caller-visible type or state concept may still count as a `type` atom
if callers must understand it.

Keep the atom list beside the count so disagreements remain inspectable.

## Interface Knowledge Load — `IKL`

For seam `S`, record the knowledge vector:

```text
IKL(S) =
  (K_type,
   K_invariant,
   K_order,
   K_error,
   K_config,
   K_ownership,
   K_performance,
   K_leak)
```

The total load is:

```text
|IKL(S)| =
  K_type
+ K_invariant
+ K_order
+ K_error
+ K_config
+ K_ownership
+ K_performance
+ K_leak
```

Preserve the vector when comparing designs. Equal totals can represent very
different interfaces.

Prefer lower caller knowledge for equivalent capability, while retaining
semantic distinctions callers genuinely need.

## Knowledge Compression — `KC`

For child `C` and parent `P`, isolate the knowledge in `P` concerning the
responsibility being moved behind `C`.

```text
KC(C → P) = 1 − K_after / K_before
```

where:

- `K_before` = relevant parent knowledge before the child seam
- `K_after` = the same category of parent knowledge after it

Interpretation:

```text
KC > 0   parent knowledge decreased
KC = 0   no knowledge compression
KC < 0   parent burden increased
```

Use `KC` only when `K_before > 0` and both counts cover the same responsibility.
When `K_before = 0`, `KC` is `N/A`; the absolute `PS` below and the underlying
parent-knowledge list still show any new burden.

### Parent Simplification — `PS`

Keep the absolute reduction as well:

```text
PS(C → P) = K_before − K_after
```

`KC` captures proportional compression.

`PS` is a signed net change: positive means fewer parent knowledge atoms, zero
means unchanged burden, and negative means new parent burden. Preserve the
before/after lists even if the counts cancel.

## Cross-Seam Leakage — `L`

For seam `S`:

```text
L(S) =
  number of lower-level concepts unnecessarily exposed across S
```

Examples include:

- storage types
- serialization representations
- hashing details
- descendant state types
- child-specific errors
- implementation ordering rules
- lower-level configuration

Record the leaked concepts beside the count.

Compare:

```text
ΔL = L_after − L_before
```

Interpretation:

```text
ΔL < 0   leakage decreased
ΔL = 0   no change
ΔL > 0   leakage increased
```

Do not count a concept as leakage when it is legitimately part of the seam's
semantics.

## Seam Bypass Ratio — `BR`

For intended seam `S`, freeze its external caller cohort, entry points, module
boundary, and edge unit before measurement. Count each distinct external
caller-to-item access path once, recording resolved target and any re-export. An
access through an intended re-export is not a bypass simply because its target
lives in a descendant. Define:

- `E_bypass`: recorded paths entering descendants without an intended entry
  point
- `E_in`: all recorded paths entering the subtree, including intended paths

Use the same edge unit and equivalent caller cohort after redesign; account for
new or removed callers separately. When `E_in > 0`:

```text
BR(S) = E_bypass / E_in
```

Interpretation:

```text
BR = 0   all external access respects the intended seam
BR > 0   some external access bypasses it
```

Define the intended entry points before calculating `BR`. Do not redraw the seam
after seeing the result.

When `E_in = 0`, `BR` is `N/A`; report that the seam has no incoming external
edges instead of treating it as a perfect boundary.

A bypass may be legitimate; if so, the intended seam description was incomplete
or the relationship deserves its own seam.

## Change Propagation Distance — `PD`

For representative change scenario `q`:

```text
PD(q) =
  number of intended architectural seams crossed
  by the minimal correct change
```

Freeze `q` before comparing designs.

Possible scenarios include:

- replace a hash implementation
- change freshness semantics
- change persistence representation
- add a domain property
- add a query capability
- change serialization format

Lower `PD` is generally preferable for implementation-level changes.

A semantic change that legitimately changes a higher-level contract is expected
to cross that contract.

Do not count files touched. Count architectural seams crossed.

## Deletion Dividend — `DD`

A redesign should account for structure made obsolete by its new ownership.

Record the vector:

```text
DD = (S, R, C, D)
```

where:

- `S` = superseded seams or abstraction layers
- `R` = redundant semantic representations
- `C` = duplicated coordination or policy sites
- `D` = dependencies required only by superseded design

Keep `DD` as a vector; do not sum or weight its dimensions. For a proposal,
record anticipated deletions as predictions. For implemented code, count only
removed or actually superseded structure, not planned future cleanup.

A zero deletion dividend does not invalidate genuinely new capability, but a
deepening redesign that only adds structure deserves scrutiny.

## Seam Variation — `V`

For seam `S`:

```text
V(S) = number of meaningful adapters satisfying the seam
```

Interpret this using `codebase-design`'s seam discipline.

One implementation may indicate hypothetical variation. Multiple meaningful
implementations demonstrate actual variation.

Do not create artificial adapters to increase `V`.

A test adapter counts only when the underlying dependency category justifies a
real substitutable seam.

## Visibility Reachability — `VE`

For Rust item `x`, compare legitimate callers with actual reachability through
declared visibility and re-exports in a named workspace and relevant
`cfg`/feature configuration. Include `pub(in crate::ancestor)`, private
descendants, `pub(super)`, and `pub(crate)`. A `pub` item re-exported from a
private module can still be publicly reachable; restricted items cannot be
publicly re-exported simply to widen visibility.

`VE(x)` is the set of unnecessarily reachable modules within that bounded scope,
plus an external-public-exposure category when the item is reachable outside the
workspace. Record paths and callers, rather than a numeric rank. If legitimate
callers cannot reach the item, record a separate interface defect. An intended
public contract can justify exposure beyond known current call sites.

## Generic Propagation Depth — `GPD`

For generic parameter, trait abstraction, or adapter type `g`:

```text
GPD(g) =
  number of architectural seams through which g passes
  without semantic use
```

A level semantically uses `g` when its own behaviour, invariant, dispatch, or
caller-facing contract depends on that variation. Merely carrying `g` in a field
or signature to pass it to a child does not establish semantic use.

High `GPD` is evidence that implementation variation may be leaking upward.

It is not automatically wrong; record the reason when the propagation is
intentional.

## Invalid-State Ratio — `ISR`

Use only when the relevant state space is finite and exactly enumerable.

Let:

- `R` = number of representable states
- `V` = number of semantically valid states

Then:

```text
ISR = (R − V) / R   when R > 0
```

Interpretation:

```text
ISR = 0   every representable state is semantically valid
ISR > 0   the representation permits invalid states
```

Use this when comparing:

- correlated booleans
- `Option` combinations
- enums
- validated types
- similar finite state representations

When `R = 0`, `ISR` is `N/A`. Do not estimate `R` when the state space cannot be
counted meaningfully.

## Illegal-Transition Expressibility — `ITE`

Use when the design contains a finite semantic state machine. Fix the universe
of relevant source states and semantic operations or events before comparison;
each state-operation pair is one possible transition. Compare equivalent
semantic operations before and after, and assess new capability separately.

Let:

- `T_illegal` = number of pairs prohibited by the domain semantics
- `T_expressible` = those illegal pairs callable through the interface

When `T_illegal > 0`:

```text
ITE = T_expressible / T_illegal
```

`ITE = 0` means no illegal pair can be expressed; a positive value means some
can. When `T_illegal = 0`, `ITE` is `N/A`; report that the chosen universe has
no illegal transitions, not a zero rate. Compare gains in `ITE` against `IKL`:
eliminating illegal transitions does not automatically justify a substantially
harder caller interface.

## Test Reach-Through — `TR`

Define the population as tests intended to verify `M`'s responsibility,
including integration tests. Count each test once. The numerator is the subset
that depends on descendants past `M`'s intended seam:

```text
TR(M) = reach-through tests for M / tests verifying M
```

Independent child-module tests are outside both numerator and denominator.
When no tests verify `M`, `TR` is `N/A`; report the verification gap.
Use `TR` to detect tests coupled to internal representation, not to drive
the ratio blindly to zero.

## Seam Mutation Adequacy — `MA`

Use scoped mutation testing when the design claim needs verification strength.
Define the policy owned by `M`, including relevant descendants behind its
seam, and fix the semantic fault classes being probed. Count only compiled,
executed, behavior-changing mutants in the denominator. The numerator is the
subset killed by tests through `M`'s seam:

```text
MA(M) = behavior-changing mutants killed through M's seam
        / behavior-changing mutants tested in M's responsibility
```

Record equivalent, uncompilable, skipped, and timed-out mutants separately
with reasons. Without a mutation run, `MA` is `not measured`; if a run
yields no qualifying mutants, it is `N/A`, not zero. If a redesign changes
mutation sites, compare equivalent semantic fault classes or mark numeric
before/after comparison unavailable. Mutation adequacy measures verification
strength, not architectural depth.

## Comparative acceptance gate

Required behaviour and correctness come first. Compare only gauges selected for
the claim. For a review, distinguish current measurements from predicted effects
and state what would verify the prediction. For an implementation, compare
observed values against the frozen baseline and scenario. Favor a design when
its required new capability or relevant improvement is evidenced, material
regressions have explicit semantic or correctness trade-offs, and any
superseded structure is accounted for. No proxy count alone establishes
architectural success.

Record a material regression with its before and after evidence, the benefit
requiring it, and whether an alternative achieves that benefit at lower cost. A
justified trade-off is allowed; an unexplained regression is not. Never sum
improvements and regressions into an aggregate score.

## Natural targets

Use absolute targets only where semantics make them meaningful.

Examples:

```text
BR  = 0   no unintended seam bypass
ISR = 0   no invalid representable finite states
ITE = 0   no expressible illegal transitions
VE  = ∅   no unnecessarily reachable modules
```

Most metrics are comparative rather than target-seeking.

## Avoid metric gaming

A metric becomes harmful when the implementation is changed to improve the
number rather than the responsibility it represents.

Examples:

```text
high IKL
→ replacing several semantic operations with one opaque "execute" call
   does not create depth if callers now need a complicated request protocol

high PD
→ merging unrelated responsibilities into one module does not create locality

high ISR
→ encoding every possible state in elaborate typestate is not automatically
   better if IKL and GPD explode

high BR
→ hiding descendants behind a forwarding facade does not repair a shallow seam

low DD
→ deleting useful concepts merely to increase the deletion dividend is not
   deepening
```

The semantic explanation remains authoritative.

## Completion criterion

Evidence is ready for a design decision when every used gauge has its raw
evidence and counting basis, scenarios were frozen before comparison, and `N/A`,
`not measured`, and observed zero are distinguished. An implementation uses the
same interpretation before and after and explains material regressions. A review
labels proposed effects as predictions and records what remains to be measured.
Neither optimizes a vanity count or hides a trade-off in an aggregate score.
