## What it measures

Cognitive complexity (Campbell / SonarSource, 2018) refines
[Cyclomatic](Cyclomatic.md) with one idea: **structures that break the linear
reading flow cost more the deeper they nest**.

- **+1** for each control-flow break (`if`, `match` arm, loop, `&&`/`||` sequence,
  `break`/`continue` to a label).
- **+1 extra per level of nesting** the structure sits inside. An `if` inside a
  `for` inside a `match` costs far more than three flat `if`s.
- A flat sequence of branches barely accrues; a deeply pyramided one explodes.

Code Ranker sums this over every function in the file. Where cyclomatic counts
*paths*, cognitive counts *reading effort* — the same branch count nested deeply
scores much higher.

## Why it matters

Two files can share the same cyclomatic total yet differ wildly in cognitive load:
ten flat `if`s vs. one `if` nested ten deep. The nested one is the maintenance
hazard — each level forces the reader to hold another condition in their head.
Cognitive complexity is the metric that tells the difference, so it is the better
signal for "this function is genuinely *hard*", not merely *branchy*.

## In Rust

High cognitive complexity shows up as:

- Pyramids of `if let Some(x) = … { if let Some(y) = … { … } }` — flatten with
  `let … else`, `?`, or a single `match (a, b)`.
- `match` arms that each contain another `match` or a loop with inner branches.
- Deeply indented loop bodies; extract the body into a named function.
- Long `&&`/`||` conditions mixed with nesting.

## Reducing it

Cognitive complexity responds to **flattening**, more than to raw branch removal:

- **Early returns / guard clauses.** `let … else { return … }` and `?` peel edge
  cases off the top so the main logic un-indents.
- **Invert and return early** instead of wrapping the rest of the function in an
  `if`.
- **Extract a nested block into its own function** — nesting resets to zero inside
  the new function, so the cost genuinely drops (not just moves).
- **Replace nested matching with a single `match` on a tuple** or with data-driven
  dispatch.

For the **file-level** number, the same two levers as cyclomatic apply: simplify
the hot function, or split a large file into cohesive sibling modules. The split
mechanics — and the **dependency-cycle trap** to avoid when extracting (extract
leaf helpers; move shared data down into a leaf both depend on, never reference
the parent back) — are written up once in
[Cyclomatic § The cycle trap](Cyclomatic.md). Read that before splitting; it
applies verbatim here.
<!-- doc:base "A workflow" -->
<!-- doc:base "Related principles" -->
<!-- doc:base "References" -->
