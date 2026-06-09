# DIP — Dependency Inversion Principle (in Python)

**TL;DR**: High-level modules should not depend on low-level modules;
both should depend on **abstractions**. Abstractions should not depend
on details. In Python this becomes: domain packages define
`typing.Protocol` interfaces; adapter packages implement them; the
application wires concrete classes in at the composition root.

## Canonical sources

- Robert C. Martin, "The Dependency Inversion Principle" (1996):
  <https://web.archive.org/web/20110714224327/http://www.objectmentor.com/resources/articles/dip.pdf>
- Martin, *Clean Architecture*, Ch. 11.
- Mark Seemann, *Dependency Injection in .NET*, Manning (2011).
  Concepts apply to Python 1:1.
- Alistair Cockburn, "Hexagonal Architecture" (2005):
  <https://alistair.cockburn.us/hexagonal-architecture/>
- Harry Percival & Bob Gregory, *Architecture Patterns with Python*
  (O'Reilly, 2020) — the canonical book-length DIP-for-Python treatment,
  with ports/adapters worked out as Python packages.
- PEP 544 — Protocols: structural subtyping (static duck typing).
  <https://peps.python.org/pep-0544/>

## The principle

The literal rule:

1. High-level modules should not depend on low-level modules. Both
   should depend on abstractions.
2. Abstractions should not depend on details. Details should depend
   on abstractions.

Concretely: if `domain` orchestrates business rules and `postgres_adapter`
implements storage, the `from x import y` arrow should run from
`postgres_adapter → domain` (the adapter implements a Protocol
defined in domain), **not** `domain → postgres_adapter` (domain
imports psycopg directly). The dependency arrow at the import level
is inverted from the flow of control.

This is the principle behind:

- Hexagonal Architecture / Ports & Adapters (Cockburn, 2005)
- Onion Architecture (Palermo, 2008)
- Clean Architecture (Martin, 2012)

All three are the same idea: *the domain owns the interfaces, the
infrastructure owns the implementations*.

## Why it matters

When the high-level depends on the low-level:

- **Replaceability** disappears. Want to swap Postgres for SQLite?
  Now you change every `import psycopg` in `domain`.
- **Testability** disappears. The domain cannot be unit-tested
  without bringing up Postgres (or monkey-patching the driver, which
  is harder than substituting your own Protocol).
- **Layering** disappears. The "domain" package imports a database
  driver, defeating the purpose of a separate domain package.
- **Startup time and install footprint** explode. Every adapter
  dependency becomes a transitive requirement of the domain.

In a Python project, the DIP arrow shows up in the **import graph**.
A project passes DIP when the `domain` package has no incoming
infrastructure imports and outgoing ones flow through Protocols the
domain owns.

## In Python

Python's `typing.Protocol` (PEP 544) makes DIP particularly clean
because Protocols are structural: an adapter implements the interface
just by having the right methods. Combined with PEP 695 generics and
`@override`, the abstractions stay tight without the boilerplate of
nominal inheritance.

```
┌──────────────────────────────────┐
│ application (composition root)   │  ← only this package sees all
│  - main / entrypoint             │     concrete classes
│  - wires PostgresOrderRepo into  │
│    use-cases                     │
└────────┬─────────────────────────┘
         │ imports (concrete)
         ▼
┌──────────────────────────────────┐    ┌──────────────────────────────────┐
│ adapters.postgres                │    │ adapters.redis                   │
│  - class PostgresOrderRepo       │    │  - class RedisOrderCache         │
│  - implements OrderRepository    │    │  - implements OrderCache         │
└────────┬─────────────────────────┘    └────────┬─────────────────────────┘
         │ imports Protocol                       │ imports Protocol
         ▼                                        ▼
┌──────────────────────────────────────────────────────────────────────────┐
│ domain (the centre)                                                       │
│  - class OrderRepository(Protocol)                                        │
│  - class OrderCache(Protocol)                                             │
│  - @dataclass class Order, OrderId, ...                                   │
│  - use-cases that take repo: OrderRepository, cache: OrderCache           │
└──────────────────────────────────────────────────────────────────────────┘
```

The domain package has **zero infrastructure dependencies**. It imports
only from the standard library (`dataclasses`, `typing`, `datetime`,
`decimal`). Tests live next to use-cases and inject fakes that satisfy
the Protocol structurally.

## Violations and remedies

### Anti-pattern: domain calls infrastructure directly

```python
# src/myapp/domain/order_service.py
import psycopg          # bad: domain → infra
import redis            # bad: domain → infra

def place_order(conn: psycopg.Connection, r: redis.Redis, order: Order) -> None:
    with conn.cursor() as cur:
        cur.execute("INSERT INTO orders ...", (...,))
    r.set(f"order:{order.id}", order.to_json())
```

`pyproject.toml` for the project now needs `psycopg` and `redis` even
for unit tests. The domain cannot be tested without a live Postgres
and Redis (or invasive monkey-patching of the driver modules).
Replacing Redis with Memcached touches the domain package.

### Idiomatic fix: domain defines Protocols; adapters implement

```python
# src/myapp/domain/ports.py
from typing import Protocol
from .models import Order, OrderId

class OrderRepository(Protocol):
    async def insert(self, order: Order) -> None: ...

class OrderCache(Protocol):
    async def put(self, id: OrderId, order: Order) -> None: ...
```

```python
# src/myapp/domain/use_cases.py
from .ports import OrderRepository, OrderCache
from .models import Order

async def place_order(
    repo: OrderRepository,
    cache: OrderCache,
    order: Order,
) -> None:
    await repo.insert(order)
    await cache.put(order.id, order)
```

```python
# src/myapp/adapters/postgres.py
from typing import override
import psycopg
from myapp.domain.ports import OrderRepository      # good: adapter → domain
from myapp.domain.models import Order

class PostgresOrderRepository:
    def __init__(self, conn: psycopg.AsyncConnection) -> None:
        self._conn = conn

    @override
    async def insert(self, order: Order) -> None:
        async with self._conn.cursor() as cur:
            await cur.execute("INSERT INTO orders ...", (...,))
```

```python
# src/myapp/app/main.py
import asyncio, psycopg, redis.asyncio
from myapp.adapters.postgres import PostgresOrderRepository
from myapp.adapters.redis_cache import RedisOrderCache
from myapp.domain.use_cases import place_order

async def main() -> None:
    conn = await psycopg.AsyncConnection.connect(...)
    r = redis.asyncio.Redis(...)
    repo = PostgresOrderRepository(conn)
    cache = RedisOrderCache(r)
    # ... build an Order, then:
    await place_order(repo, cache, order)
```

The domain package depends only on the standard library. A fake
implementation of `OrderRepository` for tests fits in 10 lines —
no inheritance needed; structural typing covers it. The cache
provider can be swapped by replacing one line in `main`.

### Anti-pattern: domain function takes a concrete class from infra

```python
# src/myapp/domain/billing.py
from stripe import StripeClient     # bad

def charge(stripe: StripeClient, amount: Money) -> None:
    stripe.charges.create(amount=amount.cents)
```

Same problem in miniature: the domain module imports a third-party
SDK class.

### Idiomatic fix: Protocol + adapter

```python
# src/myapp/domain/billing.py
from typing import Protocol
from .money import Money

class PaymentGateway(Protocol):
    def charge(self, amount: Money) -> None: ...

def charge(gateway: PaymentGateway, amount: Money) -> None:
    gateway.charge(amount)
```

```python
# src/myapp/adapters/stripe_gateway.py
from typing import override
from stripe import StripeClient
from myapp.domain.billing import PaymentGateway
from myapp.domain.money import Money

class StripeGateway:
    def __init__(self, client: StripeClient) -> None:
        self._client = client

    @override
    def charge(self, amount: Money) -> None:
        self._client.charges.create(amount=amount.cents)
```

### Anti-pattern: Protocol defined in adapter package, imported by domain

```python
# src/myapp/adapters/storage.py
from typing import Protocol
class Storage(Protocol):
    def put(self, key: str, value: bytes) -> None: ...
```

```python
# src/myapp/domain/files.py
from myapp.adapters.storage import Storage   # bad: domain → adapters
```

The Protocol is in the wrong package. Even though it's "just a
Protocol", the domain now imports from the adapters layer; the
import-graph arrow points the wrong way.

### Idiomatic fix: move the Protocol to the domain

```python
# src/myapp/domain/ports.py
from typing import Protocol
class Storage(Protocol):
    def put(self, key: str, value: bytes) -> None: ...
```

```python
# src/myapp/adapters/s3_storage.py
from typing import override
from myapp.domain.ports import Storage       # good: adapter → domain

class S3Storage:
    @override
    def put(self, key: str, value: bytes) -> None: ...
```

### Anti-pattern: dependency injection via module globals

```python
# src/myapp/db.py
import os, psycopg
POOL = psycopg.ConnectionPool(os.environ["DATABASE_URL"])

# src/myapp/domain/users.py
from myapp.db import POOL                    # bad: domain → infra global

def create_user(name: str) -> None:
    with POOL.connection() as conn:
        conn.execute("INSERT INTO users ...", (name,))
```

This is DIP-shaped on paper (the function does not "take" a DB) but
in practice has all the same vices: tests must initialize the global;
the global is hard to swap; the dependency is invisible at the call
site; and the import graph still has `domain → db → psycopg`.

### Idiomatic fix: take what you need explicitly

```python
# src/myapp/domain/users.py
from typing import Protocol

class UserRepository(Protocol):
    def insert(self, name: str) -> None: ...

def create_user(repo: UserRepository, name: str) -> None:
    repo.insert(name)
```

Construct the concrete repository in `main` and pass it in. Make the
dependency visible.

## Protocols vs ABCs vs duck typing

Python gives you three flavours of DIP, each with trade-offs:

```python
# 1. typing.Protocol (structural). No inheritance required; any class
#    with matching methods satisfies it. Preferred default.
class OrderRepository(Protocol):
    async def insert(self, order: Order) -> None: ...

# 2. abc.ABC (nominal). Adapter must explicitly inherit. Useful when
#    you want isinstance() checks or a shared base implementation.
class OrderRepository(ABC):
    @abstractmethod
    async def insert(self, order: Order) -> None: ...

# 3. Untyped duck typing. Works, but the dependency contract lives
#    only in the docstring; type-checkers and IDEs cannot help you.
async def place_order(repo, cache, order): ...
```

Use Protocol by default — it composes well with PEP 695 generics
(`class Repository[T](Protocol): ...`), supports `@runtime_checkable`
when you really do need `isinstance`, and keeps adapters free to be
defined without referencing the domain at class-definition time.
Reach for `abc.ABC` only when you need a partial base implementation
or strict nominal subtyping. There is no LSP-style penalty either way;
DIP is honoured regardless.

## How code-ranker detects DIP violations

Code Ranker's package-level import graph is precisely the DIP arrow:

| Signal | DIP interpretation |
|---|---|
| `domain` package has outgoing import edges to adapter packages (e.g. `psycopg`, `redis`, `requests`, `boto3`) | Direct DIP violation. The domain depends on a detail. |
| `domain` package's `pyproject.toml` / requirements list I/O libraries | Same. |
| Protocol or ABC defined in an adapter package is imported from the domain package | Abstraction is in the wrong place. |
| Import-graph cycle between `domain` and an adapter package | Bidirectional dependency — DIP is bilaterally violated. |
| A `domain`-categorized package imports from an `adapters`/`app`/`script`-categorized package | Layer violation flag. Already covered by code-ranker's layer-violations report. |

Cross-references to existing code-ranker capabilities:

- The **layer-violations** view in the analysis report directly maps:
  "no domain package should import from an adapters/app/script package".
- A future **dip-protocol-leakage** rule could detect:
  "domain package imports a Protocol defined in an adapter package".
- The package-level SCC detector is already a strict DIP guard —
  domain ↔ adapter cycles are caught.

## Suggested recommendation template

> **DIP candidate**: package `myapp.domain` has an outgoing import edge
> to `psycopg`. The high-level (`domain`) is depending on a low-level
> detail (`psycopg`). Define an `OrderRepository` Protocol in
> `myapp.domain.ports`, move the Postgres-specific code to
> `myapp.adapters.postgres`, and let the adapter satisfy the Protocol.
> Wire the concrete `PostgresOrderRepository` only in the application
> entrypoint.
>
> Reference: Percival & Gregory, *Architecture Patterns with Python*,
> Chapter 3 (Repository) and Chapter 13 (Dependency Injection).

## DIP and dependency injection frameworks

Python has DI containers — `dependency-injector`, `injector`, `wired`,
`punq`, FastAPI's `Depends` — but explicit constructor injection
(`MyService(dep1, dep2, ...)`) remains the most common idiom and is
all DIP requires. The cost is one `__init__` per service; the benefit
is a fully visible dependency graph at the call site and an import
graph that a tool can analyse statically.

If you use a framework, the underlying principle is unchanged:
whatever the wiring mechanism, the goal is that the **app package**
is the only one that names the concrete types; everything else names
only Protocols. FastAPI's `Depends` is fine as long as the dependency
producer lives in the app/composition layer and the use-case
parameters are typed as Protocols.

## Hexagonal architecture in Python

Hexagonal architecture (Cockburn, 2005) is DIP applied at the
package-layout level: the domain sits in the centre and defines
"ports" (Protocols); "adapters" implement those ports on the outside;
the application assembles the hexagon by handing concrete adapters
to the domain. A typical Python layout:

```
src/myapp/
├── domain/           # pure: stdlib only
│   ├── models.py     # dataclasses
│   ├── ports.py      # Protocols
│   └── use_cases.py  # functions / services parameterised by ports
├── adapters/         # impure: third-party SDKs live here
│   ├── postgres.py
│   ├── redis_cache.py
│   └── stripe_gateway.py
└── app/              # composition root
    └── main.py       # imports both layers, wires them together
```

The import-graph invariants are: `domain` imports nothing from
`adapters` or `app`; `adapters` imports from `domain` only; `app`
imports from both. Code Ranker can enforce these as layer rules.

## Related principles

- [SRP](solid-single-responsibility.md) — defines what "a module" is;
  DIP says how modules connect.
- [OCP](solid-open-closed.md) — the Protocols DIP introduces are
  exactly the extension points OCP requires.
- [ISP](solid-interface-segregation.md) — make the Protocols small
  enough to be worth depending on; Python Protocols make per-consumer
  interfaces cheap.
- [Composition Over Inheritance](composition-over-inheritance.md)
  — DIP is the macro form of "compose with Protocols, don't inherit
  from concrete classes".
- [Acyclic Dependencies](acyclic-dependencies-principle.md) — DIP is
  the most common technique for breaking would-be cycles between a
  domain package and an infrastructure package.

Hexagonal Architecture (Cockburn, 2005) is the architecture-scale
instantiation of DIP; see the section above.

## References

1. Martin, R. C. "The Dependency Inversion Principle". 1996.
   <https://web.archive.org/web/20110714224327/http://www.objectmentor.com/resources/articles/dip.pdf>
2. Martin, R. C. *Clean Architecture*. Ch. 11.
3. Seemann, M. *Dependency Injection in .NET*. Manning, 2011.
4. Cockburn, A. "Hexagonal Architecture", 2005.
   <https://alistair.cockburn.us/hexagonal-architecture/>
5. Percival, H. & Gregory, B. *Architecture Patterns with Python*.
   O'Reilly, 2020. <https://www.cosmicpython.com/>
6. PEP 544 — Protocols: structural subtyping.
   <https://peps.python.org/pep-0544/>
7. PEP 695 — Type Parameter Syntax.
   <https://peps.python.org/pep-0695/>
