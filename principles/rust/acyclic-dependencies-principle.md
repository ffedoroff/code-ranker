# ADP — Acyclic Dependencies Principle (in Rust)

**TL;DR**: The dependency graph between modules (or crates) must
form a Directed Acyclic Graph (DAG). When module `A` depends on
module `B`, no chain of dependencies should bring `B` back to `A`.
Violations destroy releasability, testability, and incremental
compilation. Rust enforces ADP at the crate level (Cargo refuses
cyclic dependencies). The principle still has to be applied
manually at the module level inside a crate.

## Canonical sources

- Robert C. Martin, "Granularity: The Acyclic Dependencies Principle"
  (1996, *C++ Report*):
  <https://web.archive.org/web/20061206155400/http://www.objectmentor.com/resources/articles/granularity.pdf>
- Robert C. Martin, *Clean Architecture* (2017), Ch. 14
  "Component Coupling": ADP, SDP, SAP.
- Cargo Book, "Dependency Resolution":
  <https://doc.rust-lang.org/cargo/reference/resolver.html>
  (Cargo enforces ADP for crate dependencies.)
- matklad, "Large Rust Workspaces":
  <https://matklad.github.io/2021/08/22/large-rust-workspaces.html>
- John Lakos, *Large-Scale C++ Software Design* (1996): the original
  case for acyclic component dependencies, applicable verbatim to
  Rust.

## The principle

Martin's "morning after syndrome": a developer commits a change to
a shared module, goes home, and the next morning everybody else's
build breaks. The cause is a cycle: changing `A` forces a rebuild
of `B`, which forces a rebuild of `C`, which depends on a different
version of `A`, etc.

Once the dependency graph has even one cycle:

- **Build order is undefined.** Cargo cannot pick "what to compile
  first" when there's no topological sort.
- **Releases lose granularity.** You cannot ship `A` v2.0 without
  also shipping `B`, `C`, and `D` at compatible versions.
- **Tests get expensive.** Testing `A` requires compiling `B` and
  `C` and `D`, even when the test exercises only `A`.
- **Incremental compilation cannot help.** Touching `A` invalidates
  the cycle members.
- **Code becomes hard to reason about.** "What does this module
  do?" cannot be answered locally if the module sits in a cycle.

The principle is therefore simple: **break the cycles**. Always.
A cycle is not a "minor smell" — it is structural debt that grows
monotonically.

## Why it matters

Cycles tend to **emerge slowly**:

- Day 1: `module::a` uses `module::b`. Fine, one-way arrow.
- Day 30: `module::b` needs a type from `module::a` for "convenience".
  An edge appears from `b → a`. Cycle.
- Day 90: the cycle is six modules deep; nobody remembers when each
  edge was added; the team accepts that "this part of the code is
  just messy".

The structural shape becomes load-bearing. Refactoring it
"properly" is a quarter-long project. Refactoring it "later" never
happens.

Detecting cycles **early**, while they are 2-module or 3-module
SCCs, makes them cheap to fix. Code Ranker's `module-call-cycle` and
related rules exist exactly for this reason.

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

## Common cycle shapes

### Shape 1: AppState ↔ Routes (axum/actix idiom)

```
module ─────────→ routes
   ↑                 │
   └─────────────────┘
```

The crate root (`module.rs`/`lib.rs`) wires up an `AppState` and
mounts route handlers. Route handlers reach back into `module.rs`
for the `AppState` struct. Cycle.

Fix: extract `app_state.rs` as a leaf; both `module` and `routes`
depend on it.

### Shape 2: Sibling types referring to each other

```
core ─────────→ manager
  ↑               │
  │           ┌───┴────┐
  │           ▼        ▼
  └───── builder    factory
```

`core` defines a trait; `manager` and `builder` each have types
that reference each other through the trait. Cycle (sometimes
just 2-mod, sometimes 3-mod).

Fix: extract types into a leaf `core::types` or `core::handle`;
everyone depends on it.

### Shape 3: Routes ↔ Handlers

```
handlers ─────→ routes
   ↑              │
   └──────────────┘
```

Routes register handlers; handlers import the route definitions
to call `Url::route_for(...)`. Cycle.

Fix: extract `urls.rs` with route definitions only; both depend
on it.

### Shape 4: Service god prelude ↔ subservices

```
service::stream_service ─────→ service (prelude)
                                  │
                              ┌───┴─────────┐
                              ▼             ▼
              service::quota_service   service::finalization_service
```

`service` re-exports common types from subservices; subservices
import from `service`. Cycle through the re-export.

Fix: extract `service::prelude` or `service::types` as a leaf;
subservices import directly from leaf modules.

## Violations and remedies

### Anti-pattern: cross-module data sharing through "convenience" imports

```rust
// crates/order/src/repo.rs
use crate::service::Order;        // domain type

// crates/order/src/service.rs
use crate::repo::OrderRepository; // infra type
```

`repo` and `service` cycle. The domain type "Order" should live in
neither — it should live in a leaf module both can depend on.

### Idiomatic fix: domain types in a leaf

```rust
// crates/order/src/model.rs
pub struct Order { /* ... */ }

// crates/order/src/repo.rs
use crate::model::Order;
pub trait OrderRepository { fn save(&self, o: &Order); }

// crates/order/src/service.rs
use crate::model::Order;
use crate::repo::OrderRepository;
pub struct OrderService<R: OrderRepository> { repo: R }
```

Three modules: `model`, `repo`, `service`. The dependency arrows
are `model ← repo ← service`. No cycle.

### Anti-pattern: routes back-reference module's state

```rust
// src/module.rs
pub struct AppState { /* ... */ }
pub fn build_router(state: AppState) -> Router {
    Router::new().nest("/api", routes::api_routes()).with_state(state)
}

// src/routes/api.rs
use crate::module::AppState;
pub fn api_routes() -> Router<AppState> { /* ... */ }
```

`module → routes::api → module`. Cycle.

### Idiomatic fix: extract AppState

```rust
// src/state.rs
pub struct AppState { /* ... */ }

// src/module.rs
use crate::state::AppState;
use crate::routes;
pub fn build_router(state: AppState) -> Router {
    Router::new().nest("/api", routes::api_routes()).with_state(state)
}

// src/routes/api.rs
use crate::state::AppState;
pub fn api_routes() -> Router<AppState> { /* ... */ }
```

`state` is a leaf. Both `module` and `routes::api` depend on it.

### Anti-pattern: trait + implementation in same module, implementation pulls in dependents

```rust
// src/cache/mod.rs
pub trait Cache { /* ... */ }

use crate::metrics::Metrics;
pub struct InstrumentedCache { /* ... */ }
impl Cache for InstrumentedCache { /* ... */ }

// src/metrics.rs
use crate::cache::Cache;       // uses cache for storage of metrics
```

`cache → metrics → cache`. Cycle.

### Idiomatic fix: separate trait module

```rust
// src/cache/trait.rs (leaf)
pub trait Cache { /* ... */ }

// src/metrics.rs
use crate::cache::r#trait::Cache;
// metrics uses cache but only its trait
struct MetricsCache { /* ... */ }

// src/cache/instrumented.rs
use crate::cache::r#trait::Cache;
use crate::metrics::Metrics;
pub struct InstrumentedCache { metrics: Metrics }
impl Cache for InstrumentedCache { /* ... */ }
```

Cycle broken. `trait` is the leaf both `metrics` and
`cache::instrumented` reach for.

## Cycles in import vs cycles in calls

**Import cycle (module-level Uses cycle)**: module `A` `use`-s a
type from module `B` and vice versa. Compiles fine, but Code Ranker
flags it. Often easy to break by extracting types into a leaf
module — no actual code change to logic.

**Call cycle (module-level Calls cycle)**: function in `A` invokes
function in `B` which invokes function in `A`. This is a real
runtime cycle. It is sometimes legitimate (recursion across modules),
but usually means the modules' responsibilities are entangled and
should be re-aligned.

Code Ranker distinguishes the two: `module-call-cycle` is Critical;
import-only cycles are Medium/Low depending on size.

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
remediation; see [OCP](solid-open-closed.md).

Bigger picture: a workspace passes ADP when:

- No path-dep cycle exists between crates (Cargo enforces).
- No version skew exists for shared dependencies (workspace
  inheritance helps).
- The crate-level DAG is **shallow** (matklad's "Large Rust
  Workspaces" advocates flat layouts: one or two layers of crates,
  not a deep tower).

## How code-ranker detects ADP violations

Code Ranker's primary purpose includes ADP enforcement:

| Signal | Rule |
|---|---|
| SCC of size > 1 on module-level `Uses`/`Reexports` edges | `prelude-sibling-cycle`, `outbox-layering`, structural-cycle-report (general) |
| SCC of size > 1 on module-level call graph | `module-call-cycle` (Critical) |
| Crate-level cycle | Reported by Code Ranker's analysis (currently 0 on cyberfabric-core) |
| Layer violation (`libs/*` depends on `modules/*`) | Flagged in cross-crate analysis report |

Existing rule cross-references:
- `axum-state-cycle`: a specific shape of import cycle
- `outbox-layering`: a specific shape of {core/manager/builder} cycle
- `prelude-sibling-cycle`: a specific shape of {prelude/sibling} cycle
- `module-call-cycle`: any module-level call cycle

A future general rule `module-import-cycle` could capture remaining
cases that don't fit a specific shape.

## Suggested recommendation template

> **ADP violation**: modules `routes` and `module` form a 2-module
> import cycle in crate `cyberware-mini-chat`. The morning-after
> failure mode for cycles (Martin 1996) applies: changes in either
> module invalidate the other; release granularity is lost.
> Break the cycle by extracting `app_state.rs` as a leaf; both
> `routes` and `module` depend on it.
>
> Reference:
> <https://web.archive.org/web/20061206155400/http://www.objectmentor.com/resources/articles/granularity.pdf>

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

## Related principles

- [DIP](solid-dependency-inversion.md) — DIP is *how* you break a
  cycle. The trait moves to one side of the arrow; the cycle
  becomes a one-way street.
- [SRP](solid-single-responsibility.md) — modules that share
  responsibilities tend to cycle. SRP-clean modules don't.
- [SDP — Stable Dependencies Principle](https://web.archive.org/web/20110714224327/http://www.objectmentor.com/resources/articles/stability.pdf)
  (Martin): dependencies should point in the direction of stability.
- [SAP — Stable Abstractions Principle](https://web.archive.org/web/20110714224327/http://www.objectmentor.com/resources/articles/stability.pdf)
  (Martin): stable modules should be abstract.

## References

1. Martin, R. C. "Granularity: The Acyclic Dependencies Principle".
   *C++ Report*, 1996.
   <https://web.archive.org/web/20061206155400/http://www.objectmentor.com/resources/articles/granularity.pdf>
2. Martin, R. C. *Clean Architecture*. Ch. 14.
3. Lakos, J. *Large-Scale C++ Software Design*. 1996, Ch. 4–5.
4. matklad. "Large Rust Workspaces". 2021.
   <https://matklad.github.io/2021/08/22/large-rust-workspaces.html>
5. Cargo Book, dependency resolution.
   <https://doc.rust-lang.org/cargo/reference/resolver.html>
6. Tolnay, D. "The semver trick".
   <https://github.com/dtolnay/semver-trick>
