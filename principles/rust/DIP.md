# DIP — Dependency Inversion Principle (in Rust)

**TL;DR**: High-level modules should not depend on low-level modules;
both should depend on **abstractions**. Abstractions should not depend
on details. In Rust this becomes: domain crates define traits;
infrastructure crates implement them; the application wires concrete
types in at the composition root.

## Canonical sources

- Robert C. Martin, "The Dependency Inversion Principle" (1996):
  <https://web.archive.org/web/20110714224327/http://www.objectmentor.com/resources/articles/dip.pdf>
- Martin, *Clean Architecture*, Ch. 11.
- Mark Seemann, *Dependency Injection in .NET*, Manning (2011).
  Concepts apply to Rust 1:1.
- Alex Pusch, "Master Hexagonal Architecture in Rust":
  <https://www.howtocodeit.com/guides/master-hexagonal-architecture-in-rust>
- matklad, "Large Rust Workspaces" — flat workspace layout where
  `app` is the only crate that imports infra crates.
  <https://matklad.github.io/2021/08/22/large-rust-workspaces.html>
- niko matsakis, "Dyn dispatch design notes":
  <https://smallcultfollowing.com/babysteps/blog/2022/01/07/dyn-async-traits-part-7/>

## The principle

The literal rule:

1. High-level modules should not depend on low-level modules. Both
   should depend on abstractions.
2. Abstractions should not depend on details. Details should depend
   on abstractions.

Concretely: if `domain` orchestrates business rules and `postgres`
implements storage, the `Cargo.toml` arrow should run from
`postgres → domain` (postgres implements a trait defined in domain),
**not** `domain → postgres` (domain calls postgres functions directly).
The dependency arrow at the compilation level is inverted from the
flow of control.

This is the principle behind:

- Hexagonal Architecture / Ports & Adapters (Cockburn, 2005)
- Onion Architecture (Palermo, 2008)
- Clean Architecture (Martin, 2012)

All three are the same idea: *the domain owns the interfaces, the
infrastructure owns the implementations*.

## Why it matters

When the high-level depends on the low-level:

- **Replaceability** disappears. Want to swap Postgres for SQLite?
  Now you change every `use postgres::*` in `domain`.
- **Testability** disappears. The domain cannot be unit-tested
  without bringing up Postgres (or mocking the SQL library, which
  is harder than mocking your own trait).
- **Layering** disappears. The "domain" crate links to a database
  driver, defeating the purpose of a separate domain crate.
- **Compilation time** explodes. Every infrastructure crate becomes
  a transitive dependency of the domain.

In a Rust workspace, the DIP arrow shows up in the **crate graph**.
A workspace passes DIP when `domain` has no incoming infrastructure
dependencies and outgoing ones flow through traits the domain owns.

## In Rust

The Rust toolchain makes DIP particularly clean because traits are
the natural abstraction:

```
┌──────────────────────────────────┐
│ application (composition root)   │  ← only this crate sees all
│  - main()                        │     concrete types
│  - wires PostgresRepo into       │
│    use-cases                     │
└────────┬─────────────────────────┘
         │ depends on (concrete)
         ▼
┌──────────────────────────────────┐    ┌──────────────────────────────────┐
│ infra-postgres                   │    │ infra-redis                      │
│  - struct PostgresRepo           │    │  - struct RedisCache              │
│  - impl Repository for PostgresR │    │  - impl Cache for RedisCache      │
└────────┬─────────────────────────┘    └────────┬─────────────────────────┘
         │ implements trait                       │ implements trait
         ▼                                        ▼
┌──────────────────────────────────────────────────────────────────────────┐
│ domain (the centre)                                                       │
│  - trait Repository                                                       │
│  - trait Cache                                                            │
│  - struct User, struct OrderId, ...                                       │
│  - use-cases that take `R: Repository, C: Cache`                          │
└──────────────────────────────────────────────────────────────────────────┘
```

The domain crate has **zero infrastructure dependencies**. It compiles
without a database, a network stack, or a clock. Tests live next to
use-cases and inject fake `impl Repository` / `impl Cache`.

## Violations and remedies

### Anti-pattern: domain calls infrastructure directly

```rust
// crates/domain/src/order_service.rs
use postgres::Client;            // bad: domain → infra
use redis::Connection;           // bad: domain → infra

pub async fn place_order(client: &Client, redis: &mut Connection, order: Order) -> Result<()> {
    client.execute("INSERT INTO orders ...", &[]).await?;
    redis.set(format!("order:{}", order.id), &order.serialize())?;
    Ok(())
}
```

`crates/domain/Cargo.toml` now pulls `postgres`, `redis`, `tokio`,
and everything they transitively need. The domain cannot be tested
without spinning up real services. Replacing Redis with Memcached
touches the domain crate.

### Idiomatic fix: domain defines traits; infra implements

```rust
// crates/domain/src/lib.rs
pub trait OrderRepository: Send + Sync {
    fn insert(&self, o: &Order) -> impl Future<Output = Result<()>> + Send;
}
pub trait OrderCache: Send + Sync {
    fn put(&self, id: OrderId, o: &Order) -> impl Future<Output = Result<()>> + Send;
}

// Use-case: generic over the traits, knows nothing about Postgres/Redis.
pub async fn place_order<R: OrderRepository, C: OrderCache>(
    repo: &R, cache: &C, order: Order
) -> Result<()> {
    repo.insert(&order).await?;
    cache.put(order.id, &order).await?;
    Ok(())
}
```

```rust
// crates/infra-postgres/src/lib.rs
use domain::{Order, OrderRepository};       // good: infra → domain
pub struct PostgresOrderRepository { /* ... */ }
impl OrderRepository for PostgresOrderRepository { /* ... */ }
```

```rust
// crates/app/src/main.rs
fn main() {
    let repo = PostgresOrderRepository::new();
    let cache = RedisOrderCache::new();
    let use_case = OrderUseCase::new(repo, cache);
    // serve ...
}
```

`crates/domain/Cargo.toml` lists only `serde` and `thiserror`.
A fake implementation of `OrderRepository` for tests fits in 10
lines. The cache provider can be swapped by replacing one line in
`main`.

### Anti-pattern: domain function takes a concrete struct from infra

```rust
// crates/domain/src/billing.rs
use infra::stripe::Client;     // bad

pub fn charge(stripe: &Client, amount: Money) -> Result<()> {
    stripe.charge(amount.to_cents())?;
    Ok(())
}
```

Same problem in miniature.

### Idiomatic fix: trait + adapter

```rust
// crates/domain/src/billing.rs
pub trait PaymentGateway {
    fn charge(&self, amount: Money) -> Result<()>;
}

pub fn charge<G: PaymentGateway>(g: &G, amount: Money) -> Result<()> {
    g.charge(amount)
}
```

```rust
// crates/infra-stripe/src/lib.rs
pub struct StripeGateway { client: stripe::Client }
impl PaymentGateway for StripeGateway {
    fn charge(&self, amount: Money) -> Result<()> {
        self.client.charge(amount.to_cents()).map(|_| ()).map_err(|e| e.into())
    }
}
```

### Anti-pattern: trait defined in infra crate, imported by domain

```rust
// crates/infra-storage/src/lib.rs
pub trait Storage { fn put(...); }
```

```rust
// crates/domain/src/lib.rs
use infra_storage::Storage;     // bad: domain depends on infra crate
```

The trait is in the wrong place. Even though it's "just a trait", the
domain crate now compiles against the infra crate.

### Idiomatic fix: move the trait to the domain

```rust
// crates/domain/src/lib.rs
pub trait Storage { fn put(...); }
```

```rust
// crates/infra-storage/src/lib.rs
use domain::Storage;            // good: infra implements the abstraction
pub struct PostgresStorage;
impl Storage for PostgresStorage { /* ... */ }
```

### Anti-pattern: dependency injection via globals or `lazy_static`

```rust
lazy_static! {
    static ref DB: Pool = Pool::new(env::var("DATABASE_URL").unwrap());
}

pub fn create_user(...) -> Result<()> {
    DB.execute("INSERT INTO users ...")?;
    Ok(())
}
```

This is DIP-shaped on paper (the function does not "take" a DB) but
in practice has all the same vices: tests must initialize the global;
the global is hard to swap; the dependency is invisible at the
call site.

### Idiomatic fix: take what you need explicitly

```rust
pub fn create_user<R: UserRepository>(repo: &R, ...) -> Result<()> {
    repo.insert(...)?;
    Ok(())
}
```

Pass the concrete pool from `main`. Make the dependency visible.

## Dispatch choices: `impl Trait` vs `&dyn Trait` vs generic

Rust gives you three flavours of DIP, each with trade-offs:

```rust
// 1. Generic (monomorphized). Zero-cost; one copy per concrete type.
fn create_user<R: UserRepository>(repo: &R, ...) -> Result<()>

// 2. impl Trait in argument position. Same as generic, less typing.
fn create_user(repo: &impl UserRepository, ...) -> Result<()>

// 3. Trait object. One vtable call per method; works in any return
//    or container position.
fn create_user(repo: &dyn UserRepository, ...) -> Result<()>
```

Use generics when monomorphization is OK (small number of impls,
small bodies). Use trait objects when storing implementations
heterogeneously (a `Vec<Box<dyn Renderer>>`) or when avoiding
generics for compile time. There is no LSP-style penalty either way;
DIP is honoured regardless.

## How code-ranker detects DIP violations

Code Ranker's crate-level graph is precisely the DIP arrow:

| Signal | DIP interpretation |
|---|---|
| `domain` crate has outgoing `Uses` edges to infra crates (e.g. `tokio-postgres`, `redis`, `reqwest`) | Direct DIP violation. The domain depends on a detail. |
| `domain` crate's `Cargo.toml` lists I/O crates | Same. |
| Trait defined in infra crate is `use`d from domain crate | Trait is in the wrong place. |
| Crate-level cycle between `domain` and an infra crate | Bidirectional dependency — DIP is bilaterally violated. |
| `lib`-categorized crate depends on `module`/`app`/`example`-categorized crate | Layer violation flag. Already covered by code-ranker's layer-violations report. |

Cross-references to existing code-ranker capabilities:

- The **layer-violations** view in the analysis report directly maps:
  "no lib should depend on a module/app/example crate".
- A future **dip-trait-leakage** rule could detect:
  "domain crate uses a trait defined in an infra crate".
- The crate-level SCC detector is already a strict DIP guard.

## Suggested recommendation template

> **DIP candidate**: crate `domain` has an outgoing `Uses` edge to
> `tokio-postgres`. The high-level (`domain`) is depending on a
> low-level detail (`tokio-postgres`). Define a `Repository` trait in
> `domain`, move the Postgres-specific code to `infra-postgres`, and
> let `infra-postgres` implement `domain::Repository`. Wire the
> concrete `PostgresRepository` only in the application crate.
>
> Reference: <https://www.howtocodeit.com/guides/master-hexagonal-architecture-in-rust>

## DIP and dependency injection frameworks

Rust has no widely-adopted DI container (no Spring, no Dagger). This
is intentional: explicit constructor injection — `MyService::new(dep1, dep2, ...)`
— is the idiomatic approach. The cost is one constructor per service;
the benefit is full visibility of the dependency graph at compile
time.

Some crates offer wirable patterns (shaku, dilib, axum's `FromRef`);
the underlying principle is unchanged. Whatever framework you use,
the goal is: the **app crate** has the concrete types; everything
else has the abstractions.

## Related principles

- [SRP](SRP.md) — defines what "a module" is;
  DIP says how modules connect.
- [OCP](OCP.md) — the traits DIP introduces are
  exactly the extension points OCP requires.
- [ISP](ISP.md) — make the abstractions
  small enough to be worth depending on.
- [Composition Over Inheritance](CoI.md)
  — DIP is the macro form of "compose with traits, don't inherit
  from concretes".
- Hexagonal Architecture (Cockburn) — the
  architecture-scale instantiation of DIP.

## References

1. Martin, R. C. "The Dependency Inversion Principle". 1996.
   <https://web.archive.org/web/20110714224327/http://www.objectmentor.com/resources/articles/dip.pdf>
2. Martin, R. C. *Clean Architecture*. Ch. 11.
3. Seemann, M. *Dependency Injection in .NET*. Manning, 2011.
4. Pusch, A. "Master Hexagonal Architecture in Rust".
   <https://www.howtocodeit.com/guides/master-hexagonal-architecture-in-rust>
5. matklad. "Large Rust Workspaces". 2021.
   <https://matklad.github.io/2021/08/22/large-rust-workspaces.html>
6. Cockburn, A. "Hexagonal Architecture", 2005.
   <https://alistair.cockburn.us/hexagonal-architecture/>
