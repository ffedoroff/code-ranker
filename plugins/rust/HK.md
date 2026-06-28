<!-- doc:base "What it measures" -->
<!-- doc:base "Why it matters" -->

## In Rust

Fan-in and fan-out are counted over real code dependencies (`use` paths,
qualified paths, derives) — the flow edges, not structural `mod` / `pub use`
re-export relationships. **One thing inflates `fan_in` artificially:** a
`pub(in <ancestor>)` restricted-visibility path is recorded as a fan-in edge up
to that ancestor even when nothing there `use`s the item (the same modelling as
[ADP](ADP.md)). A Rust module scores high HK when it is both widely imported and
imports widely:

- A `lib.rs` or `mod.rs` facade that re-exports and also orchestrates.
- A `types.rs` / `model.rs` that every layer imports *and* that itself pulls
  in serialization, validation, and persistence concerns.
- A `utils.rs` junk drawer that accumulates helpers used everywhere.

### Remedies, in order

**1. Narrow artificial fan-in first.** Before anything structural, check whether
the hub's in-edges are real `use` imports or just over-broad
`pub(in <ancestor>)` visibility. If artificial, narrow the visibility
(`pub(super)` if only the parent uses the item, `pub(crate)` if a sibling subtree
does): the edge dissolves, `fan_in` drops, and HK falls — a one-line change, no
split. (`HK = sloc × (fan_in × fan_out)²`, so dropping `fan_in` from 5 to 2 alone
is a ~6× cut.)

**2. The highest-value fix: split a multi-role hub by responsibility.** When the
in-edges are *real*, the most valuable thing you can do is separate a component
that has accreted **2–3 distinct roles** into **one module per role**. A file
that does, say, *field mapping* **and** *platform-API orchestration* **and**
*status handling* is three components wearing one name; giving each its own
module is what genuinely cuts HK, because each role then couples only to its own
dependencies — **both `fan_in` and `fan_out` drop for real**, and each piece
becomes independently testable and changeable. Find the seams (distinct
responsibilities, distinct dependency sets) and cut along them.

**3. Do not shave `sloc` by mechanical splitting.** Moving a type declaration
away from its `impl`, or hoisting a trait into a sibling file, splits **one
cohesive role** across two files. It lowers the HK *number* (less `sloc`) without
separating any responsibility — and can even *raise* coupling by widening
visibility to make the move compile. That is metric-gaming, not decoupling; see
"When a hub is legitimate" below. The test: if your split does not leave each
new module owning a **distinct role**, it is not a real HK fix.
<!-- doc:base "Reducing it" -->

## When a hub is legitimate (accept, don't game)

Not every high-HK file should be split. A few are *irreducible by design* —
their coupling **is** the architecture, not an accident:

- **A core contract / trait** that every implementor depends on. Its `fan_in`
  grows with each implementation by definition, and it references the types its
  own signatures use (`fan_out`). The number is the cost of having one contract
  instead of many ad-hoc ones.
- **A top-level orchestrator** that wires every subsystem together. High
  `fan_out` is its whole job; pushing those dependencies elsewhere only moves
  the crossroads, it does not remove it.

Before accepting one, *prove* it is irreducible — apply the Step-4 test: would a
split **dissolve** coupling or merely **relocate** it? If every candidate
extraction either leaves `fan_in × fan_out` unchanged (you only shaved `sloc`)
or *raises* `fan_out` (you moved out a type the file's own signatures mention),
the hub is load-bearing and splitting it is metric-gaming, not decoupling.

When that holds, **accept it explicitly**: raise the `hk` threshold to sit just
above the hub, and record *why* right next to the value in config — name the
file, its role, and the factor that makes it irreducible. That turns a silent
suppression into a reviewed, documented decision, and keeps the gate meaningful
for the *next* file that crosses the line.

This is the exception, not the default. Raise the ceiling only for a NEW,
genuine hub you have proven irreducible; for everything else, prefer the split.
<!-- doc:base "How code-ranker surfaces it" -->

<!-- doc:base "A workflow: dissecting and splitting a high-HK file" -->

<!-- doc:base "Related principles" -->
<!-- doc:base "References" -->
