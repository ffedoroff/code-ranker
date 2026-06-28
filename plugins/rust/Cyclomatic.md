# Cyclomatic — McCabe Complexity (in Rust)

**TL;DR**: Cyclomatic complexity counts the independent paths through code —
`1 + one per decision point` (`if`, `else if`, each `match`/`if let` arm, every
loop, every `&&`/`||`). Code Ranker reports it **per file, summed across every
function**, so a file's `cyclomatic` is its total branching burden. A high number
means the file is doing a lot of deciding — split the branching out or simplify it.

## What it measures

McCabe's 1976 metric is the number of linearly independent paths through a piece
of code. Operationally, for one function:

- Start at **1**.
- **+1** for each `if` / `else if`, each `match` / `if let` arm that branches,
  each `while` / `for` / `loop`, and each `&&` / `||` in a condition (each is a
  new way for control flow to fork).

Code Ranker sums this over **every function, closure, and method in the file**
(`cyclomatic = spaces + branches`). So the file-level number grows two ways: more
functions, and more branching per function. It is an **additive size-like metric**
— unlike `hk`, there is no squared coupling term. That distinction matters for how
you reduce it (see below).

For a readability-weighted view that punishes *nesting* rather than raw path
count, see [Cognitive](Cognitive.md).
<!-- doc:base "Why it matters" -->

## In Rust

Branch-heavy Rust usually shows up as:

- Long `match` arms over an enum, especially nested `match` inside `match`.
- `if let … else if let …` ladders that could be one `match`.
- Parsing / dispatch / config-resolution code: one function that maps many input
  shapes to many outputs.
- A file that has grown to hold a whole subsystem's worth of free functions.

## Reducing it

There are **two** levers. Pick by reading the per-function breakdown first
(Step 1 below) — they are not interchangeable.

### Lever A — simplify the branching (lower the *true* complexity)

When one function dominates the file's total, cut its decision points:

- **Replace an `if/else-if` ladder or a flat `match` with data-driven dispatch** —
  a lookup table / `BTreeMap` / slice of `(pattern, handler)`. The branches become
  data, not control flow.
- **Early-return the error/edge cases** (`let … else`, `?`, guard clauses) so the
  happy path is flat instead of nested.
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

```rust
mod eval;                       // new sibling file: registry/eval.rs
pub(crate) use eval::register_math;   // keep external paths stable
use eval::{reduce, exec_f64};         // internal call sites unchanged
```

## The cycle trap (read this before you split)

Splitting for **Lever B** has one failure mode that the gate will catch as a
**dependency cycle** ([ADP](ADP.md)): if the functions you
move still reference items *defined in the parent* — a type, a `static`, a helper —
while the parent imports the moved functions, you have created a **mutual
parent ↔ child dependency**. The file-level cyclomatic drops, but a new violation
appears:

```
mutual cycle between …/registry.rs ↔ …/registry/eval.rs
```

You traded a complexity breach for a coupling breach. Two rules keep a split
acyclic:

- **Extract leaf helpers first.** A function that depends only on its arguments
  and external crates moves with no back-edge. These are free to relocate.
- **Move shared data *down*, never reference it *up*.** If the moved functions
  need a type / `static` / helper the parent *also* uses, relocate that shared
  item into a **dependency-free leaf module** that both the parent and the new
  module import. Then dependencies flow one way (`parent → leaf`, `child → leaf`) —
  no cycle. Do **not** leave the child reaching back via `super::Thing` while the
  parent reaches into the child.

Concretely, the layering that works:

```
model.rs   (leaf)   — the shared types/data. Imports only external crates.
eval.rs            — the moved helpers.   `use super::model::{…}`  → edge to leaf
registry.rs        — the orchestrator.    `use {model::*, eval::*}` → edges to both
```

The resolver follows `pub use` re-exports to the **defining** file, so you may
re-export the leaf's items from the parent for call-site stability without
recreating an edge *to* the parent — the edge lands on the leaf where the item is
defined. (This is the same mechanism that lets a `config/` facade keep short paths
without becoming a hub.)

If, after a split, you see a `mutual cycle … ↔ …`: the extraction *relocated* a
dependency instead of cutting it. Find what the child still pulls from the parent
(`grep 'super::' child.rs`) and move that shared item to a leaf.
<!-- doc:base "A workflow: bringing one file under the budget" -->
<!-- doc:base "Related principles" -->
<!-- doc:base "References" -->
