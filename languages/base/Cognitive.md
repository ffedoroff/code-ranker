# Cognitive — Cognitive Complexity

**TL;DR**: Cognitive complexity scores how hard code is to *read*, not how many
paths it has. It penalises **nesting**: a branch three levels deep costs more than
a branch at the top. Code Ranker reports it **per file, summed across functions**.
A high number means deeply nested, hard-to-follow control flow — flatten it.

## What it measures

Cognitive complexity (Campbell / SonarSource, 2018) refines
[Cyclomatic](Cyclomatic.md) with one idea: **structures that break the linear
reading flow cost more the deeper they nest**.

- **+1** for each control-flow break (a conditional, a branch arm, a loop, a
  `&&`/`||` sequence, a `break`/`continue` to a label).
- **+1 extra per level of nesting** the structure sits inside. A conditional
  inside a loop inside another branch costs far more than three flat conditionals.
- A flat sequence of branches barely accrues; a deeply pyramided one explodes.

Code Ranker sums this over every function in the file. Where cyclomatic counts
*paths*, cognitive counts *reading effort* — the same branch count nested deeply
scores much higher.

## Why it matters

Two files can share the same cyclomatic total yet differ wildly in cognitive load:
ten flat conditionals vs. one conditional nested ten deep. The nested one is the
maintenance hazard — each level forces the reader to hold another condition in
their head. Cognitive complexity is the metric that tells the difference, so it is
the better signal for "this function is genuinely *hard*", not merely *branchy*.

## What high complexity looks like

High cognitive complexity shows up as:

- Pyramids of nested conditional unwrapping — `if value is present { if other is
  present { … } }` — flatten with early exits, guard clauses, or a single combined
  conditional.
- Branch arms that each contain another branch or a loop with inner branches.
- Deeply indented loop bodies; extract the body into a named function.
- Long `&&`/`||` conditions mixed with nesting.

## Reducing it

Cognitive complexity responds to **flattening**, more than to raw branch removal:

- **Early returns / guard clauses.** Peeling edge cases off the top so the main
  logic un-indents.
- **Invert and return early** instead of wrapping the rest of the function in a
  conditional.
- **Extract a nested block into its own function** — nesting resets to zero inside
  the new function, so the cost genuinely drops (not just moves).
- **Replace nested matching with a single combined branch on a tuple** or with
  data-driven dispatch.

For the **file-level** number, the same two levers as cyclomatic apply: simplify
the hot function, or split a large file into cohesive sibling modules. The split
mechanics — and the **dependency-cycle trap** to avoid when extracting (extract
leaf helpers; move shared data down into a leaf both depend on, never reference
the parent back) — are written up once in
[Cyclomatic § The cycle trap](Cyclomatic.md). Read that before splitting; it
applies verbatim here.

## A workflow

```bash
# Flag files over the cognitive budget, worst-first:
code-ranker check <path/to/project> --threshold file.cognitive=110 --top 1

# Triage worst-first by cognitive — ranked offenders, no snapshot to parse:
code-ranker report <path/to/project> --output.scorecard --focus-rule cognitive

# After flattening / splitting — confirm it dropped and no new cycle appeared:
code-ranker check <path/to/project> --threshold file.cognitive=110
```

To find the deepest offender within a file, turn on the function level
(`[levels] functions = true`) and open the viewer (`report --output.html`) — each
function shows its own cognitive score in the node popup.

Prefer flattening (early returns, extracted blocks) over relocation when one
function dominates — it lowers the *true* reading cost. Run the test suite after.

## Related principles

- [Cyclomatic](Cyclomatic.md) — the path-count twin; the split/cycle workflow
  lives there. Reduce both together.
- [KISS](KISS.md) — the qualitative root.
- [SRP](SRP.md) — a deeply nested function often conflates several jobs.
- [ADP](ADP.md) — the cycle trap a careless split triggers.

## References

1. Campbell, G. A. "Cognitive Complexity — A new way of measuring understandability".
   SonarSource, 2018.
2. McCabe, T. J. "A Complexity Measure". *IEEE TSE*, SE-2(4), 1976.
