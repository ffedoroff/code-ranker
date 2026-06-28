# ADP — Acyclic Dependencies Principle

**TL;DR**: The dependency graph between modules (or packages) must
form a Directed Acyclic Graph (DAG). When module `A` depends on
module `B`, no chain of dependencies should bring `B` back to `A`.
Violations destroy releasability, testability, and incremental
builds. Some toolchains enforce ADP at the package level (the build
tool refuses cyclic package dependencies). Even then, the principle
still has to be applied manually at the module level inside a
package.

## Canonical sources

- Robert C. Martin, "Granularity: The Acyclic Dependencies Principle"
  (1996, *C++ Report*):
  <https://web.archive.org/web/20061206155400/http://www.objectmentor.com/resources/articles/granularity.pdf>
- Robert C. Martin, *Clean Architecture* (2017), Ch. 14
  "Component Coupling": ADP, SDP, SAP.
- John Lakos, *Large-Scale C++ Software Design* (1996): the original
  case for acyclic component dependencies, applicable to any
  language.

## The principle

Martin's "morning after syndrome": a developer commits a change to
a shared module, goes home, and the next morning everybody else's
build breaks. The cause is a cycle: changing `A` forces a rebuild
of `B`, which forces a rebuild of `C`, which depends on a different
version of `A`, etc.

Once the dependency graph has even one cycle:

- **Build order is undefined.** The build tool cannot pick "what to
  compile first" when there's no topological sort.
- **Releases lose granularity.** You cannot ship `A` v2.0 without
  also shipping `B`, `C`, and `D` at compatible versions.
- **Tests get expensive.** Testing `A` requires building `B` and
  `C` and `D`, even when the test exercises only `A`.
- **Incremental builds cannot help.** Touching `A` invalidates
  the cycle members.
- **Code becomes hard to reason about.** "What does this module
  do?" cannot be answered locally if the module sits in a cycle.

The principle is therefore simple: **break the cycles**. Always.
A cycle is not a "minor smell" — it is structural debt that grows
monotonically.

## Why it matters

Cycles tend to **emerge slowly**:

- Day 1: `module a` uses `module b`. Fine, one-way arrow.
- Day 30: `module b` needs a type from `module a` for "convenience".
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

## Module-level cycles

Many toolchains enforce ADP at the **package** level absolutely: if
package `foo` lists `bar` as a dependency and `bar` lists `foo` in
turn, the build tool refuses to build the configuration. ADP is then
structurally guaranteed across packages.

At the **module** level, the language usually does NOT enforce ADP.
Cycles between sibling modules in the same package compile fine:

```
// package root declares two modules: a, b

// module a
import b.B
public type A { b: optional B }

// module b
import a.A
public type B { a: optional A }
```

This compiles. The compiler does not flag it. Code Ranker does.

## Common cycle shapes

### Shape 1: AppState ↔ Routes (web-framework idiom)

```
module ─────────→ routes
   ↑                 │
   └─────────────────┘
```

The package root wires up an `AppState` and mounts route handlers.
Route handlers reach back into the root for the `AppState` type.
Cycle.

Fix: extract `app_state` as a leaf; both `module` and `routes`
depend on it.

### Shape 2: Sibling types referring to each other

```
core ─────────→ manager
  ↑               │
  │           ┌───┴────┐
  │           ▼        ▼
  └───── builder    factory
```

`core` defines an interface; `manager` and `builder` each have types
that reference each other through the interface. Cycle (sometimes
just 2-mod, sometimes 3-mod).

Fix: extract types into a leaf `core.types` or `core.handle`;
everyone depends on it.

### Shape 3: Routes ↔ Handlers

```
handlers ─────→ routes
   ↑              │
   └──────────────┘
```

Routes register handlers; handlers import the route definitions
to call `Url.route_for(...)`. Cycle.

Fix: extract `urls` with route definitions only; both depend
on it.

### Shape 4: Service god prelude ↔ subservices

```
service.stream_service ─────→ service (prelude)
                                  │
                              ┌───┴─────────┐
                              ▼             ▼
              service.quota_service   service.finalization_service
```

`service` re-exports common types from subservices; subservices
import from `service`. Cycle through the re-export.

Fix: extract `service.prelude` or `service.types` as a leaf;
subservices import directly from leaf modules.

## Violations and remedies

### Anti-pattern: cross-module data sharing through "convenience" imports

```
// order/repo
import service.Order        // domain type

// order/service
import repo.OrderRepository  // infra type
```

`repo` and `service` cycle. The domain type "Order" should live in
neither — it should live in a leaf module both can depend on.

### Idiomatic fix: domain types in a leaf

```
// order/model
public type Order { /* ... */ }

// order/repo
import model.Order
public interface OrderRepository { save(o: Order) }

// order/service
import model.Order
import repo.OrderRepository
public type OrderService(repo: OrderRepository) { /* ... */ }
```

Three modules: `model`, `repo`, `service`. The dependency arrows
are `model ← repo ← service`. No cycle.

### Anti-pattern: routes back-reference the root's state

```
// module (root)
public type AppState { /* ... */ }
public function build_router(state: AppState) -> Router {
    Router().nest("/api", routes.api_routes()).with_state(state)
}

// routes/api
import module.AppState
public function api_routes() -> Router<AppState> { /* ... */ }
```

`module → routes.api → module`. Cycle.

### Idiomatic fix: extract AppState

```
// state
public type AppState { /* ... */ }

// module (root)
import state.AppState
import routes
public function build_router(state: AppState) -> Router {
    Router().nest("/api", routes.api_routes()).with_state(state)
}

// routes/api
import state.AppState
public function api_routes() -> Router<AppState> { /* ... */ }
```

`state` is a leaf. Both `module` and `routes.api` depend on it.

### Anti-pattern: interface + implementation in same module, implementation pulls in dependents

```
// cache
public interface Cache { /* ... */ }

import metrics.Metrics
public type InstrumentedCache { /* ... */ }
// InstrumentedCache implements Cache

// metrics
import cache.Cache       // uses cache for storage of metrics
```

`cache → metrics → cache`. Cycle.

### Idiomatic fix: separate interface module

```
// cache/contract (leaf)
public interface Cache { /* ... */ }

// metrics
import cache.contract.Cache
// metrics uses cache but only its interface
type MetricsCache { /* ... */ }

// cache/instrumented
import cache.contract.Cache
import metrics.Metrics
public type InstrumentedCache { metrics: Metrics }
// InstrumentedCache implements Cache
```

Cycle broken. `contract` is the leaf both `metrics` and
`cache.instrumented` reach for.

## Cycles in import vs cycles in calls

**Import cycle (module-level Uses cycle)**: module `A` imports a
type from module `B` and vice versa. Compiles fine, but Code Ranker
flags it. Often easy to break by extracting types into a leaf
module — no actual code change to logic.

**Call cycle (module-level Calls cycle)**: a function in `A` invokes
a function in `B` which invokes a function in `A`. This is a real
runtime cycle. It is sometimes legitimate (recursion across modules),
but usually means the modules' responsibilities are entangled and
should be re-aligned.

Code Ranker distinguishes the two: `module-call-cycle` is Critical;
import-only cycles are Medium/Low depending on size.

## ADP at the package level

A build tool that enforces acyclicity for *direct* dependencies does
not necessarily prevent diamond-via-multiple-versions situations:

```
A → B v1.0
A → C → B v2.0
```

Two versions of `B` coexist; types from `B v1.0` are incompatible
with `B v2.0`. Symptoms: "expected B.Foo, found B.Foo" type errors.
A versioning-trick (re-exporting the new type from the old version)
is the canonical remediation; see [OCP](OCP.md).

Bigger picture: a multi-package project passes ADP when:

- No dependency cycle exists between packages.
- No version skew exists for shared dependencies (centralized
  dependency configuration helps).
- The package-level DAG is **shallow** — flat layouts (one or two
  layers of packages, not a deep tower) are preferable.

## How code-ranker detects ADP violations

Code Ranker's primary purpose includes ADP enforcement:

| Signal | Rule |
|---|---|
| SCC of size > 1 on module-level `Uses`/`Reexports` edges | `prelude-sibling-cycle`, `outbox-layering`, structural-cycle-report (general) |
| SCC of size > 1 on module-level call graph | `module-call-cycle` (Critical) |
| Package-level cycle | Reported by Code Ranker's analysis (currently 0 on cyberfabric-core) |
| Layer violation (`libs/*` depends on `modules/*`) | Flagged in cross-package analysis report |

Existing rule cross-references:
- `axum-state-cycle`: a specific shape of import cycle
- `outbox-layering`: a specific shape of {core/manager/builder} cycle
- `prelude-sibling-cycle`: a specific shape of {prelude/sibling} cycle
- `module-call-cycle`: any module-level call cycle

A future general rule `module-import-cycle` could capture remaining
cases that don't fit a specific shape.

## Suggested recommendation template

> **ADP violation**: modules `routes` and `module` form a 2-module
> import cycle in package `cyberware-mini-chat`. The morning-after
> failure mode for cycles (Martin 1996) applies: changes in either
> module invalidate the other; release granularity is lost.
> Break the cycle by extracting `app_state` as a leaf; both
> `routes` and `module` depend on it.
>
> Reference:
> <https://web.archive.org/web/20061206155400/http://www.objectmentor.com/resources/articles/granularity.pdf>

## ADP and incremental builds

Even when a cycle compiles, it hurts incremental rebuilds. A build
tool invalidates modules whose dependencies have changed. In a
cycle, all members share the same change set — touching one
forces recompilation of all. The wider the cycle, the longer the
rebuild.

A subtle implication: cycles are **time-multiplicative** for build
performance. A 12-module SCC means every change to any one of the
12 invalidates the others. This is why cycles feel "stickier" than
straight-line dependencies as the codebase grows.

## Related principles

- [DIP](DIP.md) — DIP is *how* you break a
  cycle. The interface moves to one side of the arrow; the cycle
  becomes a one-way street.
- [SRP](SRP.md) — modules that share
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
