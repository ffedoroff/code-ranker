# ISP — Interface Segregation Principle (in TypeScript)

**TL;DR**: Clients should not be forced to depend on methods (or
properties) they do not use. In TypeScript: prefer many small,
structurally-typed interfaces and accept the *narrowest* shape a
function actually reads. Let consumers ask for `Iterable<T>` rather
than `Array<T>`, and for `{ name: string }` rather than the full
`User`.

## Canonical sources

- Robert C. Martin, "The Interface Segregation Principle" (1996):
  <https://web.archive.org/web/20060822033314/http://www.objectmentor.com/resources/articles/isp.pdf>
- Martin, *Clean Architecture*, Ch. 10.
- TypeScript Handbook, "Type Compatibility" (structural typing):
  <https://www.typescriptlang.org/docs/handbook/type-compatibility.html>
- TypeScript Handbook, "Utility Types" (`Pick`, `Omit`, `Partial`):
  <https://www.typescriptlang.org/docs/handbook/utility-types.html>
- TypeScript Handbook, "Iterators and Generators":
  <https://www.typescriptlang.org/docs/handbook/iterators-and-generators.html>
- React docs, "Passing Props to a Component" (narrow prop interfaces):
  <https://react.dev/learn/passing-props-to-a-component>

## The principle

A type that bundles too many members forces consumers to depend on
all of them even when only one is needed. Test doubles, mocks, and
substitute implementations all pay the cost of the largest member
of the interface.

TypeScript's structural typing makes ISP *very natural*: a function
parameter type is, by definition, "the shape this function reads".
A function that asks for `{ id: string }` accepts any object that
has an `id: string` — you do not need to declare conformance. So
ISP in TypeScript is less about "splitting a nominal `interface`"
and more about "type the parameter as the slice you actually use".

The failure modes:

- A consumer types its parameter as a fat `User` when it only reads
  `user.email` — every call site must construct or mock a full `User`.
- A React component declares `props: AppState` and "drills" the
  entire app state through, when it only renders `state.user.name`.
- A class implements a wide `interface` and stubs methods with
  `throw new Error("not implemented")` because the backend cannot
  support them.
- A "god" service object is passed around when callers only invoke
  one of its twenty methods.

ISP says: **fold a fat interface into several thin ones**, or better
yet **let each function declare its own minimal parameter shape**,
which structural typing will satisfy automatically.

## Why it matters

In a workspace with many implementors and many consumers, fat
interfaces create a **double bind**:

1. **Implementors are penalized** — they must satisfy every member
   even when only one is meaningful for their backend.
2. **Consumers are penalized** — they cannot reuse a function in
   contexts where a smaller object is available (e.g. a function
   typed `(u: User) => string` cannot be called with a freshly
   parsed `{ name: string }` from a CSV row).

ISP interacts strongly with
[LSP](solid-liskov-substitution.md): small interfaces have small
contracts that are easier to write down and honour. A 12-property
interface has a 12-fold larger contract surface.

In TypeScript specifically, ISP is the **type-shape counterpart of
SRP**: SRP segregates *modules*; ISP segregates *types*.

## In TypeScript

TypeScript's design rewards ISP-style factoring at every level.

### The iterable ladder

```ts
interface Iterable<T>         { [Symbol.iterator](): Iterator<T>; }
interface Iterator<T>         { next(): IteratorResult<T>; }
interface IterableIterator<T> extends Iterator<T>, Iterable<T> {}
interface Array<T>            extends ReadonlyArray<T> { /* push, splice, … */ }
```

A function that sums numbers asks for `Iterable<number>`. A function
that needs random access asks for `ReadonlyArray<number>`. A
function that mutates asks for `Array<number>`. Each consumer
declares exactly the surface it needs.

```ts
function sum(xs: Iterable<number>): number {
  let s = 0;
  for (const x of xs) s += x;
  return s;
}
```

`sum` accepts `number[]`, `Set<number>`, a generator, a `Map`'s
`.values()` — anything iterable. Typing the parameter as
`number[]` would have rejected all but the first.

### Narrow parameter shapes via `Pick`

```ts
interface User {
  id: string;
  email: string;
  passwordHash: string;
  createdAt: Date;
  preferences: UserPreferences;
  billing: BillingProfile;
  // … 20 more fields
}

function greeting(u: Pick<User, "email">): string {
  return `Hello ${u.email}`;
}
```

`greeting` documents at the type level that it only reads `email`.
Tests can pass `{ email: "x@y" }` without constructing a full `User`.

`Omit<T, K>` is the inverse — useful when *almost everything* is
needed except a few sensitive fields:

```ts
type PublicUser = Omit<User, "passwordHash" | "billing">;
```

### Callbacks as narrow function signatures

Do not accept a god service object when you only call one method:

```ts
// Bad — caller must supply the whole logger
function processOrder(o: Order, logger: Logger) {
  logger.info("processing", o.id);
}

// Good — caller supplies just a function
function processOrder(o: Order, log: (msg: string) => void) {
  log(`processing ${o.id}`);
}
```

The second form composes with `console.log`, with a test spy, with
a logger method via `logger.info.bind(logger)` — no fat interface
required.

### React: prop drilling vs narrow prop interfaces

```tsx
// Bad — Avatar depends on the entire User
function Avatar({ user }: { user: User }) {
  return <img src={user.avatarUrl} alt={user.name} />;
}

// Good — Avatar declares exactly what it renders
interface AvatarProps { name: string; avatarUrl: string; }
function Avatar(props: AvatarProps) {
  return <img src={props.avatarUrl} alt={props.name} />;
}
```

The second form is testable with a literal `{ name: "x", avatarUrl: "y" }`,
reusable for non-`User` entities (a `Team`, a `Bot`), and survives
unrelated changes to `User`.

The same applies to **god `Context` values**:

```tsx
// Bad — every consumer re-renders when any field changes
const AppContext = createContext<{
  user: User; theme: Theme; cart: Cart; flags: Flags; analytics: Analytics;
}>(/* … */);

// Good — segregate by concern
const UserContext = createContext<User | null>(null);
const ThemeContext = createContext<Theme>("light");
const CartContext = createContext<Cart>(emptyCart);
```

Consumers subscribe only to the slice they read; re-render
invalidation follows the same segregation.

### `Partial<T>` as an ISP smell

`Partial<T>` is sometimes legitimate (e.g. patch objects). But when
it appears in a *consumer* signature it often means "I take this
whole interface but don't promise to use all of it" — i.e. the
parameter type is wider than the function's true contract:

```ts
// Smell — what does this function actually require?
function render(opts: Partial<RenderOptions>) { /* … */ }

// Better — say exactly what is read, with defaults at the boundary
function render(opts: { width?: number; height?: number }) { /* … */ }
```

If `RenderOptions` has 30 fields and `render` reads two, `Partial`
is hiding an ISP violation.

## Violations and remedies

### Anti-pattern: fat interface covering every backend feature

```ts
interface Database {
  query(sql: string): Promise<Row[]>;
  execute(sql: string): Promise<number>;
  beginTransaction(): Promise<Tx>;
  commit(tx: Tx): Promise<void>;
  rollback(tx: Tx): Promise<void>;
  migrate(m: Migration): Promise<void>;
  dump(): Promise<Buffer>;
  restore(b: Buffer): Promise<void>;
  vacuum(): Promise<void>;
  metrics(): DbMetrics;
  health(): Health;
  subscribe(channel: string): AsyncIterable<Notification>;
}
```

A `SqliteDatabase` is forced to fake `subscribe` (no pub/sub). A
read-only replica is forced to fake `execute`. A migration runner
that only needs `migrate` must accept the whole surface.

### Idiomatic fix: split by capability

```ts
interface Query        { query(sql: string): Promise<Row[]>; }
interface Execute      { execute(sql: string): Promise<number>; }
interface Transactional { begin(): Promise<Tx>; commit(t: Tx): Promise<void>; rollback(t: Tx): Promise<void>; }
interface Migratable   { migrate(m: Migration): Promise<void>; }
interface Backup       { dump(): Promise<Buffer>; restore(b: Buffer): Promise<void>; }
interface Observability { metrics(): DbMetrics; health(): Health; }
interface PubSub       { subscribe(ch: string): AsyncIterable<Notification>; }
```

A concrete `SqliteDatabase` implements `Query & Execute & Transactional & Migratable & Backup & Observability` but not `PubSub`.
The migration runner asks for `Migratable`. A read-only replica's
return type is `Query & Observability`.

If consumers commonly need three or four together, define an
intersection alias as a *convenience*:

```ts
type DatabaseFull = Query & Execute & Transactional & Migratable;
```

But avoid making `DatabaseFull` the *primary* type — it should be
shorthand over the segregated parts.

### Anti-pattern: god `Service` interface

```ts
interface UserService {
  create(input: CreateUserInput): Promise<User>;
  deactivate(id: string): Promise<void>;
  rotatePassword(id: string): Promise<void>;
  exportGdpr(id: string): Promise<Buffer>;
  sendWelcomeEmail(id: string): Promise<void>;
  assignRole(id: string, role: Role): Promise<void>;
}
```

A test that only needs `create` must mock all six methods. A
notification module that only invokes `sendWelcomeEmail` must
accept a full `UserService`.

### Idiomatic fix: interfaces per use case

```ts
interface CreateUser     { create(input: CreateUserInput): Promise<User>; }
interface DeactivateUser { deactivate(id: string): Promise<void>; }
interface RotatePassword { rotatePassword(id: string): Promise<void>; }
interface GdprExport     { export(id: string): Promise<Buffer>; }
interface WelcomeMailer  { welcome(id: string): Promise<void>; }
interface RoleAssigner   { assign(id: string, role: Role): Promise<void>; }
```

The concrete `UserService` class implements all six; consumers
accept the interface they actually need. Mocks become trivially
small — a one-method object literal.

### Anti-pattern: method that throws "not implemented"

```ts
interface Cache {
  get(k: string): Promise<Buffer | undefined>;
  put(k: string, v: Buffer): Promise<void>;
  evict(k: string): Promise<void>;
  evictAll(): Promise<void>;
  ttlSeconds(): number | undefined;
}

class FakeCacheForTest implements Cache {
  async get(k: string)         { return this.data.get(k); }
  async put(k: string, v: Buffer) { this.data.set(k, v); }
  async evict()                { throw new Error("not implemented"); }   // smell
  async evictAll()             { throw new Error("not implemented"); }   // smell
  ttlSeconds()                 { return undefined; }
}
```

The thrown errors are runtime ISP debt — the interface is too broad
for the test's needs.

### Idiomatic fix: split

```ts
interface CacheGet   { get(k: string): Promise<Buffer | undefined>; }
interface CachePut   { put(k: string, v: Buffer): Promise<void>; }
interface CacheEvict { evict(k: string): Promise<void>; evictAll(): Promise<void>; }
interface CacheTtl   { ttlSeconds(): number | undefined; }
```

The test fake implements only `CacheGet & CachePut`.

## ISP at the package level

The same principle applies to **npm packages and barrel files**: a
package's public surface should be focused. The classic
anti-pattern is a "kitchen sink" package (`utils`, `common`,
`helpers`) that becomes a dependency of everything and hard to
update.

Apply ISP at the package level by splitting:

```
@org/utils/                ←  becomes  →   @org/string-utils/
                                           @org/time-utils/
                                           @org/collection-utils/
```

A barrel `index.ts` that re-exports 200 symbols has the same
problem inside a single package: every consumer pulls the
union of types, breaking tree-shaking and inflating type-check
times.

## How code-ranker detects ISP violations

The structural signals:

| Signal | ISP interpretation |
|---|---|
| `interface`/`type` with > N members (high property/method count) | Possible fat interface. Threshold tunable per project. |
| Fat type alias with N members + downstream files only touching M ≪ N | Strong ISP smell — most consumers want a smaller surface. |
| Parameter typed `T` but function body only reads a small subset of `T`'s keys | Candidate for `Pick<T, …>` or a fresh narrow interface. |
| Multiple `implements`/object-literal sites throw "not implemented" or return `undefined as any` | Direct ISP smell. |
| Re-exporting barrel imported by N modules, each using ≤ 2 symbols | "Kitchen sink" package. |
| React component takes a wide prop type but renders only a few fields | Prop-drilling / ISP violation at the component boundary. |

A concrete future rule code-ranker could add:

**`fat-interface`**: `interface` or `type` has ≥ 7 members AND has
≥ 2 implementations or consumers in the workspace AND the majority
of call sites read ≤ 2 members. Severity: low / medium.

## Suggested recommendation template

> **ISP candidate**: interface `Database` exposes 12 methods and
> has 4 implementations across the workspace. Several
> implementations throw "not implemented" for methods their
> backend cannot support; 6 of 9 call sites use only 1-2 methods.
> Split into capability-segregated interfaces (`Query`, `Execute`,
> `Transactional`, `Migratable`, `Backup`, `PubSub`) and let each
> consumer ask for exactly the capabilities it needs. Where a
> single function reads only a few fields of a record, type its
> parameter with `Pick<T, …>` or a fresh narrow interface.
>
> References:
>  - <https://web.archive.org/web/20060822033314/http://www.objectmentor.com/resources/articles/isp.pdf>
>  - <https://www.typescriptlang.org/docs/handbook/type-compatibility.html>

## Related principles

- [SRP](solid-single-responsibility.md) — SRP segregates *modules*;
  ISP segregates *interfaces and parameter types*. They reinforce
  each other.
- [LSP](solid-liskov-substitution.md) — small interfaces have small
  contracts; ISP makes LSP affordable.
- [DIP](solid-dependency-inversion.md) — DIP wants consumers to
  depend on interfaces; ISP keeps those interfaces small enough to
  be worth depending on.
- [Composition Over Inheritance](composition-over-inheritance.md)
  — composing small interface intersections (`Read & Seek`) is the
  TypeScript expression of "compose, don't inherit".

## References

1. Martin, R. C. "The Interface Segregation Principle". 1996.
   <https://web.archive.org/web/20060822033314/http://www.objectmentor.com/resources/articles/isp.pdf>
2. Martin, R. C. *Clean Architecture*. Ch. 10.
3. TypeScript Handbook, "Type Compatibility".
   <https://www.typescriptlang.org/docs/handbook/type-compatibility.html>
4. TypeScript Handbook, "Utility Types".
   <https://www.typescriptlang.org/docs/handbook/utility-types.html>
5. TypeScript Handbook, "Iterators and Generators".
   <https://www.typescriptlang.org/docs/handbook/iterators-and-generators.html>
6. React docs, "Passing Props to a Component".
   <https://react.dev/learn/passing-props-to-a-component>
