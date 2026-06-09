# ADP — Acyclic Dependencies Principle (in Python)

**TL;DR**: The dependency graph between modules (or packages) must
form a Directed Acyclic Graph (DAG). When module `a` depends on
module `b`, no chain of dependencies should bring `b` back to `a`.
Violations destroy releasability, testability, and import-time
sanity. Unlike Cargo, Python's import system does **not** refuse
cyclic packages — it lets you build them and then explodes at
runtime with `ImportError: cannot import name 'X' from partially
initialized module 'a'`. ADP must therefore be applied manually at
both the package and module level.

## Canonical sources

- Robert C. Martin, "Granularity: The Acyclic Dependencies Principle"
  (1996, *C++ Report*):
  <https://web.archive.org/web/20061206155400/http://www.objectmentor.com/resources/articles/granularity.pdf>
- Robert C. Martin, *Clean Architecture* (2017), Ch. 14
  "Component Coupling": ADP, SDP, SAP.
- John Lakos, *Large-Scale C++ Software Design* (1996): the original
  case for acyclic component dependencies, applicable verbatim to
  Python.
- Python Language Reference, "The import system":
  <https://docs.python.org/3/reference/import.html>
  (sys.modules caching, partially-initialised modules.)
- PEP 484 / PEP 563 / PEP 649, type hints and deferred evaluation:
  the basis for `TYPE_CHECKING`-guarded imports.

## The principle

Martin's "morning after syndrome": a developer commits a change to
a shared module, goes home, and the next morning everybody else's
import breaks. The cause is a cycle: importing `a` triggers
execution of `b`, which re-enters `a` while `a` is half-initialised,
and the name the cycle needs hasn't been bound yet.

Once the dependency graph has even one cycle:

- **Import order becomes load-bearing.** Whichever module is
  imported first wins; the other sees a partially-initialised
  sibling. Tests that pass in one order fail in another.
- **Releases lose granularity.** You cannot ship package `a` v2.0
  without also shipping `b`, `c`, and `d` at compatible versions.
- **Tests get expensive.** Importing `a` in a test drags in the
  whole SCC, including network/db/SDK modules you didn't want.
- **Runtime startup gets fragile.** `ImportError: cannot import
  name 'X' from partially initialized module 'a' (most likely due
  to a circular import)` is the standard greeting.
- **Code becomes hard to reason about.** "What does this module
  do?" cannot be answered locally if the module sits in a cycle.

The principle is therefore simple: **break the cycles**. Always.
A cycle is not a "minor smell" — it is structural debt that grows
monotonically.

## Why it matters

Cycles tend to **emerge slowly**:

- Day 1: `pkg.a` uses `pkg.b`. Fine, one-way arrow.
- Day 30: `pkg.b` needs a type from `pkg.a` "just for type hints".
  An edge appears from `b → a`. Cycle.
- Day 90: the cycle is six modules deep; somebody has sprinkled
  `import` statements inside function bodies to "make it work";
  the team accepts that "this part of the code is just messy".

The structural shape becomes load-bearing. Refactoring it
"properly" is a quarter-long project. Refactoring it "later" never
happens.

Detecting cycles **early**, while they are 2-module or 3-module
SCCs, makes them cheap to fix. Code Ranker's `module-call-cycle` and
related rules exist exactly for this reason.

## In Python

Python does **not** enforce ADP. The interpreter loads modules
lazily and caches them in `sys.modules`. Mutual imports often
"work" — until they don't:

```python
# pkg/a.py
from pkg.b import B

class A:
    b: B | None = None

# pkg/b.py
from pkg.a import A   # ImportError at runtime

class B:
    a: A | None = None
```

If `pkg.a` is imported first, Python starts executing `a.py`, hits
`from pkg.b import B`, switches to executing `b.py`, hits
`from pkg.a import A`, finds `pkg.a` already in `sys.modules` but
only **partially initialised** (the `A` class hasn't been bound
yet), and raises:

```
ImportError: cannot import name 'A' from partially initialized
module 'pkg.a' (most likely due to a circular import)
```

The traceback gives you the symptom; it doesn't tell you the
right fix.

### Package-level cycles

Two installable distributions cycling via `pyproject.toml`
dependencies is rarer but possible — pip will resolve it (modern
resolver picks compatible versions), but the runtime hazard
remains. There's no equivalent of Cargo's hard refusal.

## Common cycle shapes

### Shape 1: AppState ↔ Routes (FastAPI/Flask idiom)

```
app/main ─────────→ app/routes
   ↑                    │
   └────────────────────┘
```

`app/main.py` builds an `AppState` (or `FastAPI()` instance) and
mounts route modules. Route modules import `app.main` to reach the
state singleton or `app` object. Cycle.

Fix: extract `app/state.py` (or `app/dependencies.py`) as a leaf;
both `app.main` and `app.routes` depend on it.

### Shape 2: Sibling types referring to each other

```
core ─────────→ manager
  ↑               │
  │           ┌───┴────┐
  │           ▼        ▼
  └───── builder    factory
```

`core` defines a base class; `manager` and `builder` each have
types that reference each other through the base. Cycle (sometimes
just 2-mod, sometimes 3-mod).

Fix: extract types into a leaf `core/types.py` or `core/protocols.py`;
everyone depends on it.

### Shape 3: Routes ↔ Handlers

```
handlers ─────→ routes
   ↑              │
   └──────────────┘
```

Routes register handlers; handlers import the route definitions
to call `url_for(...)`. Cycle.

Fix: extract `urls.py` with route path constants only; both depend
on it.

### Shape 4: Package `__init__.py` god prelude ↔ submodules

```
service.stream_service ─────→ service (__init__.py re-exports)
                                  │
                              ┌───┴─────────┐
                              ▼             ▼
              service.quota_service   service.finalization_service
```

`service/__init__.py` re-exports common types from submodules;
submodules `from service import ...`. Cycle through the re-export.
This is the single most common Python cycle shape because
`__init__.py` re-exports are idiomatic.

Fix: extract `service/_types.py` (or `service/protocols.py`) as a
leaf; submodules import directly from leaf modules, never from the
package root.

## Violations and remedies

### Anti-pattern: cross-module data sharing through "convenience" imports

```python
# pkg/order/repo.py
from pkg.order.service import Order   # domain type

# pkg/order/service.py
from pkg.order.repo import OrderRepository   # infra type
```

`repo` and `service` cycle. The domain type `Order` should live in
neither — it should live in a leaf module both can depend on.

### Idiomatic fix: domain types in a leaf

```python
# pkg/order/model.py
from dataclasses import dataclass

@dataclass(frozen=True, slots=True)
class Order:
    ...

# pkg/order/repo.py
from typing import Protocol
from pkg.order.model import Order

class OrderRepository(Protocol):
    def save(self, o: Order) -> None: ...

# pkg/order/service.py
from pkg.order.model import Order
from pkg.order.repo import OrderRepository

class OrderService:
    def __init__(self, repo: OrderRepository) -> None:
        self._repo = repo
```

Three modules: `model`, `repo`, `service`. The dependency arrows
are `model ← repo ← service`. No cycle.

### Anti-pattern: routes back-reference app's state

```python
# app/main.py
from fastapi import FastAPI
from app.routes import api

class AppState:
    ...

app = FastAPI()
app.state = AppState()
app.include_router(api.router)

# app/routes/api.py
from app.main import AppState   # ImportError at runtime
```

`main → routes.api → main`. Cycle.

### Idiomatic fix: extract AppState

```python
# app/state.py
class AppState:
    ...

# app/main.py
from fastapi import FastAPI
from app.state import AppState
from app.routes import api

app = FastAPI()
app.state = AppState()
app.include_router(api.router)

# app/routes/api.py
from fastapi import APIRouter
from app.state import AppState

router = APIRouter()
```

`state` is a leaf. Both `main` and `routes.api` depend on it.

### Anti-pattern: protocol + implementation in same module, implementation pulls in dependents

```python
# pkg/cache/__init__.py
from typing import Protocol
from pkg.metrics import Metrics   # cycle starts here

class Cache(Protocol):
    def get(self, k: str) -> bytes | None: ...

class InstrumentedCache:
    def __init__(self, m: Metrics) -> None: ...

# pkg/metrics.py
from pkg.cache import Cache   # cycle closes
```

`cache → metrics → cache`. Cycle.

### Idiomatic fix: separate protocol module

```python
# pkg/cache/protocol.py  (leaf)
from typing import Protocol

class Cache(Protocol):
    def get(self, k: str) -> bytes | None: ...

# pkg/metrics.py
from pkg.cache.protocol import Cache

class MetricsCache: ...

# pkg/cache/instrumented.py
from pkg.cache.protocol import Cache
from pkg.metrics import Metrics

class InstrumentedCache:
    def __init__(self, m: Metrics) -> None: ...
```

Cycle broken. `protocol` is the leaf both `metrics` and
`cache.instrumented` reach for.

## The Python "trick": deferred imports inside functions

A widely-used "fix" for circular imports is to push the offending
`import` into a function body:

```python
# pkg/a.py
def make_thing():
    from pkg.b import B   # imported lazily, after both modules
    return B()            # are fully initialised

# pkg/b.py
from pkg.a import make_thing
```

This **works**. The import is deferred until the function is
called, by which time `sys.modules` holds a fully-initialised
`pkg.b`. The `ImportError` goes away.

It is, however, a **smell, not a fix**:

- The dependency arrow `a → b` still exists; you only hid it from
  the import-time graph. Code Ranker's call-graph rules still see
  it.
- The cycle is now invisible to grep-based reviews and to most
  static analyzers that look only at module-level imports.
- Touching either module still invalidates the other's reasoning:
  testability and releasability haven't improved.
- The next developer to add a top-level call to `make_thing()` from
  module-import context resurrects the original `ImportError`.
- Type checkers see the lazy import and degrade inference inside
  the function.

If you find yourself reaching for the deferred-import trick, treat
it as an alarm: **there is a cycle, and the architecture wants you
to extract a leaf module.** Use deferred imports only as a
temporary tourniquet while the proper refactor is in flight.

## `TYPE_CHECKING`-guarded imports: acceptable, but distinct

PEP 484 introduced `typing.TYPE_CHECKING`, a constant that is
`False` at runtime and `True` under type checkers. With
`from __future__ import annotations` (PEP 563) or PEP 649
deferred evaluation in 3.14, annotations are strings or lazy and
do not require the referenced names to exist at runtime:

```python
# pkg/a.py
from __future__ import annotations
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from pkg.b import B   # type-only, never executed at runtime

class A:
    def link(self, b: B) -> None: ...

# pkg/b.py
from __future__ import annotations
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from pkg.a import A

class B:
    def link(self, a: A) -> None: ...
```

This is **legitimate** and not what ADP forbids. Both modules type-
check against each other, but neither imports the other at module
load time. The runtime import graph is acyclic; the type graph
isn't, and type cycles are harmless.

Caveats:

- The moment one of these names is needed at runtime (e.g.,
  `isinstance(x, B)`, `pydantic.TypeAdapter(B)`, `dataclass`
  field-resolution with `eq=True` in some configurations,
  `get_type_hints()` without `include_extras` guards), the cycle
  surfaces again.
- Frameworks that resolve annotations eagerly (older Pydantic v1,
  SQLAlchemy ORM mapping, attrs `auto_attribs`) may force the
  import. Audit before relying on `TYPE_CHECKING`.
- `TYPE_CHECKING` imports are still a *design* signal: if two
  modules need each other's types, ask whether the types belong
  in a shared leaf.

## Cycles in import vs cycles in calls

**Import cycle (module-level Uses cycle)**: module `a` does
`from b import ...` and vice versa at module top level. Often
explodes at runtime with `ImportError`; even when it doesn't (due
to import-order luck), Code Ranker flags it. Usually easy to break
by extracting types into a leaf module — no actual logic change.

**Call cycle (module-level Calls cycle)**: function in `a`
invokes function in `b` which invokes function in `a`. Compiles
fine, imports fine — but the modules' runtime responsibilities are
entangled. Sometimes legitimate (mutual recursion across modules),
but usually means responsibilities are misaligned and should be
re-cut.

Code Ranker distinguishes the two: `module-call-cycle` is Critical;
import-only cycles are Medium/Low depending on size. Deferred
function-body imports collapse the import cycle into a call cycle,
which Code Ranker still catches.

## ADP at the package level

There is no Cargo here. pip and uv will resolve diamond-via-
multiple-versions situations by picking *one* version of the
shared dependency (pip's resolver, since 20.3, is strict; older
behaviour was last-wins). Symptoms of version skew:

```
A → B v1.0
A → C → B v2.0
```

A single `B` ends up installed; whichever side expected the other
version sees attribute errors or `TypeError` at runtime, not
compile-time mismatches. Pin compatible ranges; consider a uv/Poetry
workspace with shared constraints.

Bigger picture: a workspace passes ADP when:

- No package-level cycle exists between distributions (no tool
  enforces this; review by `pyproject.toml` audit).
- No version skew exists for shared dependencies (lockfiles +
  workspace inheritance help).
- The package-level DAG is **shallow**: prefer flat layouts (one
  or two layers of packages) over deep towers of micro-packages.

## How code-ranker detects ADP violations

Code Ranker's primary purpose includes ADP enforcement:

| Signal | Rule |
|---|---|
| SCC of size > 1 on module-level `Imports`/`Reexports` edges | `prelude-sibling-cycle`, structural-cycle-report (general) |
| SCC of size > 1 on module-level call graph | `module-call-cycle` (Critical) |
| Package-level cycle | Reported by Code Ranker's cross-package analysis |
| Layer violation (`libs/*` depends on `apps/*`) | Flagged in cross-package analysis report |

Existing rule cross-references:
- `app-state-cycle`: a specific shape of import cycle
- `prelude-sibling-cycle`: a specific shape of {`__init__.py`
  prelude / sibling submodule} cycle
- `module-call-cycle`: any module-level call cycle (catches
  deferred-import "fixes")

A future general rule `module-import-cycle` could capture remaining
cases that don't fit a specific shape.

## Suggested recommendation template

> **ADP violation**: modules `app.routes.api` and `app.main` form
> a 2-module import cycle in package `myservice`. The morning-after
> failure mode for cycles (Martin 1996) applies: changes in either
> module invalidate the other; release granularity is lost; the
> import order has already begun to leak into test setup. Break
> the cycle by extracting `app/state.py` as a leaf; both
> `app.routes.api` and `app.main` depend on it.
>
> Reference:
> <https://web.archive.org/web/20061206155400/http://www.objectmentor.com/resources/articles/granularity.pdf>

## ADP and import-time cost

Even when a cycle imports successfully, it hurts startup time and
test isolation. Importing any one module in the SCC transitively
loads the rest — including any side effects each member runs at
import time (logging config, metric registration, ORM mapping).

A subtle implication: cycles are **time-multiplicative** for
startup. A 12-module SCC means every `pytest` test that touches
one module pays the cost of all 12. This is why cycles feel
"stickier" than straight-line dependencies as the codebase grows,
and why CI test suites mysteriously slow down over time.

## Related principles

- [DIP](solid-dependency-inversion.md) — DIP is *how* you break a
  cycle. The `Protocol` (or ABC) moves to one side of the arrow;
  the cycle becomes a one-way street.
- [SRP](solid-single-responsibility.md) — modules that share
  responsibilities tend to cycle. SRP-clean modules don't.
- [ISP](solid-interface-segregation.md) — fat Protocols pull more
  symbols across the cycle boundary; segregated ones don't.
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
4. Python Language Reference, "The import system".
   <https://docs.python.org/3/reference/import.html>
5. PEP 484, "Type Hints" (introduces `TYPE_CHECKING`).
   <https://peps.python.org/pep-0484/>
6. PEP 563, "Postponed Evaluation of Annotations".
   <https://peps.python.org/pep-0563/>
7. PEP 649, "Deferred Evaluation Of Annotations Using Descriptors"
   (Python 3.14). <https://peps.python.org/pep-0649/>
