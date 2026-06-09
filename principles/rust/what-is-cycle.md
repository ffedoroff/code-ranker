# What code-ranker counts as a cycle (and what it doesn't)

**TL;DR**: A cycle is a loop in the **flow** sub-graph. Code Ranker runs Kosaraju
SCC over edges whose kind is marked `flow`, keeps components of **2+ nodes**, and
drops any component that **spans more than one crate**. Today only `uses` is
flow, so `contains` / `reexports` / `super` edges are kept in the JSON but never
close a loop. The Rust sample (`crates/code-ranker-plugin-rust/sample`) is wired
to demonstrate every case — including loops that would only close *if*
`reexports` / `super` were made flow.

## The rule

Cycle detection lives in `crates/code-ranker-graph/src/cycles.rs`:

- An edge participates only if its kind is in the flow set, derived from
  `EdgeKindSpec.flow` (`cycles.rs:29`). Structural kinds are skipped.
- Strongly-connected components are found with Kosaraju (`cycles.rs:41`).
- A component counts as a cycle only when it has **≥ 2 members**
  (`cycles.rs:46`) — a self-loop (size 1) never counts, and file→self edges are
  dropped earlier in `finalize_graph`.
- A component that **spans multiple crates is discarded** (`cycles.rs:52`): Rust
  forbids circular crate dependencies, so such an SCC is not a real cycle.
- Survivors are classified by `classify_scc` purely by size: exactly 2 nodes →
  `mutual`, 3+ → `chain`. There are only these two kinds.

> **Removed: `test_embed`.** A third kind once tagged any SCC containing a
> test-named file. It was dropped: a test file only joins a flow SCC when
> production depends *back* on it (rare), and the `any-test-member → test_embed`
> rule was coarse — one test node in a large SCC re-labelled the whole real cycle
> as `test_embed`, which was off by default and so hid it. Test files are instead
> handled by the `[ignore] tests` filter when unwanted; a test file that genuinely
> sits in a cycle is now reported as a plain `mutual` / `chain` like any other.

Edges in one loop may mix kinds — what matters is that **every** edge in the loop
is flow.

## Edge kinds and flow

| Kind | Source form | `flow` | Counts toward cycles / fan-in / fan-out / HK / drawn on map |
|---|---|---|---|
| `uses` | `use a::Item;`, qualified path, derive | **yes** | yes |
| `contains` | `mod foo;` | no | no — module ownership, structure only |
| `reexports` | `pub use a::Item;` | no | no — facade; re-publishes another file's item |
| `super` | glob up: `use super::*`, `use crate::<anc>::*` | no | no — namespace pull from an ancestor |

### Named vs glob `super` — the dependency only the glob hides

This distinction matters and is implemented in
`crates/code-ranker-plugin-rust/src/module_graph.rs`: `is_super_glob` returns
`false` for any non-glob import (`module_graph.rs:593`), and the edge kind is
chosen as `Reexports` → `Super` → `Uses` in that order (`module_graph.rs:628`).
So:

- A **named** up-import — `use super::a::alpha` — is a real dependency on a
  concrete parent item → emitted as **`uses`** (flow). See `b.rs:5`: that line is
  the `b → a` edge that closes the only cycle the sample reports today (`a ↔ b`).
- A **glob** up-import — `use super::*` / `use crate::<anc>::*` — only pulls the
  ancestor's namespace into scope (scope-sugar) → emitted as **`super`**
  (non-flow). See `bar.rs:13` and `cycle_examples/sup_parent/child.rs:7`.

The consequence: a child→parent loop **counts today only when the child names a
concrete item of the parent** (the named form, a genuine dependency). With the
glob form the analyzer cannot tell whether the child truly uses a parent item
(Case 3b — a **real** back-dependency, i.e. a real cycle) or just pulls the
namespace for convenience (Case 3a — no real dependency), so it records **every**
ancestor-glob as non-flow `super`.

That is a deliberate **deprioritization**, not a claim that no dependency exists:
a file-split module looping back on itself is low-priority next to an obvious
cross-module cycle like `a ⇄ b`. The cost is a genuine 3b cycle goes unreported
(a low-priority miss); the benefit is a benign 3a glob is not reported as a false
cycle. Distinguishing the two would need name resolution, which the syntactic
analyzer does not do.

## The sample, case by case

All paths are under `crates/code-ranker-plugin-rust/sample/src/`.

| # | What | Edges (file:line) | Cycle **today**? | If `reexports`+`super` were flow? |
|---|---|---|---|---|
| 1 | **`uses` loop** | `a.rs:4` (a→b uses) + `b.rs:5` (b→a uses) | **yes — `mutual`** | yes |
| 2 | **`reexports` + back-`uses`** | `cycle_examples/reex_hub.rs:8` (hub→spoke reexports) + `cycle_examples/reex_spoke.rs:6` (spoke→hub uses) | no | **yes — `mutual`** (real) |
| 3a | **`super` glob, child uses NO parent item** | `cycle_examples/sup_loose.rs:8` (parent→child uses) + `cycle_examples/sup_loose/child.rs:9` (child→parent super) | no | reported — but a **false positive** (no real dep up) |
| 3b | **`super` glob, child USES a parent item** | `cycle_examples/sup_parent.rs:16` (parent→child uses) + `cycle_examples/sup_parent/child.rs:11` (child→parent super) | no — **deprioritized** (but it IS a real cycle) | yes — `mutual` (real) |
| 4 | **one-directional `reexports`** | `lib.rs:35` (lib→a reexports; a never depends back on lib) | no | no — only goes down |
| 5 | **one-directional `super`** | `foo/bar.rs:13` (bar→foo super) + `foo.rs:15` (foo→bar is `contains`, non-flow) | no | no — the only down-edge is `contains` |
| 6 | **`contains` only** | `lib.rs:25` (`mod foo;`), `sup_parent.rs:12` (`pub mod child;`) | no | no — `contains` is never flow |
| 7 | **cross-crate** | `cross.rs:11` (cross → `helper` crate) | no | no — multi-crate SCCs are discarded (`cycles.rs:52`) |

### Why the loops close or don't

**Case 1 — counts today.** Both `a → b` and `b → a` are `uses` (flow). Two flow
edges in opposite directions → a 2-node SCC → `mutual`. This is the only cycle
the sample reports today.

**Case 2 — a re-export hub.** `reex_hub` re-publishes the spoke's type
(`pub use … reex_spoke::Widget`) and the spoke depends back on the hub
(`use … reex_hub::Hub`). Today the flow graph has only `spoke → hub` (the `uses`
edge); the `hub → spoke` `reexports` edge is non-flow, so the loop is open — **no
cycle**. Make `reexports` flow and both directions are present → `mutual`. This
is exactly the kind of loop the metric hides while `reexports` is non-flow.

**Case 3b — a real parent ⇄ child cycle that is deprioritized.** The parent
depends on a child item (`use self::child::Chick` — a `uses` edge down) and the
child glob-pulls the parent **and actually uses `Nest`** (`use super::*` + `fn
settle(_n: Nest)`). This is a **genuine** mutual dependency — strictly, a real
cycle. But the upward edge is recorded as `super` (non-flow), because the
analyzer does not resolve that `Nest` came from the glob, so **no** `uses` edge
is emitted for it (a bare 1-segment name is not collected). Today → **not
reported**. This is a deliberate **low-priority miss**: a file-split module
looping back on itself is deprioritized vs. obvious cross-module cycles. Make
`super` flow and it surfaces as a (real) `mutual` cycle.

**Case 3a — a benign `super` glob (would be a false positive).** Same shape, but
the child (`sup_loose/child.rs`) uses **no** parent item — the glob is pure
scope-sugar. There is no real dependency upward. Today → not reported (correct).
But making `super` flow would report `sup_loose ⇄ child` as a cycle even though
the child does not depend on the parent — a **false positive**. The analyzer
cannot tell 3a from 3b without name resolution, which is the core reason `super`
is left non-flow: counting it would trade 3b false-negatives for 3a
false-positives.

**Case 4 — one-directional re-export.** `lib.rs` does `pub use crate::a::Alpha`,
but `a.rs` never depends back on `lib.rs`. Even if `reexports` were flow, the
edge only goes down → it is a DAG, **never a cycle**. (This is the prelude/facade
shape: re-export hubs add fan-out, not loops, as long as nothing depends back up.)

**Case 5 — one-directional `super`.** `bar.rs` does `use super::*` (up to
`foo.rs`), but `foo.rs` only *contains* `bar` (`pub mod bar;`) — it does not
`use` a `bar` item. So even if `super` were flow, the only down-edge is
`contains` (non-flow): the loop never closes. **Not a cycle.** Contrast Cases
3a/3b, where the parent has a real `uses` edge down.

**Case 6 — `contains` only.** Declaring a submodule (`mod foo;`, `pub mod child;`)
is ownership, not information flow. `contains` is never flow, so a parent/child
pair is **never** a cycle on its own.

**Case 7 — cross-crate.** `cross.rs` imports from the `helper` crate. Even if
such edges formed an SCC, `spans_multiple_crates` discards it (`cycles.rs:52`):
the Rust compiler forbids circular crate dependencies, so a multi-crate "cycle"
is not real.

## Status

This describes the **current** algorithm (`uses`-only flow). Treating `pub use`
and glob `use super::*` as real dependencies (`reexports` / `super` → flow) is a
**proposed** change, not yet applied — the algorithm in `cycles.rs` and the
`EdgeKindSpec.flow` flags in `crates/code-ranker-plugin-rust/src/lib.rs` are
unchanged.

If that flip landed: Cases 2 and 3b would surface as **real** `mutual` cycles
(true positives the metric hides today — Case 3b is a genuine cycle deliberately
deprioritized now), Case 3a would surface as a **false** cycle (a false positive,
which is the price of not resolving glob names), and Cases 4–7 would stay
non-cycles regardless. The sample records all of these so the trade-off is
verifiable rather than asserted.
