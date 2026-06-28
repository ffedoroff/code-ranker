# DIP — Dependency Inversion Principle (in TypeScript)

**TL;DR**: High-level modules should not depend on low-level modules;
both should depend on **abstractions**. Abstractions should not depend
on details. In TypeScript this becomes: domain packages declare
`interface`s (or `type` aliases for behaviour); infrastructure
packages implement them; the application wires the concrete classes
in at the composition root. The `import` arrow in a `.ts` file *is*
the dependency arrow — if a domain file says
`import { Pg } from "./infra/pg"`, you have already lost.

## Canonical sources

- Robert C. Martin, "The Dependency Inversion Principle" (1996):
  <https://web.archive.org/web/20110714224327/http://www.objectmentor.com/resources/articles/dip.pdf>
- Martin, *Clean Architecture*, Ch. 11.
- Alistair Cockburn, "Hexagonal Architecture" (2005):
  <https://alistair.cockburn.us/hexagonal-architecture/>
- Mark Seemann, *Dependency Injection: Principles, Practices, and Patterns*, Manning.
  The .NET examples translate to TypeScript with very little friction.
- Khalil Stemmler, "Domain-Driven Hexagonal Architecture" essays:
  <https://khalilstemmler.com/articles/software-design-architecture/organizing-app-logic/>
  and the broader DDD/clean-architecture series at
  <https://khalilstemmler.com/articles/>.
- `dependency-cruiser` docs: <https://github.com/sverweij/dependency-cruiser>
- `eslint-plugin-boundaries`:
  <https://github.com/javierbrea/eslint-plugin-boundaries>
- TypeScript handbook on project references and `paths`:
  <https://www.typescriptlang.org/docs/handbook/project-references.html>

## The principle

The literal rule:

1. High-level modules should not depend on low-level modules. Both
   should depend on abstractions.
2. Abstractions should not depend on details. Details should depend
   on abstractions.

Concretely: if `@app/domain` orchestrates business rules and
`@app/infra-pg` implements storage, the `package.json` arrow — and
the `import` arrow inside the code — should run from
`infra-pg → domain` (infra implements an interface defined in
domain), **not** `domain → infra-pg`. The dependency arrow at the
*module-graph* level is inverted from the runtime flow of control.

This is the principle behind:

- Hexagonal Architecture / Ports & Adapters (Cockburn, 2005)
- Onion Architecture (Palermo, 2008)
- Clean Architecture (Martin, 2012)

All three are the same idea: *the domain owns the interfaces, the
infrastructure owns the implementations*. See
Hexagonal Architecture (Cockburn) for the
architecture-scale instantiation.

## Why it matters

When the high-level depends on the low-level:

- **Replaceability** disappears. Want to swap Postgres for SQLite, or
  the `pg` driver for `postgres.js`? You change every
  `import ... from "pg"` in the domain.
- **Testability** disappears. The domain cannot be unit-tested in
  Vitest/Jest without booting a real database, or jamming the module
  graph with `vi.mock("pg", ...)`. Mocking your own interface is
  always cheaper than mocking somebody else's module.
- **Layering** disappears. The "domain" package transitively pulls
  Node-only modules (`fs`, `net`), which is fatal if you also want
  to run the domain in the browser, in an edge runtime, or in a
  React Server Component.
- **Bundle size and cold start** balloon. Every infra dependency
  becomes a transitive dep of the domain — and of every consumer of
  the domain.

In a monorepo (pnpm / Nx / Turborepo), DIP shows up in the
**package graph**. A workspace passes DIP when `@app/domain` has no
incoming infrastructure dependencies and outgoing ones flow through
interfaces the domain owns.

## In TypeScript

```
┌──────────────────────────────────┐
│ @app/composition (entrypoint)    │  ← only this package sees all
│  - main.ts                       │     concrete classes
│  - wires PgUserRepo into         │
│    use-cases                     │
└────────┬─────────────────────────┘
         │ imports (concrete)
         ▼
┌──────────────────────────────────┐    ┌──────────────────────────────────┐
│ @app/infra-pg                    │    │ @app/infra-redis                  │
│  - class PgUserRepo              │    │  - class RedisCache               │
│  - implements UserRepo           │    │  - implements Cache               │
└────────┬─────────────────────────┘    └────────┬─────────────────────────┘
         │ implements interface                   │ implements interface
         ▼                                        ▼
┌──────────────────────────────────────────────────────────────────────────┐
│ @app/domain (the centre)                                                  │
│  - interface UserRepo                                                     │
│  - interface Cache                                                        │
│  - type User, type OrderId, ...                                           │
│  - use-cases that take `UserRepo`, `Cache`                                │
└──────────────────────────────────────────────────────────────────────────┘
```

`@app/domain` has **zero infrastructure dependencies**. Its
`package.json` lists only `zod` (or no runtime deps at all). Its
`tsconfig.json` `lib` setting can be `["ES2022"]` — no `"dom"`, no
`"node"` — because nothing in the domain touches a platform.

## Violations and remedies

### Anti-pattern: domain calls infrastructure directly

```ts
// packages/domain/src/orderService.ts
import { Pool } from "pg";                  // bad: domain → infra
import { Redis } from "ioredis";            // bad: domain → infra

export async function placeOrder(
  pg: Pool, redis: Redis, order: Order,
): Promise<void> {
  await pg.query("INSERT INTO orders ...", [order.id]);
  await redis.set(`order:${order.id}`, JSON.stringify(order));
}
```

`packages/domain/package.json` now lists `pg` and `ioredis`. The
domain cannot be tested without spinning up real services. Swapping
Redis for Memcached touches the domain package.

### Idiomatic fix: domain defines interfaces; infra implements

```ts
// packages/domain/src/ports.ts
export interface OrderRepository {
  insert(o: Order): Promise<void>;
}
export interface OrderCache {
  put(id: OrderId, o: Order): Promise<void>;
}

// packages/domain/src/placeOrder.ts
export async function placeOrder(
  repo: OrderRepository,
  cache: OrderCache,
  order: Order,
): Promise<void> {
  await repo.insert(order);
  await cache.put(order.id, order);
}
```

```ts
// packages/infra-pg/src/pgOrderRepository.ts
import type { OrderRepository, Order } from "@app/domain";   // good
import { Pool } from "pg";

export class PgOrderRepository implements OrderRepository {
  constructor(private readonly pool: Pool) {}
  async insert(o: Order): Promise<void> {
    await this.pool.query("INSERT INTO orders ...", [o.id]);
  }
}
```

```ts
// packages/composition/src/main.ts
const repo  = new PgOrderRepository(new Pool({ /* ... */ }));
const cache = new RedisOrderCache(new Redis(/* ... */));
await placeOrder(repo, cache, order);
```

A fake `OrderRepository` for tests fits in five lines:

```ts
const fakeRepo: OrderRepository = { insert: vi.fn().mockResolvedValue(undefined) };
```

### Anti-pattern: interface defined in infra package, imported by domain

```ts
// packages/infra-storage/src/index.ts
export interface Storage { put(key: string, v: Uint8Array): Promise<void> }
```

```ts
// packages/domain/src/file.ts
import type { Storage } from "@app/infra-storage";   // bad
```

Even though `Storage` is "just an interface" and `import type` is
erased at runtime, the domain `package.json` still declares the
infra package as a dependency, and a `tsc --build` of the domain
package now needs the infra package compiled. The interface is in
the wrong place.

### Idiomatic fix: move the interface to the domain

```ts
// packages/domain/src/ports.ts
export interface Storage { put(key: string, v: Uint8Array): Promise<void> }
```

```ts
// packages/infra-storage/src/s3Storage.ts
import type { Storage } from "@app/domain";          // good
export class S3Storage implements Storage { /* ... */ }
```

### Anti-pattern: dependency injection via module-level singletons

```ts
// packages/domain/src/db.ts
import { Pool } from "pg";
export const db = new Pool({ connectionString: process.env.DATABASE_URL });

// packages/domain/src/createUser.ts
import { db } from "./db";
export async function createUser(u: User) {
  await db.query("INSERT INTO users ...", [u.id]);
}
```

This is DIP-shaped on paper (the function does not "take" a DB) but
in practice has the same vices: tests must mutate or mock the
module, the singleton is hard to swap per-request, and the
dependency is invisible at the call site. It also breaks in any
runtime where a top-level `new Pool()` would fire before
configuration is loaded (Lambda, Workers, RSC).

### Idiomatic fix: take what you need explicitly

```ts
export async function createUser(repo: UserRepository, u: User) {
  await repo.insert(u);
}
```

Construct the concrete pool in `@app/composition` and pass the
adapter down. Make the dependency visible.

### Anti-pattern: domain function takes a concrete class from infra

```ts
// packages/domain/src/billing.ts
import { Stripe } from "stripe";              // bad

export async function charge(stripe: Stripe, amount: Money) {
  await stripe.charges.create({ amount: amount.toCents(), currency: "usd" });
}
```

Same problem in miniature.

### Idiomatic fix: port + adapter

```ts
// packages/domain/src/billing.ts
export interface PaymentGateway {
  charge(amount: Money): Promise<void>;
}

export const charge = (g: PaymentGateway, amount: Money) => g.charge(amount);
```

```ts
// packages/infra-stripe/src/stripeGateway.ts
import type { PaymentGateway, Money } from "@app/domain";
import Stripe from "stripe";

export class StripeGateway implements PaymentGateway {
  constructor(private readonly stripe: Stripe) {}
  charge(amount: Money) {
    return this.stripe.charges
      .create({ amount: amount.toCents(), currency: "usd" })
      .then(() => undefined);
  }
}
```

## React: callbacks invert deps from child to parent

DIP isn't only a backend concern. In React, a prop-typed callback
inverts the dependency arrow at the component boundary. A `<Button>`
that *imports* `useRouter` to navigate has a `Button → router`
dependency. A `<Button onClick={...} />` whose parent supplies the
handler has the arrow reversed: the parent depends on the button's
interface (its props), and the button depends only on the prop
types it declares.

```tsx
// bad: Button knows about the router (and the route names!).
import { useRouter } from "next/navigation";
export function Button({ to, children }: { to: string; children: ReactNode }) {
  const router = useRouter();
  return <button onClick={() => router.push(to)}>{children}</button>;
}

// good: Button knows nothing about navigation.
export function Button(props: { onClick: () => void; children: ReactNode }) {
  return <button onClick={props.onClick}>{props.children}</button>;
}
```

The good version is testable without a router, reusable outside
Next, and reachable from a Storybook story without mocks.

## React Server Components & Server Actions

The RSC boundary is a DIP boundary that the compiler enforces. A
file with `"use client"` cannot import a server-only module; a
file with `"use server"` exports actions that clients consume by
*name*, not by importing their bodies. The arrow runs from client
boundary → server action signature, while the server's
implementation depends only on the action's type. Treat server
actions as ports: define them in the domain layer (with input/output
schemas in zod), import them where called, and let the runtime wire
the rest. Do not let a client component import anything from
`@app/infra-*`.

## Enforcing the arrow

TypeScript does not enforce package layering on its own. Pair these:

- **`tsconfig.json` `paths` / project references.** Map
  `@app/domain` to `packages/domain/src` and forbid the inverse path.
  With project references, `@app/domain` builds in isolation; if a
  domain file `import`s `@app/infra-pg`, `tsc --build` fails because
  the reference doesn't exist.
- **`dependency-cruiser`.** Add a `forbidden` rule:

  ```js
  // .dependency-cruiser.cjs
  module.exports = {
    forbidden: [
      {
        name: "no-domain-to-infra",
        severity: "error",
        from: { path: "^packages/domain" },
        to:   { path: "^packages/(infra-|adapters-)" },
      },
    ],
  };
  ```

- **`eslint-plugin-boundaries`.** Tag each package as `domain`,
  `infra`, or `app` and declare allowed targets per tag. Runs in
  the editor; catches violations as you type.
- **`madge --circular`** in CI to catch the related sin of cycles
  between domain and infra packages.

If none of these are in place, DIP is a convention; it will be
violated by the next person to land a feature on a Friday.

## DI containers vs plain constructor injection

TypeScript has InversifyJS, tsyringe, NestJS's DI, awilix, typedi.
They all work. None are required, and none replace the principle.

**Prefer plain constructor injection** in new code:

```ts
class OrderService {
  constructor(
    private readonly repo: OrderRepository,
    private readonly cache: OrderCache,
    private readonly clock: Clock,
  ) {}
}
```

The composition root (`main.ts`, `server.ts`, a Next route handler
factory) is the only place that knows the concrete classes:

```ts
const service = new OrderService(
  new PgOrderRepository(pool),
  new RedisOrderCache(redis),
  new SystemClock(),
);
```

Reach for a container only when wiring becomes painful at scale
(dozens of services, multiple lifecycles, request-scoped
dependencies). Even then, the container is a *convenience*, not the
principle. If using NestJS, prefer `@Inject(TOKEN)` against an
interface token rather than a concrete class — otherwise the
`providers` array becomes a stealthy `domain → infra` import.

## Dispatch choices: `interface` vs `type` vs class with abstract methods

TypeScript offers three flavours of "the abstraction":

```ts
// 1. interface — the default. Structural, declaration-mergeable, free at runtime.
interface UserRepo { insert(u: User): Promise<void> }

// 2. type alias — same as interface for record types; no declaration merging.
type UserRepo = { insert(u: User): Promise<void> };

// 3. abstract class — interface + a shared implementation hook. Carries a
//    runtime constructor; useful when you want `instanceof` checks.
abstract class UserRepo { abstract insert(u: User): Promise<void> }
```

Use `interface` for ports unless you need shared implementation. The
DIP arrow is honoured regardless; the choice is about
declaration-merging and runtime presence, not about layering.

## Suggested recommendation template

> **DIP candidate**: package `@app/domain` has an outgoing import
> edge to `pg`. The high-level (`@app/domain`) is depending on a
> low-level detail (`pg`). Define a `UserRepo` interface in
> `@app/domain`, move the Postgres-specific code to
> `@app/infra-pg`, and let `@app/infra-pg` implement
> `UserRepo` from `@app/domain`. Wire the concrete `PgUserRepo`
> only in `@app/composition`. Add a `dependency-cruiser` rule
> forbidding `packages/domain → packages/infra-*`.
>
> Reference: <https://khalilstemmler.com/articles/software-design-architecture/organizing-app-logic/>

## Related principles

- [SRP](SRP.md) — defines what "a module" is;
  DIP says how modules connect.
- [OCP](OCP.md) — the interfaces DIP introduces are
  exactly the extension points OCP requires.
- [ISP](ISP.md) — make the ports small
  enough to be worth depending on (one method ports are common and
  good).
- [LSP](LSP.md) — substitutability of
  implementations behind a port is what makes DIP pay off.
- [Composition Over Inheritance](CoI.md)
  — DIP is the macro form of "compose with interfaces, don't extend
  concretes".
- Hexagonal Architecture (Cockburn) — the
  architecture-scale instantiation of DIP; ports = domain
  interfaces, adapters = infra implementations.

## References

1. Martin, R. C. "The Dependency Inversion Principle". 1996.
   <https://web.archive.org/web/20110714224327/http://www.objectmentor.com/resources/articles/dip.pdf>
2. Martin, R. C. *Clean Architecture*. Ch. 11.
3. Cockburn, A. "Hexagonal Architecture". 2005.
   <https://alistair.cockburn.us/hexagonal-architecture/>
4. Seemann, M. *Dependency Injection: Principles, Practices, and Patterns*. Manning.
5. Stemmler, K. "Organizing App Logic with the Clean Architecture".
   <https://khalilstemmler.com/articles/software-design-architecture/organizing-app-logic/>
6. Stemmler, K. "Domain-Driven Design w/ TypeScript" series.
   <https://khalilstemmler.com/articles/categories/domain-driven-design/>
7. `dependency-cruiser`. <https://github.com/sverweij/dependency-cruiser>
8. `eslint-plugin-boundaries`. <https://github.com/javierbrea/eslint-plugin-boundaries>
9. TypeScript project references.
   <https://www.typescriptlang.org/docs/handbook/project-references.html>
