# ADP — Acyclic Dependencies Principle (in TypeScript)

**TL;DR**: The dependency graph between modules (and packages) must
form a Directed Acyclic Graph (DAG). When module `A` imports module
`B`, no chain of imports should bring `B` back to `A`. TypeScript
(targeting TS 5.4+) and its host runtimes do **not** reject cycles
the way Cargo rejects cyclic crates — they "work" until they
don't. Cycles destroy releasability, testability, tree-shaking, and
introduce *temporal dead-zone* (TDZ) and partial-export bugs that
surface only at runtime.

## Canonical sources

- Robert C. Martin, "Granularity: The Acyclic Dependencies Principle"
  (1996, *C++ Report*):
  <https://web.archive.org/web/20061206155400/http://www.objectmentor.com/resources/articles/granularity.pdf>
- Robert C. Martin, *Clean Architecture* (2017), Ch. 14
  "Component Coupling": ADP, SDP, SAP.
- John Lakos, *Large-Scale C++ Software Design* (1996): the original
  case for acyclic component dependencies, applicable verbatim to
  TypeScript.
- ECMAScript specification, "Cyclic Module Records":
  <https://tc39.es/ecma262/#sec-cyclic-module-records>
- Node.js docs, "Modules: ECMAScript modules — Cycles":
  <https://nodejs.org/api/esm.html#cycles>
- TypeScript Handbook, "Modules":
  <https://www.typescriptlang.org/docs/handbook/2/modules.html>

## The principle

Martin's "morning after syndrome": a developer commits a change to
a shared module, goes home, and the next morning everybody else's
build breaks. The cause is a cycle: changing `A` forces a rebuild
of `B`, which forces a rebuild of `C`, which depends on a different
shape of `A`, etc.

Once the dependency graph has even one cycle:

- **Initialization order is undefined.** ES module loaders pick *some*
  topological order; for cycles they break ties arbitrarily, and a
  `const` from a not-yet-evaluated module reads as `undefined`
  (CJS) or throws a TDZ `ReferenceError` (ESM).
- **Releases lose granularity.** You cannot publish `@scope/a` v2.0
  without also bumping `@scope/b`, `@scope/c`, `@scope/d`.
- **Tests get expensive.** Testing `a.ts` requires loading `b.ts` and
  `c.ts` because they pull `a.ts` back in.
- **Tree-shaking degrades.** Bundlers (esbuild, Rollup, webpack)
  cannot eliminate dead code across cycle members; the whole SCC is
  retained.
- **Type inference slows.** `tsc` cannot finalize the type of a
  symbol participating in a cycle until every member of the cycle is
  parsed; large SCCs are a known cause of "slow build" complaints.
- **Code becomes hard to reason about.** "What does this module do?"
  cannot be answered locally if the module sits in a cycle.

The principle is therefore simple: **break the cycles**. Always.

## Why it matters in JS/TS specifically

In Rust a cycle is a build-time crime. In TypeScript a cycle is
usually a runtime *time bomb*. The two failure modes differ:

### ESM cycle failure mode

```ts
// a.ts
import { B } from "./b.js";
export const A = { kind: "A" as const, peer: B };  // TDZ if b is mid-eval
```

```ts
// b.ts
import { A } from "./a.js";
export const B = { kind: "B" as const, peer: A };  // TDZ if a is mid-eval
```

The ECMAScript spec evaluates one module at a time. Whichever loads
first hits a *binding* to the other that exists but is in the
temporal dead zone. The reference evaluated at top level throws
`ReferenceError: Cannot access 'B' before initialization`. If the
reference is inside a function body, evaluation is deferred and it
"works" — until a different entry point reorders the load and it
doesn't.

### CommonJS cycle failure mode

```js
// a.js
const { b } = require("./b");
exports.a = () => `a calls ${b()}`;
```

```js
// b.js
const { a } = require("./a");          // a is mid-evaluation
exports.b = () => `b calls ${a?.()}`;  // a is undefined here
```

Node returns the *partial* `exports` object — whatever has been
assigned so far. The second `require` resolves to `{}`. There is no
error, just `undefined` where a function should be. Bugs surface
later as `TypeError: a is not a function`, often only on specific
entry points.

### The dual-package hazard

A package shipping both ESM and CJS (`exports` map with `import`
and `require` conditions) can hit *both* failure modes from the same
cycle, plus a third where two copies of the same module are
instantiated and `instanceof` checks fail across the seam.

## Type-only imports are not a free pass

```ts
import type { Order } from "./order.js";  // erased at emit
```

`import type` (and `export type`) is erased by `tsc` and by
`isolatedModules`-aware bundlers. It does **not** create a runtime
edge. This is TypeScript's analogue of Python's
`if TYPE_CHECKING:` — useful for breaking *runtime* cycles caused
purely by type references.

But: type-only imports do not erase the *architectural* cycle.
`madge --ts-config` and `dependency-cruiser` (with
`detectJSCircular: true, tsPreCompilationDeps: true`) will still
report the cycle, and rightly so. Two modules that need each other's
types are coupled regardless of whether the coupling survives to
runtime. Treat `import type` as an emergency suture, not a cure.

## Common cycle shapes

### Shape 1: Barrel re-export ↔ sibling

```
index.ts ────────→ order.ts
   ▲                   │
   └───── customer.ts ◄┘
```

`index.ts` (barrel) re-exports `./order` and `./customer`. Inside
`customer.ts` someone writes `import { Order } from "./index.js"`
because the IDE auto-imported from the package root. Cycle.

Fix: ban deep imports of your own barrel from sibling files. Inside
the package, import siblings directly: `import { Order } from
"./order.js"`. Barrel files are *external* API.

### Shape 2: Component ↔ child component (React)

```
Parent.tsx ─────→ Child.tsx
   ▲                 │
   └─────────────────┘
```

`Parent.tsx` renders `<Child />`. `Child.tsx` imports
`ParentProps` for a callback signature. Cycle.

Fix: extract the shared type into `Parent.types.ts` (a leaf both
import from), or invert the dependency — pass the callback shape
inline (`(arg: { id: string }) => void`) and stop sharing the
parent's prop type.

### Shape 3: Service container ↔ service module

```
container.ts ──────→ userService.ts
     ▲                     │
     └─────────────────────┘
```

`container.ts` builds a DI container and constructs `UserService`.
`userService.ts` imports `container` to grab `db` and `logger`.
Cycle — and the canonical service-locator anti-pattern besides.

Fix: inject `db` and `logger` as constructor arguments. `container`
becomes the only place that knows about all services; services
know nothing about the container.

### Shape 4: `package.json` workspace cycle

```
packages/api ─────→ packages/core
     ▲                   │
     └───────────────────┘
```

`pnpm` and `yarn` workspaces allow this if both packages list each
other under `dependencies`. `npm` will install it; `pnpm` warns but
proceeds. The package graph is now cyclic — `pnpm publish` of one
without the other is impossible, and `pnpm -r build`'s topological
order is undefined.

Fix: extract `@scope/types` as a leaf workspace. Both `api` and
`core` depend on it.

## Violations and remedies

### Anti-pattern: shared domain type imported across siblings

```ts
// src/order/repo.ts
import { Order } from "./service.js";   // domain type lives in service

// src/order/service.ts
import { OrderRepository } from "./repo.js";
```

`repo` and `service` cycle. The domain type `Order` should live in
neither.

### Idiomatic fix: domain types in a leaf

```ts
// src/order/model.ts
export interface Order { /* ... */ }

// src/order/repo.ts
import type { Order } from "./model.js";
export interface OrderRepository {
  save(order: Order): Promise<void>;
}

// src/order/service.ts
import type { Order } from "./model.js";
import type { OrderRepository } from "./repo.js";

export class OrderService {
  constructor(private readonly repo: OrderRepository) {}
  async place(order: Order): Promise<void> {
    await this.repo.save(order);
  }
}
```

Three modules: `model`, `repo`, `service`. Dependency arrows:
`model ← repo ← service`. No cycle.

### Anti-pattern: route file back-references the app

```ts
// src/app.ts
import { apiRoutes } from "./routes/api.js";
export const app = express().use("/api", apiRoutes);
export const config = loadConfig();

// src/routes/api.ts
import { config } from "../app.js";       // reaches back into app
export const apiRoutes = Router().get("/", (_, res) => res.json(config));
```

`app → routes/api → app`. Cycle. At cold start, depending on which
file the test runner loads first, `config` is `undefined`.

### Idiomatic fix: extract config

```ts
// src/config.ts
export const config = loadConfig();

// src/app.ts
import { config } from "./config.js";
import { apiRoutes } from "./routes/api.js";
export const app = express().use("/api", apiRoutes);

// src/routes/api.ts
import { config } from "../config.js";
export const apiRoutes = Router().get("/", (_, res) => res.json(config));
```

`config` is a leaf. Both `app` and `routes/api` depend on it.

### Anti-pattern: barrel re-exports as cycle multiplier

```ts
// src/index.ts
export * from "./order.js";
export * from "./customer.js";
export * from "./invoice.js";

// src/invoice.ts
import { Order, Customer } from "./index.js";  // re-import own barrel
```

A single edge from `invoice` to `index` creates a cycle with **every
sibling** the barrel re-exports. One offence, six SCC members.

Fix: import siblings directly. Reserve `index.ts` for *external*
consumers. Some teams forbid `index.ts` altogether inside the
package and write it only at the package boundary.

### Anti-pattern: the `import()` "fix"

```ts
// src/a.ts
export async function doA() {
  const { doB } = await import("./b.js");  // dynamic import
  return doB();
}

// src/b.ts
import { doA } from "./a.js";
export const doB = () => doA();
```

The cycle is now *hidden* from static analysis: `tsc` and the
bundler see only an edge `b → a`. The runtime cycle is intact. The
test suite still hits TDZ if `b` calls `doA` at top level. Bundlers
emit a separate chunk for `b`, defeating tree-shaking and turning a
synchronous call site into a Promise.

**This is a smell, not a fix.** Dynamic `import()` belongs in code-
splitting (lazy route loading, optional features), not in cycle
laundering. If your only reason for `import()` is "the linter
stopped complaining", you have moved the problem from build time to
runtime where it is more expensive to diagnose.

The real fix is the same as in the static case: extract a leaf.

## Cycles in imports vs cycles in calls

**Import cycle (module-level Uses cycle)**: module `A` imports a
symbol from module `B` and vice versa. Loads, may TDZ at top level,
will at minimum break tree-shaking. Code Ranker flags it. Often easy
to break by extracting types/constants into a leaf — no logic
change.

**Call cycle (module-level Calls cycle)**: a function in `A` calls
a function in `B` which calls back into `A`. This is a real runtime
cycle. Occasionally legitimate (mutually recursive parsers), usually
a sign that two modules share a responsibility and should be merged
or re-sliced.

Code Ranker distinguishes the two: `module-call-cycle` is **Critical**;
import-only cycles are Medium/Low depending on size.

## ADP at the package level

`pnpm`, `yarn`, and `npm` workspaces allow but do not prevent
cyclic `dependencies` between local packages. The symptoms differ
from a single-package cycle:

- `pnpm publish` of one cycle member alone is impossible — the
  registry will reject because the peer is unpublished.
- Topological build (`pnpm -r build`) has no valid order; tools fall
  back to an arbitrary one and may build against stale `dist/`.
- Version skew: if one cycle member is also published to a registry,
  the local workspace and the registry version diverge silently.

A workspace passes ADP when:

- No `dependencies`/`devDependencies` cycle exists between local
  packages (Code Ranker's `package-cycle` rule detects this).
- No version skew exists for shared dependencies (use `pnpm`'s
  `catalog:` or `overrides`).
- The package-level DAG is **shallow** — flat layouts of one or two
  layers, not a deep tower of `@scope/utils` → `@scope/core` →
  `@scope/domain` → `@scope/api` → ....

## How Code Ranker detects ADP violations

| Signal | Rule |
|---|---|
| SCC of size > 1 on module-level `Uses`/`Re-exports` edges | `barrel-cycle`, `module-import-cycle` |
| SCC of size > 1 on module-level call graph | `module-call-cycle` (Critical) |
| Package-level cycle in `package.json` `dependencies` | `package-cycle` |
| Barrel (`index.ts`) participating in any cycle | `barrel-cycle` (specific shape, high impact) |

Related external tools:

- **`madge`** — `npx madge --circular --extensions ts,tsx src/`
  produces a list of SCCs. Fast, no config, integrates in CI.
- **`dependency-cruiser`** — richer rules (`no-circular`,
  `not-to-unresolvable`, layer constraints) and a JSON output for
  custom checks. Configure with `tsPreCompilationDeps: true` to see
  cycles that exist only through `import type`.
- **`eslint-plugin-import`** with `import/no-cycle` — per-file lint
  feedback in the editor. Useful as a fast guard rail but blind to
  longer SCCs (default `maxDepth: ∞` is slow; teams cap it and miss
  cycles).

## Suggested recommendation template

> **ADP violation**: modules `routes/api.ts` and `app.ts` form a
> 2-module import cycle in package `@scope/server`. The morning-
> after failure mode for cycles (Martin 1996) applies: changes in
> either invalidate the other, and at cold-start the ESM loader may
> evaluate `routes/api.ts` before `app.ts`, leaving `config` in TDZ.
> Break the cycle by extracting `src/config.ts` as a leaf; both
> `app` and `routes/api` depend on it.
>
> Reference:
> <https://web.archive.org/web/20061206155400/http://www.objectmentor.com/resources/articles/granularity.pdf>

## ADP and incremental compilation

`tsc --incremental` and `tsc -b` (project references) rebuild the
*transitive closure* of a changed file. In a cycle, the closure is
the entire SCC: touching any one member invalidates all of them.
A 12-module SCC means every change to any one of the 12 invalidates
the others.

Bundlers behave similarly. Vite's module graph and webpack's
`HotModuleReplacement` cannot replace a single module inside a
cycle — they fall back to a full page reload because partial
re-evaluation would expose the cycle's TDZ. Cycles are
**time-multiplicative** for both build and dev-loop performance:
the wider the SCC, the longer every iteration takes.

## Related principles

- [DIP](DIP.md) — DIP is *how* you break a
  cycle. An interface moves to one side of the arrow; the cycle
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
4. ECMAScript spec, "Cyclic Module Records".
   <https://tc39.es/ecma262/#sec-cyclic-module-records>
5. Node.js, "Modules: ECMAScript modules — Cycles".
   <https://nodejs.org/api/esm.html#cycles>
6. `madge`. <https://github.com/pahen/madge>
7. `dependency-cruiser`.
   <https://github.com/sverweij/dependency-cruiser>
8. `eslint-plugin-import`, `import/no-cycle`.
   <https://github.com/import-js/eslint-plugin-import/blob/main/docs/rules/no-cycle.md>
