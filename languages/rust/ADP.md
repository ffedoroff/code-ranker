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
