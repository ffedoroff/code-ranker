# DIP — Dependency Inversion Principle

**TL;DR**: High-level modules should not depend on low-level modules;
both should depend on **abstractions**. Abstractions should not depend
on details. In practice this becomes: domain packages define
interfaces; infrastructure packages implement them; the application
wires concrete types in at the composition root.

## Canonical sources

- Robert C. Martin, "The Dependency Inversion Principle" (1996):
  <https://web.archive.org/web/20110714224327/http://www.objectmentor.com/resources/articles/dip.pdf>
- Martin, *Clean Architecture*, Ch. 11.
- Mark Seemann, *Dependency Injection in .NET*, Manning (2011).
  Concepts apply to any language.
- Alistair Cockburn, "Hexagonal Architecture" (2005):
  <https://alistair.cockburn.us/hexagonal-architecture/>

## The principle

The literal rule:

1. High-level modules should not depend on low-level modules. Both
   should depend on abstractions.
2. Abstractions should not depend on details. Details should depend
   on abstractions.

Concretely: if `domain` orchestrates business rules and `postgres`
implements storage, the dependency arrow should run from
`postgres → domain` (postgres implements an interface defined in
domain), **not** `domain → postgres` (domain calls postgres functions
directly). The dependency arrow at the compilation level is inverted
from the flow of control.

This is the principle behind:

- Hexagonal Architecture / Ports & Adapters (Cockburn, 2005)
- Onion Architecture (Palermo, 2008)
- Clean Architecture (Martin, 2012)

All three are the same idea: *the domain owns the interfaces, the
infrastructure owns the implementations*.

## Why it matters

When the high-level depends on the low-level:

- **Replaceability** disappears. Want to swap Postgres for SQLite?
  Now you change every reference to postgres in `domain`.
- **Testability** disappears. The domain cannot be unit-tested
  without bringing up Postgres (or mocking the SQL library, which
  is harder than mocking your own interface).
- **Layering** disappears. The "domain" package links to a database
  driver, defeating the purpose of a separate domain package.
- **Build time** explodes. Every infrastructure package becomes
  a transitive dependency of the domain.

In a codebase, the DIP arrow shows up in the **package graph**.
A project passes DIP when `domain` has no incoming infrastructure
dependencies and outgoing ones flow through interfaces the domain
owns.

## Applying it

Interfaces are the natural abstraction for DIP:

```
┌──────────────────────────────────┐
│ application (composition root)   │  ← only this package sees all
│  - main()                        │     concrete types
│  - wires PostgresRepo into       │
│    use-cases                     │
└────────┬─────────────────────────┘
         │ depends on (concrete)
         ▼
┌──────────────────────────────────┐    ┌──────────────────────────────────┐
│ infra-postgres                   │    │ infra-redis                      │
│  - type PostgresRepo             │    │  - type RedisCache               │
│  - PostgresRepo: Repository      │    │  - RedisCache: Cache             │
└────────┬─────────────────────────┘    └────────┬─────────────────────────┘
         │ implements interface                   │ implements interface
         ▼                                        ▼
┌──────────────────────────────────────────────────────────────────────────┐
│ domain (the centre)                                                       │
│  - interface Repository                                                   │
│  - interface Cache                                                        │
│  - type User, type OrderId, ...                                           │
│  - use-cases that take (repo: Repository, cache: Cache)                   │
└──────────────────────────────────────────────────────────────────────────┘
```

The domain package has **zero infrastructure dependencies**. It builds
without a database, a network stack, or a clock. Tests live next to
use-cases and inject fake implementations of `Repository` / `Cache`.

## Violations and remedies

### Anti-pattern: domain calls infrastructure directly

```
// domain/order_service
import postgres.Client            // bad: domain → infra
import redis.Connection           // bad: domain → infra

function place_order(client: Client, redis: Connection, order: Order) -> Result {
    client.execute("INSERT INTO orders ...")
    redis.set("order:" + order.id, order.serialize())
}
```

The domain package now pulls `postgres`, `redis`, and everything they
transitively need. The domain cannot be tested without spinning up
real services. Replacing Redis with Memcached touches the domain
package.

### Idiomatic fix: domain defines interfaces; infra implements

```
// domain
public interface OrderRepository {
    insert(o: Order) -> Result
}
public interface OrderCache {
    put(id: OrderId, o: Order) -> Result
}

// Use-case: generic over the interfaces, knows nothing about Postgres/Redis.
function place_order(repo: OrderRepository, cache: OrderCache, order: Order) -> Result {
    repo.insert(order)
    cache.put(order.id, order)
}
```

```
// infra-postgres
import domain.{Order, OrderRepository}       // good: infra → domain
public type PostgresOrderRepository { /* ... */ }
// PostgresOrderRepository implements OrderRepository
```

```
// app/main
function main() {
    repo = PostgresOrderRepository.new()
    cache = RedisOrderCache.new()
    use_case = OrderUseCase.new(repo, cache)
    // serve ...
}
```

The domain package depends only on serialization and error-handling
helpers. A fake implementation of `OrderRepository` for tests fits in
10 lines. The cache provider can be swapped by replacing one line in
`main`.

### Anti-pattern: domain function takes a concrete type from infra

```
// domain/billing
import infra.stripe.Client     // bad

function charge(stripe: Client, amount: Money) -> Result {
    stripe.charge(amount.to_cents())
}
```

Same problem in miniature.

### Idiomatic fix: interface + adapter

```
// domain/billing
public interface PaymentGateway {
    charge(amount: Money) -> Result
}

function charge(g: PaymentGateway, amount: Money) -> Result {
    g.charge(amount)
}
```

```
// infra-stripe
public type StripeGateway { client: stripe.Client }
// StripeGateway implements PaymentGateway:
//   charge(amount) -> this.client.charge(amount.to_cents())
```

### Anti-pattern: interface defined in infra package, imported by domain

```
// infra-storage
public interface Storage { put(...) }
```

```
// domain
import infra_storage.Storage     // bad: domain depends on infra package
```

The interface is in the wrong place. Even though it's "just an
interface", the domain package now builds against the infra package.

### Idiomatic fix: move the interface to the domain

```
// domain
public interface Storage { put(...) }
```

```
// infra-storage
import domain.Storage            // good: infra implements the abstraction
public type PostgresStorage
// PostgresStorage implements Storage
```

### Anti-pattern: dependency injection via globals

```
global DB = Pool.new(env("DATABASE_URL"))

function create_user(...) -> Result {
    DB.execute("INSERT INTO users ...")
}
```

This is DIP-shaped on paper (the function does not "take" a DB) but
in practice has all the same vices: tests must initialize the global;
the global is hard to swap; the dependency is invisible at the
call site.

### Idiomatic fix: take what you need explicitly

```
function create_user(repo: UserRepository, ...) -> Result {
    repo.insert(...)
}
```

Pass the concrete pool from `main`. Make the dependency visible.

## Dispatch choices: static vs dynamic

You typically get two flavours of DIP, each with trade-offs:

```
// 1. Static dispatch (generic / monomorphized).
//    Zero-cost; one specialized copy per concrete type.
function create_user(repo: R where R: UserRepository, ...) -> Result

// 2. Dynamic dispatch (interface object).
//    One indirect call per method; works in any return or container
//    position, e.g. a list of mixed implementations.
function create_user(repo: dynamic UserRepository, ...) -> Result
```

Use static dispatch when specialization is OK (small number of
implementations, small bodies). Use dynamic dispatch when storing
implementations heterogeneously (a `list<Renderer>` of different
concrete types) or when avoiding specialization for build time. There
is no LSP-style penalty either way; DIP is honoured regardless.

## How code-ranker detects DIP violations

Code Ranker's package-level graph is precisely the DIP arrow:

| Signal | DIP interpretation |
|---|---|
| `domain` package has outgoing `Uses` edges to infra packages (e.g. a Postgres driver, Redis client, HTTP client) | Direct DIP violation. The domain depends on a detail. |
| `domain` package's dependency config lists I/O packages | Same. |
| Interface defined in an infra package is used from the domain package | Interface is in the wrong place. |
| Package-level cycle between `domain` and an infra package | Bidirectional dependency — DIP is bilaterally violated. |
| `lib`-categorized package depends on a `module`/`app`/`example`-categorized package | Layer violation flag. Already covered by code-ranker's layer-violations report. |

Cross-references to existing code-ranker capabilities:

- The **layer-violations** view in the analysis report directly maps:
  "no lib should depend on a module/app/example package".
- A future **dip-interface-leakage** rule could detect:
  "domain package uses an interface defined in an infra package".
- The package-level SCC detector is already a strict DIP guard.

## Suggested recommendation template

> **DIP candidate**: package `domain` has an outgoing `Uses` edge to
> a Postgres driver package. The high-level (`domain`) is depending on
> a low-level detail. Define a `Repository` interface in `domain`,
> move the Postgres-specific code to `infra-postgres`, and let
> `infra-postgres` implement `domain.Repository`. Wire the concrete
> `PostgresRepository` only in the application package.
>
> Reference: <https://alistair.cockburn.us/hexagonal-architecture/>

## DIP and dependency injection frameworks

Not every ecosystem has a widely-adopted DI container. Where one is
absent, explicit constructor injection — `MyService.new(dep1, dep2, ...)`
— is the idiomatic approach. The cost is one constructor per service;
the benefit is full visibility of the dependency graph at build time.

Some libraries offer wirable patterns (DI containers, framework state
extractors); the underlying principle is unchanged. Whatever framework
you use, the goal is: the **app package** has the concrete types;
everything else has the abstractions.

## Related principles

- [SRP](SRP.md) — defines what "a module" is;
  DIP says how modules connect.
- [OCP](OCP.md) — the interfaces DIP introduces are
  exactly the extension points OCP requires.
- [ISP](ISP.md) — make the abstractions
  small enough to be worth depending on.
- [Composition Over Inheritance](CoI.md)
  — DIP is the macro form of "compose with interfaces, don't inherit
  from concretes".
- Hexagonal Architecture (Cockburn) — the
  architecture-scale instantiation of DIP.

## References

1. Martin, R. C. "The Dependency Inversion Principle". 1996.
   <https://web.archive.org/web/20110714224327/http://www.objectmentor.com/resources/articles/dip.pdf>
2. Martin, R. C. *Clean Architecture*. Ch. 11.
3. Seemann, M. *Dependency Injection in .NET*. Manning, 2011.
4. Cockburn, A. "Hexagonal Architecture", 2005.
   <https://alistair.cockburn.us/hexagonal-architecture/>
