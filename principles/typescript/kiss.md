# KISS — Keep It Simple, Stupid (in TypeScript)

**TL;DR**: When choosing between two designs that solve the problem,
pick the simpler one. In TypeScript, this most often means: fewer
generic parameters, fewer interface abstractions, fewer indirection
layers, fewer barrel re-export hops. Reach for a discriminated union
before a class hierarchy; reach for a function before an interface;
reach for one object literal before a builder; reach for a plain
`type` before a conditional-type chain.

## Canonical sources

- Kelly Johnson (Lockheed Skunk Works, c. 1960): origin of the
  acronym in engineering folklore. <https://en.wikipedia.org/wiki/KISS_principle>
- Edsger Dijkstra, "The Humble Programmer" (1972 ACM Turing
  Award lecture): "Simplicity is prerequisite for reliability."
  <https://www.cs.utexas.edu/~EWD/transcriptions/EWD03xx/EWD340.html>
- Tony Hoare, "The Emperor's Old Clothes" (1980 Turing lecture):
  "I conclude that there are two ways of constructing a software
  design: One way is to make it so simple that there are obviously
  no deficiencies, and the other way is to make it so complicated
  that there are no obvious deficiencies."
  <https://dl.acm.org/doi/10.1145/358549.358561>
- John Ousterhout, *A Philosophy of Software Design* (2018, 2nd ed.
  2021): the concept of **cognitive load** as the modern KISS metric.
- Brian Kernighan: "Everyone knows that debugging is twice as hard
  as writing a program in the first place. So if you're as clever
  as you can be when you write it, how will you ever debug it?"
  (*The Elements of Programming Style*, 1978)
- Dan Abramov, "The Wet Codebase" (Deconstruct 2019): premature
  deduplication is a tax on future change. The cheapest fix to a
  duplicate is often to leave it duplicated.
  <https://overreacted.io/the-wet-codebase/>
- Anders Hejlsberg, interviews on TypeScript's design (Channel 9,
  TSConf keynotes): the type system exists to describe JavaScript
  as it is, not to impose a parallel calculus. Types are a tool,
  not the product.
- Kent C. Dodds, "AHA Programming" and "Avoid Hasty Abstractions":
  the cost of a wrong abstraction is higher than the cost of
  duplication. <https://kentcdodds.com/blog/aha-programming>

## The principle

KISS is the discipline of preferring **the boring solution that
works**. It is not "the shortest code". It is not the cleverest
type. It is "the design with the least surface area for surprise".

A TypeScript package violates KISS when:

- It introduces a generic where a function with a single concrete
  type would do.
- It introduces an `interface` where a discriminated union would do.
- It introduces a class hierarchy where a tagged record would do.
- It introduces a builder where an options object would do.
- It introduces a decorator/codegen step where a function would do.
- It introduces a conditional type chain where a plain `type` alias
  with a hand-written branch would do.
- It introduces a barrel `index.ts` whose only job is to re-export.
- It introduces an abstraction "in case" a second implementation
  arrives. (See [YAGNI](yagni.md).)

The complexity carries a cost: every additional layer is more code
to read, more types to remember, slower `tsc` runs, slower editor
hovers, and more chances for the compiler to point at the wrong
line in a deep inference failure.

Dijkstra: simplicity is a *prerequisite* for reliability. You
cannot build trustworthy code on top of a design that is too
complex to hold in your head — and TypeScript's type system makes
it easy to write designs nobody, including the author, can hold in
their head.

## Why it matters

Complexity is **superlinear** in its cost. Each additional
abstraction layer multiplies the reader's mental load: not just by
the size of the layer, but by the interactions with all the layers
above it. Ten layers of three concepts each is harder to understand
than one layer of thirty concepts, because the reader must hold
each layer's invariants in mind while reading the next.

Ousterhout's *Philosophy of Software Design* puts numbers to this:
he calls each non-obvious bit of code a "cognitive load token", and
proposes that good software design minimizes the sum of cognitive
load tokens across all the people who must read the code.

In a TypeScript monorepo, KISS is what keeps onboarding manageable.
A new engineer who can read your code without asking "why is this
an interface?", "where does this generic resolve?", "what does this
conditional type narrow to in this branch?" — that is KISS achieved.

> **In plain JS without types**: most KISS advice here applies
> unchanged to JavaScript — fewer files, fewer wrappers, fewer
> indirections. The TS-specific load (conditional types, deep
> generics, branded types) only appears once you have a type
> system to abuse.

## In TypeScript

The TypeScript and broader JS ecosystem has, sometimes against its
own instincts, a strong simplicity culture. The canonical examples:

### Standard-library and idiom examples of restraint

- **Optional values** are `T | undefined`, not `Maybe<T>` from a
  library. The language ships with `?.` and `??`; you do not need
  an algebraic-data-type layer on top.
- **Results** are either thrown exceptions (boring, ubiquitous) or
  a small discriminated union:
  `type Result<T, E> = { ok: true; value: T } | { ok: false; error: E }`.
  Either is fine; what is not fine is a `Result` library with a
  fluent `.mapOrElseAsync()` chain.
- **Arrays** are the universal sequence. `Array<T>` covers 95% of
  cases; reach for `Map`, `Set`, or a typed-array only when the
  array literally cannot do the job. (cf. matklad, "Almost Always
  Always Use a Vector".)
- **Records** (`Record<string, V>` or `Map<K, V>`) are one shape.
  There is no `SortedMap`/`OrderedMap`/`MultiMap` family in the
  language; if you need ordering, sort on the way out.

The TypeScript compiler itself is a useful role model: a few core
data shapes (`Node`, `Symbol`, `Type`), big `switch` statements on
`SyntaxKind`, very little inheritance.

### The simpler tool first

A useful mental ladder for choosing the simplest tool:

1. **Function** — does this need any state at all?
2. **Function returning an object literal** — does it need to bundle
   outputs? Use `as const` or `satisfies` for the return type.
3. **Module with closures** — does this object have state but only
   one instance?
4. **Class** — does this object have state with multiple instances?
5. **Class + interface** — does this object need to be substitutable
   at call sites?
6. **Interface with multiple impls** — do you actually have multiple
   implementations *today*?
7. **Generic over an interface** — is the variation in types or in
   behaviour? If behaviour only, take the interface by value
   (structural typing means you do not need `<T extends Foo>`).
8. **Conditional / mapped types** — is the type genuinely derived,
   or are you just avoiding writing it out?
9. **Decorators / codegen** — is the repetition large enough that a
   function with `NoInfer<T>` cannot express it?
10. **Custom build step (ts-morph, AST transform)** — is the
    transformation not expressible in any of the above?

Move down only when the rung you are on cannot do the job. Each step
adds significant cost — to readers, to `tsc` time, to debuggers, to
source maps.

### Boring infrastructure choices

The TypeScript ecosystem rewards boring choices:

- `zod` (or `valibot`) for runtime validation and inferred types,
  instead of hand-written parsers + duplicated interfaces.
- Native `async` / `Promise` for concurrency, instead of a
  homegrown effect system.
- `commander` or `yargs` for CLI parsing.
- Plain `Error` subclasses (`class NotFoundError extends Error`)
  for typed errors; one `AppError` discriminated union if you want
  exhaustive handling.
- `pino` for structured logging.
- `drizzle` or `prisma` for databases.
- `vitest` for tests.

Reach for these *before* writing your own. Your codebase becomes a
"normal TS codebase" that any new hire can read.

## Violations and remedies

### Anti-pattern: interface with one implementation

```ts
export interface UserRepository {
  findById(id: UserId): Promise<User | undefined>;
  save(u: User): Promise<void>;
}

export class PostgresUserRepository implements UserRepository {
  constructor(private readonly pool: Pool) {}
  async findById(id: UserId) { /* ... */ }
  async save(u: User) { /* ... */ }
}

// No other impl exists. There is no plan for another.
```

The interface is overhead with no payoff. Callers must depend on
the abstract symbol; tests must construct mocks; the editor jumps
through indirection on "Go to Definition".

### Idiomatic fix: drop the interface until a second impl exists

```ts
export class UserRepository {
  constructor(private readonly pool: Pool) {}
  async findById(id: UserId): Promise<User | undefined> { /* ... */ }
  async save(u: User): Promise<void> { /* ... */ }
}
```

When the second backend (an in-memory implementation for tests) is
*actually written*, extract an interface — or rely on structural
typing and `Pick<UserRepository, "findById">` at the test site.
Until then, the concrete class is simpler in every way.

### Anti-pattern: deep generic chain

```ts
export function process<
  S extends AppStateLike,
  R extends UserRepository,
  C extends Cache<UserId, User>,
  M extends MetricsRecorder,
>(state: S, repo: R, cache: C, metrics: M): Promise<void> {
  /* ... */
}
```

Four generic parameters, four constraints. Calling code is verbose;
hover tooltips become unreadable; inference cascades on small
changes; `NoInfer` patches start appearing.

### Idiomatic fix: pass an `AppState` carrying wired collaborators

```ts
export interface AppState {
  repo: UserRepository;
  cache: Cache<UserId, User>;
  metrics: MetricsRecorder;
}

export async function process(state: AppState): Promise<void> { /* ... */ }
```

Structural typing means callers do not need to thread bounds. The
overhead of one extra property lookup is nothing. Reach for full
generic parameterization only when you genuinely need the type
relationship preserved across arguments.

### Anti-pattern: builder with one configurable field

```ts
export class ClientBuilder {
  private _timeout?: number;
  timeout(t: number): this { this._timeout = t; return this; }
  build(): Client { return new Client(this._timeout ?? DEFAULT_TIMEOUT); }
}
```

A builder buys you flexibility for *N* knobs. With 1, it is
busywork.

### Idiomatic fix: an options object with a default

```ts
export interface ClientOptions {
  timeout?: number;
}

export class Client {
  readonly timeout: number;
  constructor(opts: ClientOptions = {}) {
    this.timeout = opts.timeout ?? DEFAULT_TIMEOUT;
  }
}
```

One constructor call. No fluent API. Add a builder when there are
4+ optional knobs *and* the call sites visibly suffer (e.g.
conditional construction). Options objects scale further than
people expect.

### Anti-pattern: decorator/codegen for what a function can do

```ts
@validated(UserSchema)
@logged("createUser")
@traced
async function createUser(input: unknown): Promise<User> { /* ... */ }
```

Stage-3 decorators are useful, but they hide control flow. They
break "Go to Definition" on the wrapped function, they interfere
with source maps, and three of them stacked make the actual logic
hard to find.

### Idiomatic fix: explicit composition

```ts
async function createUser(rawInput: unknown): Promise<User> {
  const input = UserSchema.parse(rawInput);
  logger.info({ op: "createUser" });
  return tracer.span("createUser", async () => { /* ... */ });
}
```

Slightly more lines. Every step visible. Reach for decorators when
the same three steps wrap fifty functions and the noise dominates.

### Anti-pattern: conditional-type chain for a value the author knows

```ts
type RouteParams<P extends string> =
  P extends `${infer _Pre}:${infer Param}/${infer Rest}`
    ? { [K in Param | keyof RouteParams<Rest>]: string }
    : P extends `${infer _Pre}:${infer Param}`
      ? { [K in Param]: string }
      : {};
```

Impressive. Also: editor hovers go ten lines deep, errors point at
template-literal positions nobody can read, and the next maintainer
will not touch it.

### Idiomatic fix: write the types out, or accept `Record<string, string>`

```ts
interface UserRouteParams { id: string }
type Params = Record<string, string>;
```

Type-level computation is a power tool; use it where it pays for
itself (e.g. one widely-used helper). Do not use it inline in
twelve features.

### Anti-pattern: barrel `index.ts` whose only job is re-export

```ts
// src/index.ts
export * from "./user";
export * from "./order";
export * from "./payment";
export * from "./shipping";
// 40 more lines
```

Barrels look tidy, but they tank tree-shaking, slow `tsc`, hide
circular imports, and force readers to grep the source file to
find a definition the editor *could* have jumped to directly.

### Idiomatic fix: import from the actual file

```ts
import { User } from "./user/types.js";
import { createOrder } from "./order/create.js";
```

Allow barrels only at package boundaries (the package's public
entry point), never internally.

### Anti-pattern: branded type for a value used in one place

```ts
type UserId = string & { readonly __brand: "UserId" };
function makeUserId(s: string): UserId { return s as UserId; }
```

If the type is only ever passed from `repo.findById` to a single
response object, the brand is pure ceremony. Branded types pay off
when the value crosses many module boundaries and confusion with
adjacent strings is realistic (e.g. `UserId` vs `OrgId`).

### Idiomatic fix: just `string`, with a clear parameter name

```ts
async function findById(userId: string): Promise<User | undefined> { /* ... */ }
```

Add the brand when you have a second `Id` type *and* you have seen
or expect mix-ups.

## KISS at the package level

The KISS-friendly TypeScript monorepo:

- Has a flat `packages/` layout (one level), not a deep tree of
  `packages/core/internal/utils/...`.
- Has package names that match what they do (no `@org/core`,
  `@org/common`, `@org/utils` — be specific: `@org/string-ops`,
  `@org/time-helpers`).
- Has fewer than ~20 runtime dependencies in most `package.json`s.
- Has a single root `tsconfig.base.json` whose settings the
  per-package `tsconfig.json`s `extend`; no per-package
  `compilerOptions` overrides except `outDir` / `references`.
- Has shallow `paths`/`references` — every alias is one hop.
- Has a `README.md` per package that explains in three paragraphs
  what the package does and what its main exports are.

## How code-ranker detects KISS violations

KISS is qualitative; code-ranker detects its *quantitative shadows*:

| Signal | KISS interpretation |
|---|---|
| `package.json` with many feature flags / conditional exports and few users of each | Speculative complexity. |
| Interface with one implementor in the workspace | Speculative abstraction. |
| Function with 3+ generic parameters or 3+ `extends` constraints | Caller-side complexity. |
| Module nesting deeper than 4 levels under `src/` | Navigation friction. |
| `tsconfig.json` `paths` entries pointing more than 2 directories deep | Layered indirection. |
| `package.json` runtime-dependency count above project median × 2 | Heavy dependency footprint. |
| `index.ts` whose body is exclusively `export * from "..."` lines | Barrel-only re-export. |
| Conditional-type chains longer than ~3 `extends` branches | Type-level overengineering. |

A future rule **`single-impl-interface`**: when an in-workspace
`interface` (or abstract class) has exactly one implementor in the
same monorepo, suggest collapsing. Severity low, confidence medium
(the human can verify whether a second impl is planned).

A future rule **`barrel-only-index`**: when an `index.ts` is
exclusively wildcard re-exports and has no public-API justification,
suggest direct imports.

## Suggested recommendation template

> **KISS candidate**: interface `UserRepository` has exactly one
> implementation (`PostgresUserRepository`) in this workspace. If
> no second implementation is planned, consider collapsing the
> interface into the class directly. The current shape adds an
> indirection at every call site (and every test mock) without a
> corresponding benefit.
>
> Source: KISS — Hoare, "The Emperor's Old Clothes" (1980);
> Kent C. Dodds, "AHA Programming"; Dan Abramov, "The Wet Codebase".

## Related principles

- [YAGNI](yagni.md) — KISS and YAGNI overlap heavily; YAGNI is
  scoped to features-you-haven't-used-yet.
- [SRP](solid-single-responsibility.md) — KISS at the module level
  often *is* SRP applied.
- [Composition Over Inheritance](composition-over-inheritance.md)
  — composition tends to be simpler than the alternative, and TS's
  structural typing makes composition essentially free.

## References

1. Dijkstra, E. W. "The Humble Programmer". 1972 ACM Turing Award.
   <https://www.cs.utexas.edu/~EWD/transcriptions/EWD03xx/EWD340.html>
2. Hoare, C. A. R. "The Emperor's Old Clothes". 1980 Turing lecture.
   <https://dl.acm.org/doi/10.1145/358549.358561>
3. Ousterhout, J. *A Philosophy of Software Design*. 2nd ed., 2021.
4. Kernighan, B. *The Elements of Programming Style*. 1978.
5. Brooks, F. *The Mythical Man-Month* (anniversary ed.) — the
   "second-system effect" describes the failure mode KISS guards
   against.
6. Abramov, D. "The Wet Codebase". Deconstruct, 2019.
   <https://overreacted.io/the-wet-codebase/>
7. Dodds, K. C. "AHA Programming" and "Avoid Hasty Abstractions".
   <https://kentcdodds.com/blog/aha-programming>
8. Hejlsberg, A. Various TypeScript design talks (TSConf, Channel 9).
