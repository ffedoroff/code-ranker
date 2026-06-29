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

**3. Do NOT spread one struct's `impl` across `mod` submodules — in Rust this
*raises* HK.** This is the single most common wrong move. Splitting
`impl Service` into `service/create.rs`, `service/query.rs`, … makes every
submodule write `use super::{Service, …}` — and each of those is a **new
`fan_in` edge** to the module that *defines* the struct. Because
`HK = sloc × (fan_in × fan_out)²`, the squared `fan_in` bump typically outweighs
the `sloc` you moved out, so the defining module's HK goes **up**. A struct's HK
is driven by who **calls** it and what it **depends on** — never by how many
files its `impl` is scattered across. The same applies to hoisting a type away
from its `impl` (it then needs wider `pub` visibility to compile, *adding*
edges). Shaving the HK *number* this way separates no responsibility: it is
metric-gaming, see "When a hub is legitimate" below.

**The moves that actually lower a Rust hub's HK:**
- **Move a leaf out so a caller stops importing the hub (`fan_in` ↓).** If a
  dependant reaches the hub only for one pure helper / DTO / `const`, relocate
  that item to its own module; the dependant now imports *that* module and its
  edge to the hub disappears. (A pure data type with no deps has `fan_out = 0`,
  so its own HK is ~0 — the safest win.)
- **Extract a genuinely separate responsibility into its own type (`fan_out` ↓).**
  Not the same struct's `impl` in another file — a *new* struct with its *own*
  fields and dependencies (e.g. pull an I/O / streaming concern out of a
  coordinator into its own worker type). The unrelated imports leave the hub with
  that responsibility, and the new type carries its own, lower HK.
- **Fold several same-concern edges behind one facade (`fan_out` ↓ — usually the
  biggest win for a service/orchestrator hub).** A service hub often spends most
  of its `fan_out` on a *single* concern that is split across several edges.
  The classic case is persistence: the hub holds the database handle/pool, opens
  connections and runs transactions itself (e.g. `sqlx` / a `DBRunner`-style
  trait / the DB-driver crate), **and** also imports several repositories.
  **Trace those edges at depth 2:** the repos already sit on the DB driver, so
  the hub's *direct* driver edge is redundant with what its repos encapsulate.
  Move the DB handle + the repositories + the connection/transaction management
  into one repository / unit-of-work facade that exposes intent-level operations
  (`store.get_x(…)`, `store.commit_y(…)`), keeping connections and transactions
  *inside* it. The hub then depends on that **one** facade instead of the driver
  crate plus each repo — N edges collapse to 1. This is the [DIP](DIP.md) remedy
  and it genuinely *dissolves* `fan_out` (the orchestrator stops knowing about the
  database at all), because connection/transaction lifecycle belongs in the
  persistence layer, not in the coordinator. The same shape applies to any concern
  that arrives as a cluster of edges (an HTTP/client stack, a serialization stack):
  one cohesive facade per concern, the hub depends on the facade.

**Predict the edge change before you touch code, then verify it (Step 5).** Ask:
does this make a dependant stop importing the hub, or the hub stop importing a
collaborator? If neither — if the new module still references the hub or the hub
still references it — you are *adding* an edge and the square will punish you.
Re-measure with a before/after `--focus-path` scorecard and revert if the hub's
HK did not drop.
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
