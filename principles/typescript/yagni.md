# YAGNI — You Aren't Gonna Need It (in TypeScript)

**TL;DR**: Build for the problem you have now, not the problem you
imagine you might have later. In TypeScript this becomes: don't add
an `interface` for a hypothetical second implementation; don't add
a generic type parameter for a hypothetical second caller; don't
`export` an internal symbol "for later"; don't wire a LaunchDarkly
flag for a feature nobody asked for; don't split your monorepo into
seven packages because one of them *might* be reused.

Target: TypeScript 5.4+ in a pnpm/yarn workspace.

## Canonical sources

- Ron Jeffries, "You're NOT Gonna Need It!" (1998): origin of the
  acronym in Extreme Programming.
  <https://ronjeffries.com/xprog/articles/practices/pracnotneed/>
- Kent Beck, *Extreme Programming Explained* (1999): the practice's
  formulation.
- Fred Brooks, *The Mythical Man-Month* (1975, anniv. ed. 1995): the
  "second-system effect" — the urge to over-design once you know
  better — is the YAGNI failure mode at scale.
- John Ousterhout, *A Philosophy of Software Design* (2018): "deep
  modules" with narrow interfaces; speculative configuration is a
  classic "shallow module" smell.
- Martin Fowler, "Yagni" (2015):
  <https://martinfowler.com/bliki/Yagni.html>
- Sandi Metz, "The Wrong Abstraction":
  <https://sandimetz.com/blog/2016/1/20/the-wrong-abstraction>
- Dan Abramov, "Goodbye, Clean Code" (2020):
  <https://overreacted.io/goodbye-clean-code/> — premature
  deduplication and premature abstraction are the same disease as
  speculative `interface`s.
- TypeScript team interviews (Anders Hejlsberg, Daniel Rosenwasser,
  Ryan Cavanaugh on various podcasts) consistently push back on
  type-system gymnastics that have no runtime payoff: "the simpler
  type is almost always the right one for the codebase."

## The principle

YAGNI says: every feature, abstraction, configuration knob, or
extensibility point that is **not currently needed** has a real,
present cost — code to read, tests to maintain, `.d.ts` surface to
keep stable, documentation to write, downstream `import` paths to
honour — and zero present benefit. Its benefit is *hypothetical*.
The probability of that benefit being realized is usually lower
than engineers estimate.

The standard error: "I'll add a LaunchDarkly flag for this so we
can turn it off in the future." The future comes, the flag is never
toggled, but the test matrix is now twice as large and every reader
has to ask "is this branch live?"

YAGNI complements KISS by giving a temporal argument: even when an
abstraction *would* be appropriate eventually, it is the wrong
investment **now** if "eventually" hasn't arrived.

Fowler's clarification: YAGNI is not "never add anything in advance".
It is "the cost of adding it speculatively is usually higher than
the cost of adding it on-demand, and the on-demand version is more
likely to be the right shape because you have real requirements".

## Why it matters

Speculative engineering hurts in four ways:

1. **Direct cost**: code, tests, docs, code-review time.
2. **Carrying cost**: every reader pays for the abstraction in
   cognitive load — and in TypeScript, every speculative generic
   shows up in hover tooltips, in inferred types, and in error
   messages forever.
3. **Opportunity cost**: time spent on speculation is time not
   spent on the real problem.
4. **Lock-in cost**: once published to npm (or even just exported
   from a workspace package), the speculative shape is part of your
   API. Removing it is a breaking change, and TypeScript's
   structural typing means even renames can break consumers who
   inlined the shape.

The fourth is especially severe in libraries. A speculative
`interface Foo` that two downstream apps start implementing becomes
a versioning nightmare even if the original author never wanted it
as a public contract — and Hyrum's Law guarantees somebody depends
on the exact key order, the exact `readonly` modifier, or the
exact `| undefined` you wrote.

YAGNI is partially a humility argument: you cannot predict which
future need will materialize. The history of every npm package is
full of options nobody used and missing features everybody needed.

## In TypeScript

TypeScript's design accommodates incremental complexity well — you
can *always* extract an `interface` later when you have a second
implementation, *always* add a generic parameter later when you
have a second caller. YAGNI takes advantage of this.

### The "interface on demand" pattern

Start with a concrete class or object:

```ts
export class UserRepository {
  constructor(private readonly db: Pool) {}
  async find(id: UserId): Promise<User | undefined> { /* ... */ }
}
```

When the second backend appears (e.g. an in-memory store for
tests), *then* extract a type:

```ts
export interface UserRepository {
  find(id: UserId): Promise<User | undefined>;
}

export class PostgresUserRepository implements UserRepository { /* ... */ }
export class MemoryUserRepository implements UserRepository { /* ... */ }
```

The refactor is mechanical and small *because the interface is
being extracted from real, working code*. Compare to writing the
`interface` first when only one implementation exists — you'd be
guessing at the method set, and the second impl will inevitably
need something the first doesn't.

### The "generic on demand" pattern

```ts
export function parseUserId(s: string): UserId { /* ... */ }
```

If you discover the same logic applies to `OrderId`, *then* make
it generic:

```ts
export function parseId<T extends Brand<string, symbol>>(s: string): T { /* ... */ }
```

Don't write `parseId<T extends ...>` from the start. The
hypothetical second caller never arrives, or arrives with a shape
your constraints don't fit.

A related smell: **speculative `unknown` widening**. A function
that returns `User` doesn't need to return `User | unknown` "in
case we add more cases later". `unknown` is contagious — it forces
narrowing at every call site.

### `export` on demand

The most common, most expensive YAGNI violation in TypeScript:
adding `export` "in case someone needs it". Every exported symbol
is a contract — within a workspace package, with downstream
consumers, with `.d.ts` users. The discipline:

- Default to module-local (no `export`).
- Promote to `export` from the file when another file in the
  same package needs it.
- Promote to a package entry point (re-exported from
  `src/index.ts`) only when an external consumer actually exists.

When the package's public surface is small, you can refactor
internals freely. Tools like `ts-prune`, `knip`, or
`@typescript-eslint/no-unused-vars` (with `varsIgnorePattern: '^_'`
disabled) can flag unused exports.

### Feature flags on demand

```ts
// Don't:
if (process.env.ENABLE_NEW_PRICING === 'true') { newPricing(); }
else { oldPricing(); }
```

Add a flag only when there's a current consumer who needs the
un-flagged version *not* to apply to them — i.e., a real rollout
plan. A flag for "future flexibility" carries all the cost of two
code paths (tests, types, behavior) with none of the value.

The same applies to LaunchDarkly / Unleash / Statsig flags: every
flag is an `if/else` that must be tested in both states forever
(or until somebody is brave enough to delete it, which is rarely).

## Violations and remedies

### Anti-pattern: `interface` without a second implementation

```ts
export interface NotificationSender {
  send(to: string, message: string): Promise<void>;
}

export class EmailNotificationSender implements NotificationSender {
  async send(to: string, message: string): Promise<void> { /* ... */ }
}
```

Only `EmailNotificationSender` exists. The `interface` is dead
weight: it adds a level of indirection at every call site, requires
test doubles, and complicates type signatures (now every consumer
has to choose between the class and the interface).

### Idiomatic fix: drop the interface

```ts
export class EmailNotificationSender {
  async send(to: string, message: string): Promise<void> { /* ... */ }
}
```

When SMS or push arrives, *then* extract an `interface`.

### Anti-pattern: generic where a concrete type is fine

```ts
export function saveUser<S extends UserStore>(store: S, u: User): Promise<void> {
  return store.save(u);
}
```

There is one `UserStore` and one caller. The generic is busywork
that pollutes hover types and error messages.

### Idiomatic fix: name the concrete type

```ts
export function saveUser(store: UserStore, u: User): Promise<void> {
  return store.save(u);
}
```

### Anti-pattern: `Promise<T>` where sync would do

```ts
export async function formatPrice(cents: number, currency: Currency): Promise<string> {
  return `${currency}${(cents / 100).toFixed(2)}`;
}
```

There is nothing async here. `async` was added "in case we want to
look up locale data later". Now every call site needs `await`,
every test needs to be async, and the type signature lies. The
"future" never materialized; even if it did, making a sync function
async later is one keystroke per call site (assisted by the
compiler).

### Idiomatic fix: keep it sync

```ts
export function formatPrice(cents: number, currency: Currency): string {
  return `${currency}${(cents / 100).toFixed(2)}`;
}
```

### Anti-pattern: configuration knob nobody requested

```ts
export interface ServerConfig {
  listenAddr: string;
  maxConnections: number;
  idleTimeoutMs: number;
  bufferSize: number;            // never tuned
  readChunkSize: number;         // never tuned
  writeChunkSize: number;        // never tuned
  backpressureHighWatermark: number; // never tuned
  backpressureLowWatermark: number;  // never tuned
  queueStrategy: 'fifo' | 'lifo' | 'priority'; // only fifo used
}
```

Nine knobs. Three actually move. The other six complicate every
config-loading path, every test, every doc page, and every Zod
schema.

### Idiomatic fix: ship with what users can actually tune

```ts
export interface ServerConfig {
  listenAddr: string;
  maxConnections: number;
  idleTimeoutMs: number;
}
```

Add fields when a user *asks*. Adding optional fields to an
interface is non-breaking; removing them later is breaking.

### Anti-pattern: speculative workspace split

```
packages/
├── domain-types/     # branded ID types only
├── domain-schemas/   # zod schemas only
├── domain-errors/    # error classes only
├── domain-config/    # config types only
├── domain-impl/      # the actual logic
└── domain-utils/     # one-line helpers shared by the others
```

Six packages because "they might be useful separately". They never
are. Every PR touches three of them. `pnpm install` slows down.
Consumers pick one of the six and pull all of them transitively
via peer-deps. Version bumps cascade.

### Idiomatic fix: one `domain` package

```
packages/
└── domain/
    ├── src/
    │   ├── types.ts
    │   ├── schemas.ts
    │   ├── errors.ts
    │   ├── service.ts
    │   └── index.ts
```

When a real consumer needs only `domain/types`, *then* extract it.
Until then, one package is one cohesive thing.

### Anti-pattern: premature barrel file

```ts
// src/index.ts
export * from './users';
export * from './orders';
export * from './internals/cache';   // not meant to be public
export * from './internals/metrics'; // not meant to be public
export * from './deprecated/v1';     // nobody calls this
```

Barrel files turn every internal symbol into a public API by
default. They also defeat tree-shaking in some bundlers and slow
down `tsc` because every importer pulls the full module graph.

### Idiomatic fix: hand-curated entry point, or none at all

```ts
// src/index.ts
export { createUser, findUser } from './users';
export { createOrder } from './orders';
export type { User, Order } from './types';
```

If the package is small enough, skip the barrel entirely and let
consumers import from specific paths. Mark non-public modules with
a `// @internal` JSDoc tag so `api-extractor` strips them from
`.d.ts`.

### Anti-pattern: dead `enum` variants

```ts
export enum Behaviour {
  Strict,
  Lenient,
  Paranoid,    // designed but never used
  Permissive,  // designed but never used
}
```

Every consumer's exhaustive `switch` has to handle four cases. Two
of them are unreachable.

### Idiomatic fix: ship two variants

```ts
export type Behaviour = 'strict' | 'lenient';
```

(Prefer string-literal unions to `enum` in modern TS; they are
erased at runtime and play nicely with JSON.)

### Anti-pattern: `if (false)` and abandoned `@deprecated` paths

```ts
const NEW_PIPELINE = false; // flip this when ready

if (NEW_PIPELINE) {
  return runNewPipeline(input);
}
return runOldPipeline(input);

/** @deprecated use runNewPipeline */
export function runLegacyPipeline(input: Input) { /* 400 lines */ }
```

Two pipelines, one flag that has been `false` for 18 months, plus
a third "legacy" path nobody calls. Three for the price of one.

### Idiomatic fix: pick one and delete the rest

Either the new pipeline is ready (flip the flag, delete the old) or
it isn't (delete the new). `@deprecated` without a deletion date is
a lie.

### Anti-pattern: speculative `index.d.ts`

```ts
// types/global.d.ts
declare global {
  interface Window {
    __APP_DEBUG__?: DebugAPI;
    __APP_TELEMETRY__?: TelemetryAPI;  // never attached
    __APP_FEATURE_FLAGS__?: FlagAPI;   // never attached
  }
}
```

Global declarations for properties that don't exist at runtime.
Every consumer is now lying about `window`.

### Idiomatic fix: declare what exists

Remove the unused declarations. Add them back when the runtime
actually populates the property.

## YAGNI for libraries vs applications

A subtle but important distinction:

- For **applications**, YAGNI is almost always right. Add features
  when users ask. Delete flags after rollout.
- For **libraries** (especially published to npm), YAGNI is more
  nuanced. Some flexibility is *cheap insurance* that costs little
  now and saves a breaking change later.

The discriminator is **reversibility**: if a hypothetical future
need can be added later without a major-version bump, deferring is
safe YAGNI. If adding it later would break consumers, adding the
escape hatch now (cheaply) may be worth it.

In TypeScript libraries, the cheap defensive moves are:

- Return `Readonly<T>` / `readonly T[]` so you can change internal
  mutability later.
- Accept the *widest reasonable* input type and return the
  *narrowest reasonable* output type.
- Use object-bag parameters (`fn({ a, b })`) instead of positional
  arguments so adding option `c` is non-breaking.
- Mark unfinished public symbols `@internal` or `@alpha` (via
  `api-extractor`) so consumers know not to depend on them.

These are not YAGNI violations — they are cheap-to-add,
expensive-to-add-later guards. The line is: avoid building
**scaffolding for features**, but keep using **escape hatches for
evolution**.

## How code-ranker detects YAGNI violations

YAGNI is the hardest to detect because the violation depends on
**who uses what** in the future, which is unknowable. Code Ranker
flags *present-day signals* via static import-graph analysis:

| Signal | YAGNI interpretation |
|---|---|
| `interface` with 1 in-workspace `implements` / structural conformer | Possible speculative interface. |
| `export` with no out-of-package importers | Possible speculative export; demote to file-local. |
| Generic parameter that appears only in the signature, never in the body or return type | Phantom generic; drop it. |
| Dependencies in `package.json` not imported anywhere in `src/` | Dead dep; remove from `package.json`. |
| Module that is not imported by any other module (and isn't an entry point) | Dead module; delete. |
| `enum` variant or union member never referenced outside its declaration | Dead variant; delete. |
| Env-flag string (`process.env.FOO`) referenced in code but never set in any `.env*`, CI config, or deployment manifest | Dead flag branch. |
| `async` function whose body has no `await` and whose callers all `await` immediately | Speculative `Promise`; consider sync. |
| Barrel file re-exporting symbols that no external importer pulls | Speculative public surface. |

A future rule **`unused-export`**: any `export`ed symbol that has no
importer outside the defining file can be demoted to file-local.
Severity low; confidence high. Tools like `knip` and `ts-prune`
already cover much of this; Code Ranker adds the YAGNI framing —
*why* it matters and *what* to do about it.

## Suggested recommendation template

> **YAGNI candidate**: function `processWithRetries` is exported
> from `packages/worker/src/index.ts` but has no importers outside
> `packages/worker`. If no external consumer is planned, drop the
> re-export and keep it module-local. Every exported symbol is a
> contract; the smaller your public surface, the more freedom you
> have to refactor.
>
> Reference: Fowler, "Yagni" — <https://martinfowler.com/bliki/Yagni.html>

## Related principles

- [KISS](kiss.md) — KISS is the *what*: pick the simpler design.
  YAGNI is the *when*: don't pick a design before you need it.
- [DRY](dry.md) — premature DRY violates YAGNI (extracting a helper
  for a hypothetical second caller). See Dan Abramov, "Goodbye,
  Clean Code".
- [OCP](solid-open-closed.md) — OCP demands extension points;
  YAGNI says don't build extension points speculatively. They are
  in tension; resolve with the reversibility test.

## References

1. Jeffries, R. "You're NOT Gonna Need It!". 1998.
   <https://ronjeffries.com/xprog/articles/practices/pracnotneed/>
2. Beck, K. *Extreme Programming Explained*. 1999.
3. Brooks, F. *The Mythical Man-Month*, anniversary ed. 1995 —
   especially "The Second-System Effect".
4. Ousterhout, J. *A Philosophy of Software Design*. 2018.
5. Fowler, M. "Yagni". 2015.
   <https://martinfowler.com/bliki/Yagni.html>
6. Metz, S. "The Wrong Abstraction". 2016.
   <https://sandimetz.com/blog/2016/1/20/the-wrong-abstraction>
7. Abramov, D. "Goodbye, Clean Code". 2020.
   <https://overreacted.io/goodbye-clean-code/>
8. Hyrum's Law: <https://www.hyrumslaw.com/> — every observable
   behaviour of your system will be depended upon, which is why
   speculative `export` is so dangerous in a structurally-typed
   language.
