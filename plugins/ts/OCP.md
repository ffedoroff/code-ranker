# OCP — Open/Closed Principle (in TypeScript)

**TL;DR**: A module is **open for extension** but **closed for
modification**. In TypeScript this means: prefer adding a new
discriminated-union member behind a registry, a new strategy
implementation, or a new plugin module over editing existing call
sites. Use discriminated unions with `never`-assertion `switch` for
the closed side and interface-typed registries / dependency injection
for the open side.

## Canonical sources

- Bertrand Meyer, *Object-Oriented Software Construction* (1988):
  coined the principle in the inheritance-based form.
- Robert C. Martin, "The Open-Closed Principle" (1996, *C++ Report*):
  reframed for polymorphism rather than inheritance, the version most
  cited today. <https://web.archive.org/web/20060822033314/http://www.objectmentor.com/resources/articles/ocp.pdf>
- Martin, *Clean Architecture* (2017), Ch. 8.
- TypeScript team blog, "Declaration Merging" and "Module
  Augmentation":
  <https://www.typescriptlang.org/docs/handbook/declaration-merging.html>
- TypeScript 5.0 release notes — `const` type parameters and
  `satisfies` operator (5.0/4.9):
  <https://devblogs.microsoft.com/typescript/announcing-typescript-5-0/>
- "Exhaustiveness checking with `never`", TS handbook:
  <https://www.typescriptlang.org/docs/handbook/2/narrowing.html#exhaustiveness-checking>

## The principle

A type, module, or package has fulfilled OCP when **its consumers can
add new behaviour without modifying its source**. Modification is
"reaching inside" — adding a `case` to a `switch` you don't own,
mutating a built-in prototype, augmenting a class from `node_modules`.
Extension is "plugging in" — implementing a published interface,
registering a strategy on a registry, importing a new plugin module.

The deep idea: any line of source you change is a line your existing
users might break on. So make new behaviour additive.

OCP is most often misread as "use inheritance" or "everything must be
abstract". Neither is true. The actual prescription is:

1. Identify the **axes of likely change**.
2. For each axis, expose an extension point that varies along it.
3. Keep everything else **closed** — don't allow callers to depend on
   internals that should be free to evolve.

In a TypeScript package, the axes of likely change are usually:

- New members of a discriminated union (event kinds, error kinds,
  output formats).
- New implementations of an interface (storage backends, auth schemes,
  transport adapters).
- New optional fields on a config object (new flags, new knobs).
- New parameters on a function (new context the caller can pass).

For each, TypeScript has an idiomatic "closed for modification" tool —
but the toolset is weaker than Rust's, so the discipline must come
from the team.

## Why it matters

OCP is the principle that protects you from **upstream cascades**: a
one-line change to a popular API ripples through every downstream
consumer at semver-breaking magnitude. A `types/events.ts` exporting a
union matched in 124 call sites must be designed to evolve additively
or every release ships with a follow-up PR fanning out across the
monorepo.

The opposite of OCP is *not* "no abstraction" — it is "every change
becomes a major version bump". You feel the absence of OCP through
release notes that say "BREAKING: added union member; renamed field;
added required parameter".

## In TypeScript

TypeScript has no exact analogue of Rust's `#[non_exhaustive]`. The
closest substitutes are:

- **Opaque / branded types** — hide the structure so consumers cannot
  literal-construct the value.
- **`satisfies` with `as const`** — let the library author declare the
  shape internally while exposing only a wider, evolvable type to
  consumers.
- **Library-versioning conventions** — semver + a documented "do not
  exhaustively switch on this union outside the package" rule.

None of these is enforced by the compiler the way `#[non_exhaustive]`
is. The team makes the closure real through code review and through
the four tools below.

### 1. Discriminated unions + exhaustive `switch` (the closed side)

```ts
export type Event =
  | { kind: "insert"; row: Row }
  | { kind: "update"; row: Row; prev: Row }
  | { kind: "delete"; id: string };

export function dispatch(e: Event): void {
  switch (e.kind) {
    case "insert": return onInsert(e.row);
    case "update": return onUpdate(e.row, e.prev);
    case "delete": return onDelete(e.id);
    default: {
      const _exhaustive: never = e;
      throw new Error(`unhandled event: ${JSON.stringify(_exhaustive)}`);
    }
  }
}
```

The `never` assignment turns "I forgot a case" into a compile error.
Inside the defining package this is exactly what you want — adding a
new member is meant to ripple out and force every handler to be
updated. The union and the switches around it form a **closed**
contract: every consumer must handle every case.

The tension: this is also why adding a union member is expensive
across a workspace. Closed-ness is paid for at every call site.

### 2. Registry pattern (the open side)

```ts
export interface Renderer {
  readonly id: string;
  render(data: Data): string;
}

export class RendererRegistry {
  private readonly renderers = new Map<string, Renderer>();

  register(r: Renderer): void {
    if (this.renderers.has(r.id)) throw new Error(`duplicate: ${r.id}`);
    this.renderers.set(r.id, r);
  }

  render(id: string, data: Data): string | undefined {
    return this.renderers.get(id)?.render(data);
  }
}
```

A new format is a new `Renderer` implementation, registered at
startup. `RendererRegistry` itself never changes. Contrast with:

```ts
function render(format: "json" | "toml" | "yaml", data: Data): string {
  switch (format) {
    case "json": return renderJson(data);
    case "toml": return renderToml(data);
    case "yaml": return renderYaml(data);
  }
}
```

Adding `"cbor"` modifies `render` and every call site whose
`format` argument is a literal union of the same shape. The registry
form is **open** for new entries without modification.

### 3. Strategy via interface-typed parameter (DI)

```ts
export interface Clock { now(): number; }
export interface Storage { put(k: string, v: Uint8Array): Promise<void>; }

export class Cache {
  constructor(
    private readonly clock: Clock,
    private readonly storage: Storage,
  ) {}
}
```

`Cache` does not depend on a concrete clock or storage. Tests pass
fakes, production passes the real impl, and a future Redis backend is
a new `Storage` class — none of which requires editing `Cache`.

### 4. Plugin discovery via dynamic `import()`

```ts
// host package
const plugins = await Promise.all(
  config.plugins.map(spec => import(spec)),
);
for (const mod of plugins) {
  registry.register(mod.default);
}
```

The host knows only the plugin interface. New plugins ship as
separate packages and are discovered via configuration (or via
`package.json` `keywords` registries scanned at install time, the
pattern used by ESLint plugins, Babel principles, and the Backstage
plugin system). The host is closed; the ecosystem is open.

## The Rust tension, in TS terms

Adding a discriminated-union member **touches every switch** (closed
side — by design). Adding a registry entry **touches no consumer**
(open side — by design). The architectural question for every axis of
change is which side it belongs on:

- Is the set of variants small, semantically meaningful, and worth
  forcing every handler to acknowledge? Use a union.
- Is the set unbounded, ecosystem-supplied, or expected to grow per
  release? Use a registry.

Picking wrong is the most common OCP mistake. A `LogFormat` union
across 80 call sites that grows monthly should have been a registry
on day one.

## Violations and remedies

### Anti-pattern: `as` casts that bypass union exhaustiveness

```ts
function handle(e: Event): void {
  const ins = e as Extract<Event, { kind: "insert" }>;
  onInsert(ins.row);
}
```

The cast silences narrowing — and adding a new union member will not
flag this function. The `never` exhaustiveness guard never fires
because the union was thrown away.

**Idiomatic fix**: narrow with `switch` on the discriminant or with
a type predicate that the compiler can verify.

### Anti-pattern: discriminated union `switch` without `never` default

```ts
function dispatch(e: Event): void {
  switch (e.kind) {
    case "insert": return onInsert(e.row);
    case "update": return onUpdate(e.row, e.prev);
    case "delete": return onDelete(e.id);
  }
  // no default — adding e.kind === "truncate" silently falls through
}
```

When the union grows, the function silently returns `undefined` for
the new case. Make exhaustiveness load-bearing:

```ts
default: {
  const _exhaustive: never = e;
  throw new Error(`unhandled: ${(_exhaustive as { kind: string }).kind}`);
}
```

### Anti-pattern: `instanceof` chains for dispatch

```ts
function area(s: Shape): number {
  if (s instanceof Circle) return Math.PI * s.r ** 2;
  if (s instanceof Square) return s.side ** 2;
  if (s instanceof Triangle) return (s.base * s.height) / 2;
  throw new Error("unknown shape");
}
```

Every new shape modifies `area`. Either move `area()` onto the class
(polymorphism) or convert `Shape` into a discriminated union with an
exhaustive `switch`. Both make new-shape addition mechanical and
compiler-checked.

### Anti-pattern: `Object.assign` mutation of shared config

```ts
Object.assign(defaultConfig, { retries: 5 });
```

Every importer of `defaultConfig` is now affected, including ones that
imported it before the mutation. The config object is "open" in
exactly the wrong direction: anyone can modify it from anywhere.

**Idiomatic fix**: freeze defaults with `as const` + `Object.freeze`
and require callers to spread into a new object.

```ts
export const defaultConfig = Object.freeze({
  retries: 3,
  timeoutMs: 5_000,
} as const);
```

### Anti-pattern: classes extending third-party classes from `node_modules`

```ts
import { Request } from "express";

class AuthedRequest extends Request {
  user?: User;
}
```

You have coupled to the *implementation* of `Request`, not just its
interface. Any internal change in Express that touches the
constructor, private fields, or method signatures will break you. The
upstream maintainer cannot tell that you are extending; they consider
themselves free to evolve internals.

**Idiomatic fix**: composition (wrap a `Request` in your own object)
or, where the library invites it, *module augmentation* — the
controlled escape hatch.

### Anti-pattern: `Object.defineProperty` on built-ins (monkey-patching)

```ts
Object.defineProperty(Array.prototype, "last", {
  get() { return this[this.length - 1]; },
});
```

This is the worst form of openness: every Array in the program is now
modified, including arrays created by libraries that did not consent.
Two packages each polyfilling `Array.prototype.last` collide. The
proposed standard `Array.prototype.at` shipping later collides.
Iteration with `for..in` now visits the property. Prototype pollution
is the security-bug name for the same shape applied to `Object`.

**Idiomatic fix**: a free function `last(arr)` or a wrapper type.
Never write to `Object.prototype`, `Array.prototype`, or any built-in.

## Module augmentation as a controlled escape hatch

Declaration merging and module augmentation are TypeScript's official
answer to "I need to extend a library's types without forking it":

```ts
// src/types/express.d.ts
import "express";

declare module "express" {
  interface Request {
    user?: User;
  }
}
```

The TypeScript team blog frames this as a **deliberate** extension
point, distinct from monkey-patching:

- It only changes *types*, not runtime behaviour (the runtime
  assignment is still your responsibility, typically in middleware).
- It is scoped to your compilation; downstream packages are unaffected
  unless they explicitly import your augmentation.
- It is the mechanism libraries like Express, Fastify, and Vite
  document for plugin authors.

Use module augmentation when:

- The upstream library documents it as an extension point.
- You need to add a field whose presence is invariant within your
  process (e.g. set by middleware on every request).

Avoid module augmentation when:

- You can compose instead (wrap, don't extend).
- The upstream library does not document an augmentation contract —
  you are then back to coupling to internals.

## How code-ranker detects OCP violations

OCP violations are subtler than SRP — they often look like normal
code until upstream-evolution time. Code Ranker can flag the structural
*precursors*:

| Signal | OCP interpretation |
|---|---|
| Discriminated union `switch` without `never` default | New union member will silently fall through; the union and its consumers form an unenforced contract. |
| `instanceof` chains across many call sites | Every new subclass modifies every chain; convert to polymorphism or a registry. |
| Type predicate (`is Foo`) spread across many call sites | The predicate is being used as ad-hoc dispatch; consider a discriminated union or a strategy interface. |
| `as` cast on a union-typed value | Bypasses exhaustiveness; new members will not flag this site. |
| Public interface with N implementations across packages | Adding a method is breaking for every implementor; consider default methods or a stable v1 / experimental v2 split. |
| Glob re-exports (`export * from "./internal"`) | Closes nothing — every public name of `internal` becomes part of *your* contract. |
| `Object.assign` / `Object.defineProperty` on imported objects | Monkey-patching; modification masquerading as extension. |

Cross-references in code-ranker's catalog:

- `high-fan-in-public-api` already prescribes interface stability +
  registry patterns. Severity escalates when the API is a bare union
  matched at many call sites.
- A future `union-switch-without-never` rule would directly map.

## Suggested recommendation template

> **OCP candidate**: discriminated union `Event` is public and is
> matched in 23 call sites across the workspace. Adding a new
> member currently requires editing all 23. If new event kinds are
> expected with every release, convert dispatch to a registry of
> `EventHandler` strategies keyed by `kind`; if the set is stable and
> every consumer genuinely must handle every kind, add the `never`
> exhaustiveness guard so additions become compile errors rather than
> silent fall-throughs.
>
> Reference: <https://www.typescriptlang.org/docs/handbook/2/narrowing.html#exhaustiveness-checking>
<!-- doc:base "Related principles" -->

## References

1. Meyer, B. *Object-Oriented Software Construction*. 1988.
2. Martin, R. C. "The Open-Closed Principle". *C++ Report*, 1996.
   <https://web.archive.org/web/20060822033314/http://www.objectmentor.com/resources/articles/ocp.pdf>
3. Martin, R. C. *Clean Architecture*. Prentice Hall, 2017. Ch. 8.
4. TypeScript Handbook, "Declaration Merging".
   <https://www.typescriptlang.org/docs/handbook/declaration-merging.html>
5. TypeScript Handbook, "Narrowing — exhaustiveness checking".
   <https://www.typescriptlang.org/docs/handbook/2/narrowing.html#exhaustiveness-checking>
6. TypeScript 5.0 release notes (const type parameters, `satisfies`).
   <https://devblogs.microsoft.com/typescript/announcing-typescript-5-0/>
7. Snyk, "Prototype pollution".
   <https://learn.snyk.io/lesson/prototype-pollution/>
8. ESLint plugin discovery convention (`eslint-plugin-*` keyword).
   <https://eslint.org/docs/latest/extend/plugins>
