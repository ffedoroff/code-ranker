# ADP — Acyclic Dependencies Principle (in Rust)

**TL;DR**: The dependency graph between modules (or crates) must
form a Directed Acyclic Graph (DAG). When module `A` depends on
module `B`, no chain of dependencies should bring `B` back to `A`.
Violations destroy releasability, testability, and incremental
compilation. Rust enforces ADP at the crate level (Cargo refuses
cyclic dependencies). The principle still has to be applied
manually at the module level inside a crate.

<!-- doc:base "Canonical sources" -->
<!-- doc:base "The principle" -->
<!-- doc:base "Why it matters" -->

## In Rust

Rust enforces ADP at the **crate** level absolutely:

```toml
# In foo's Cargo.toml
[dependencies]
bar = { path = "../bar" }

# In bar's Cargo.toml
[dependencies]
foo = { path = "../foo" }   # ERROR: cyclic-package dependency
```

Cargo refuses to build this configuration. ADP is structurally
guaranteed across crates.

At the **module** level, Rust does NOT enforce ADP. Cycles between
sibling modules in the same crate compile fine:

```rust
// crate root
mod a;
mod b;

// src/a.rs
use crate::b::B;
pub struct A { pub b: Option<B> }

// src/b.rs
use crate::a::A;
pub struct B { pub a: Option<A> }
```

This compiles. The compiler does not flag it. Code Ranker does.

### Visibility paths count as dependency edges

Code Ranker builds the module graph from **both** `use`
imports **and visibility paths**. A `pub(in crate::a::b)`
item is reachable from anywhere under `crate::a::b`, so it
records an edge from the item's module **up to** that
ancestor — even when nothing there calls it. That is a real
coupling signal (the item joins that ancestor's internal API
surface), but it means a reported back-edge is not always a
real `use`.

```rust
// src/services/user_service/patch_user/status_handler.rs
// the visibility path names the user_service module, so
// code-ranker records an edge to user_service/mod.rs
pub(in crate::services::user_service)
fn update_user_status(/* … */) { /* … */ }
```

**This is the first remedy to try for a Rust module cycle —
before any extract / split / move shape in the sections
below.** Work it in order:

1. **Find the back-edges** — the cycle edges that point *up*
   to an ancestor module. For each, check whether it is a
   real `use` or only a `pub(in <path>)` visibility path.
2. **If a back-edge is a visibility path, narrow it** to the
   smallest scope its real callers need — `pub(super)` if
   only the parent uses the item, `pub(crate)` if a sibling
   subtree does. The edge to the root disappears, the cycle
   breaks, the item is less exposed (least privilege), and
   **no module moves and no test changes** — usually a
   one-line edit per item.
3. **Only a genuine `use` back-edge** needs the structural
   shapes below (extract a shared leaf, invert, split).

Check the call sites before narrowing: if an item is truly
used across the whole subtree, narrowing won't compile — that
is a *real* dependency, so extract or invert instead.
Tightening a visibility you have not verified is silencing
the metric, not fixing the structure. But the reverse is the
more common cheaper-tier mistake: **do not extract or move a
module when a one-line visibility change is the actual fix.**
The shapes below are for `use` cycles; reach for them only
after step 1 shows the back-edges are real imports.

<!-- doc:base "Module-level cycles" -->
<!-- doc:base "Common cycle shapes" -->
<!-- doc:base "Violations and remedies" -->
<!-- doc:base "Cycles in import vs cycles in calls" -->

## ADP at the crate level

Cargo enforces it for *direct* path dependencies. It does not
prevent diamond-via-multiple-versions situations:

```toml
A → B v1.0
A → C → B v2.0
```

Two versions of `B` coexist; types from `B v1.0` are incompatible
with `B v2.0`. Symptoms: "expected B::Foo, found B::Foo" compile
errors. The semver-trick (David Tolnay) is the canonical
remediation; see [OCP](OCP.md).

Bigger picture: a workspace passes ADP when:

- No path-dep cycle exists between crates (Cargo enforces).
- No version skew exists for shared dependencies (workspace
  inheritance helps).
- The crate-level DAG is **shallow** (matklad's "Large Rust
  Workspaces" advocates flat layouts: one or two layers of crates,
  not a deep tower).

<!-- doc:base "How code-ranker detects ADP violations" -->
<!-- doc:base "Suggested recommendation template" -->

## ADP and incremental compilation

Even when a cycle compiles, it hurts Cargo's incremental rebuilds.
Cargo invalidates modules whose dependencies have changed. In a
cycle, all members share the same change set — touching one
forces recompilation of all. The wider the cycle, the longer the
rebuild.

A subtle implication: cycles are **time-multiplicative** for build
performance. A 12-module SCC means every change to any one of the
12 invalidates the others. This is why cycles feel "stickier" than
straight-line dependencies as the codebase grows.

<!-- doc:base "Related principles" -->
<!-- doc:base "References" -->
