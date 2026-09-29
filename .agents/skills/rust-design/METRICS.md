# Metrics

Use architectural metrics to make comparisons explicit and repeatable.

Metrics support design reasoning; they do not define good design.

Prefer before/after comparisons over universal targets. Use a metric only when
its semantics apply to the candidate.

Never combine these metrics into an aggregate architecture score.

## Measurement discipline

Before comparing a redesign:

1. define the responsibility and intended seam
2. select only metrics that test the design claim
3. define the counting basis for each selected metric
4. record the baseline
5. freeze any scenario used for comparison
6. perform or model the redesign
7. recompute the same metrics under the same interpretation
8. explain every material regression

Mark an inapplicable metric as `N/A`, not `0`.

Counts refer to semantic or architectural units unless explicitly stated
otherwise.

Source lines, file counts, module counts, function counts, and similar size
properties are indicators only. They never enter an architectural equation.

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

Count a fact once per seam, not once per caller.

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

Use `KC` only when `K_before` and `K_after` count the same responsibility.

### Parent Simplification — `PS`

Keep the absolute reduction as well:

```text
PS(C → P) = K_before − K_after
```

`KC` captures proportional compression.

`PS` captures how many knowledge atoms actually disappeared from the parent.

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

For intended seam `S`, define:

- `E_bypass` — external dependency edges entering descendants without crossing
  an intended entry point
- `E_in` — all external dependency edges entering the module subtree

Then, when `E_in > 0`:

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

Keep `DD` as a vector.

Do not sum or weight its dimensions.

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

## Visibility Excess — `VE`

For Rust item `x`, use these architectural visibility levels:

```text
private      0
pub(super)   1
pub(crate)   2
pub          3
```

Let:

- `V_actual(x)` = declared visibility level
- `V_needed(x)` = narrowest level containing all legitimate callers

Then:

```text
VE(x) = max(0, V_actual(x) − V_needed(x))
```

Interpretation:

```text
VE = 0   visibility is no broader than needed
VE > 0   investigate why the seam is exposed this widely
```

This is evidence about seam exposure, not a mandate to minimize visibility
blindly.

## Generic Propagation Depth — `GPD`

For generic parameter, trait abstraction, or adapter type `g`:

```text
GPD(g) =
  number of architectural seams through which g passes
  without semantic use
```

A level semantically uses `g` when its own behaviour, storage, dispatch, or
contract depends on that variation.

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
ISR = (R − V) / R
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

Do not estimate `R` when the state space cannot be counted meaningfully.

## Illegal-Transition Expressibility — `ITE`

Use when the design contains a finite semantic state machine.

Let:

- `T_illegal` = number of semantically illegal transitions
- `T_expressible` = illegal transitions callable through the interface

When `T_illegal > 0`:

```text
ITE = T_expressible / T_illegal
```

Interpretation:

```text
ITE = 0   the interface cannot express an illegal transition
ITE > 0   some illegal transitions remain callable
```

Compare improvements in `ITE` against `IKL`.

Eliminating illegal transitions does not automatically justify a substantially
harder caller interface.

## Test Reach-Through — `TR`

For module `M`:

```text
TR(M) =
  tests for M that depend on descendants past M's intended seam
  ─────────────────────────────────────────────────────────────
                    tests exercising M
```

Independent child-module tests are not reach-through.

Use `TR` to detect parent-level tests coupled to internal representation or
implementation structure.

The goal is appropriate test seams, not blindly driving `TR` to zero.

## Seam Mutation Adequacy — `MA`

Where scoped mutation testing is useful:

```text
MA(M) =
  behavior-changing mutants killed through M's seam
  ────────────────────────────────────────────────
         behavior-changing mutants tested in M
```

Use `MA` to evaluate whether interface-level tests detect incorrect behaviour
hidden by the module.

Mutation adequacy measures verification strength, not architectural depth.

## Comparative acceptance gate

Required behaviour and correctness come first.

Then compare only the metrics applicable to the design claim.

Prefer a redesign when:

1. at least one relevant architectural dimension materially improves
2. no relevant dimension materially regresses without an explicit trade-off
3. the claimed improvement concerns responsibility, knowledge containment,
   correctness, or locality rather than a proxy count
4. superseded structure has been accounted for

Do not sum improvements and regressions into a weighted score.

When a metric regresses, record:

```text
Regression:
  <metric>: <before> → <after>

Benefit:
  <semantic or correctness gain requiring the regression>

Alternatives:
  <whether the same benefit could be obtained without the regression>
```

A justified trade-off is allowed.

An unexplained regression is not.

## Natural targets

Use absolute targets only where semantics make them meaningful.

Examples:

```text
BR  = 0   no unintended seam bypass
ISR = 0   no invalid representable finite states
ITE = 0   no expressible illegal transitions
VE  = 0   no unnecessarily broad visibility
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

Measurement is complete when:

- every metric used in a decision has a stated counting basis
- before and after use the same interpretation
- scenarios are frozen before comparison
- inapplicable metrics remain `N/A`
- atom or concept lists accompany ambiguous counts
- no size or count vanity measure has become a design objective
- no aggregate score hides a trade-off
- every material regression is explained
