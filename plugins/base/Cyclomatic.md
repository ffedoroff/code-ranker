# Cyclomatic — McCabe Complexity

**TL;DR**: Cyclomatic complexity counts the independent paths through code —
`1 + one per decision point` (each conditional, each branch arm, every loop, every
`&&`/`||`). Code Ranker reports it **per file, summed across every function**, so
a file's `cyclomatic` is its total branching burden. A high number means the file
is doing a lot of deciding — split the branching out or simplify it.

## What it measures

McCabe's 1976 metric is the number of linearly independent paths through a piece
of code. Operationally, for one function:

- Start at **1**.
- **+1** for each conditional, each branch arm that forks, each loop, and each
  `&&` / `||` in a condition (each is a new way for control flow to fork).

Code Ranker sums this over **every function, closure, and method in the file**
(`cyclomatic = spaces + branches`). So the file-level number grows two ways: more
functions, and more branching per function. It is an **additive size-like metric**
— unlike `hk`, there is no squared coupling term. That distinction matters for how
you reduce it (see below).

For a readability-weighted view that punishes *nesting* rather than raw path
count, see [Cognitive](Cognitive.md).

## Why it matters

A file with high total cyclomatic complexity is expensive in two ways:

- **Per function**: a single function with many branches needs many test cases to
  cover, is hard to read top-to-bottom, and tends to hide edge cases in a rarely
  taken arm.
- **Per file**: a file that sums to a high number is carrying a lot of distinct
  decisions in one place — usually several responsibilities that could live apart.

The gate is a per-file budget, so it flags both "one monster function" and "fifty
small branchy functions in one file". The remedy differs (read the breakdown
first), but both are real signals.

## What high complexity looks like

Branch-heavy code usually shows up as:

- Long chains of branch arms over a set of cases, especially nested branches
  inside branches.
- Conditional ladders that could collapse into one multi-way branch.
- Parsing / dispatch / config-resolution code: one function that maps many input
  shapes to many outputs.
- A file that has grown to hold a whole subsystem's worth of free functions.

## Reducing it

There are **two** levers. Pick by reading the per-function breakdown first
(Step 1 below) — they are not interchangeable.

### Lever A — simplify the branching (lower the *true* complexity)

When one function dominates the file's total, cut its decision points:

- **Replace a conditional ladder or a flat multi-way branch with data-driven
  dispatch** — a lookup table / map / list of `(pattern, handler)` pairs. The
  branches become data, not control flow.
- **Early-return the error/edge cases** (guard clauses) so the happy path is flat
  instead of nested.
- **Collapse boolean chains** into a well-named predicate function.
- **Extract a cohesive block into a sub-function** — the branches still exist, but
  they are split across two readable units instead of one tangled one.

This is the *better* lever when the complexity is genuine: it makes the code
easier, not just rearranged.

### Lever B — split the file (relocate, the metric is additive)

When the file is just *large* — many independent functions, none individually
awful — move a cohesive group into a sibling submodule. Because the metric is a
per-file **sum**, each resulting file carries only its share, and the total
genuinely drops. This is legitimate decomposition, not gaming (contrast `hk`,
whose squared coupling term means relocation does not help — see
[HK](HK.md)).

Extract along a real seam: a group of functions that belong together (e.g. all the
import-resolution helpers, all the value-printing helpers), re-exported so call
sites stay unchanged:

```
# new sibling module: registry/eval
import module eval                       # the new sibling file
re-export eval.register_math             # keep external paths stable
import eval.{reduce, exec_f64}           # internal call sites unchanged
```

## The cycle trap (read this before you split)

Splitting for **Lever B** has one failure mode that the gate will catch as a
**dependency cycle** ([ADP](ADP.md)): if the functions you
move still reference items *defined in the parent* — a type, a shared global, a
helper — while the parent imports the moved functions, you have created a **mutual
parent ↔ child dependency**. The file-level cyclomatic drops, but a new violation
appears:

```
mutual cycle between …/registry ↔ …/registry/eval
```

You traded a complexity breach for a coupling breach. Two rules keep a split
acyclic:

- **Extract leaf helpers first.** A function that depends only on its arguments
  and external libraries moves with no back-edge. These are free to relocate.
- **Move shared data *down*, never reference it *up*.** If the moved functions
  need a type / shared global / helper the parent *also* uses, relocate that
  shared item into a **dependency-free leaf module** that both the parent and the
  new module import. Then dependencies flow one way (`parent → leaf`,
  `child → leaf`) — no cycle. Do **not** leave the child reaching back up to the
  parent while the parent reaches into the child.

Concretely, the layering that works:

```
model   (leaf)   — the shared types/data. Imports only external libraries.
eval             — the moved helpers.   imports from model         → edge to leaf
registry         — the orchestrator.    imports from model + eval  → edges to both
```

The resolver follows re-exports to the **defining** file, so you may re-export the
leaf's items from the parent for call-site stability without recreating an edge
*to* the parent — the edge lands on the leaf where the item is defined. (This is
the same mechanism that lets a facade module keep short paths without becoming a
hub.)

If, after a split, you see a `mutual cycle … ↔ …`: the extraction *relocated* a
dependency instead of cutting it. Find what the child still pulls from the parent
and move that shared item to a leaf.

## A workflow: bringing one file under the budget

### Step 1 — Measure, and read the per-function breakdown

```bash
# Gate on cyclomatic: flag every file over budget N, worst-first.
code-ranker check <path/to/project> --threshold file.cyclomatic=110 --top 1

# Triage worst-first by cyclomatic — ranked offenders, no snapshot to parse:
code-ranker report <path/to/project> --output.scorecard --focus cyclomatic
```

To read how a file's total splits across its functions (which picks the lever),
turn on the function level (`[levels] functions = true`) and open the viewer
(`report --output.html`) — each function shows its own cyclomatic in the node popup.

Decide the lever from the shape: **one function ≫ the rest → Lever A** (simplify
it); **many comparable functions → Lever B** (split the file).

### Step 2 — Apply the lever

Lever A: cut decision points in the hot function (table-dispatch, early returns).
Lever B: move a cohesive group to a sibling module, obeying the two cycle rules
above (leaf helpers, shared data down).

### Step 3 — Verify the number dropped *and* no cycle appeared

```bash
# The file is under budget AND no new mutual/chain cycle was introduced:
code-ranker check <path/to/project> --threshold file.cyclomatic=110
```

Read the result, don't assume it: a green cyclomatic line with a fresh
`mutual cycle` line means you relocated coupling — go back to the two rules. Run
the full test suite after either lever; Lever B is behaviour-preserving by
construction (pure relocation), so any test change is a mistake.

## Related principles

- [Cognitive](Cognitive.md) — the nesting-weighted twin; reduce them together.
- [KISS](KISS.md) — the qualitative root: fewer branches is fewer surprises.
- [SRP](SRP.md) — a file that sums to a high cyclomatic is usually doing several
  jobs; splitting by responsibility lowers it for free.
- [ADP](ADP.md) — the cycle trap a careless split triggers.
- [HK](HK.md) — contrast: there, relocation does *not* help (coupling is squared).

## References

1. McCabe, T. J. "A Complexity Measure". *IEEE Transactions on Software
   Engineering*, SE-2(4), 1976.
