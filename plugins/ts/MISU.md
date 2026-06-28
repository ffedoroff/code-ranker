# Make Invalid States Unrepresentable (in TypeScript)

**TL;DR**: Move correctness from runtime checks into the type system.
A `User` cannot have a missing email; a `Connection` cannot be queried
before being opened; a parsed response cannot also be a parse error.
TypeScript is arguably the **sweet spot** for this principle in
mainstream languages: discriminated unions, literal types, branded
types, `readonly`, exhaustiveness checks, and runtime-validator
libraries like Zod combine to let you encode an unusual amount of
domain truth in types — while still emitting plain JavaScript.

## Canonical sources

- Yaron Minsky, "Effective ML: Make Illegal States Unrepresentable"
  (2010). The phrase originates here.
  <https://blog.janestreet.com/effective-ml-revisited/>
- Alexis King, "Parse, don't validate" (2019):
  <https://lexi-lambda.github.io/blog/2019/11/05/parse-don-t-validate/>
- Scott Wlaschin, *Domain Modeling Made Functional* (Pragmatic
  Bookshelf, 2018) — the F# techniques translate almost 1:1 to TS.
- Matt Pocock, "TypeScript essays" — practical TS idioms, branded
  types, discriminated unions, `satisfies`.
  <https://www.totaltypescript.com/>
- Effect-TS documentation — production-grade `Effect`, `Option`,
  `Either`, `Schema`. <https://effect.website/>
- Zod documentation — the runtime⇄type bridge. <https://zod.dev/>

## The principle

Two designs of the same feature can differ dramatically in how many
runtime checks they require.

**Design A** (invalid states representable):

```ts
interface User {
  email?: string;          // may be undefined
  age?: number;            // may be undefined; may also be -3 or 9999
  role: string;            // any string at all
}

function sendBirthdayEmail(u: User): void {
  if (!u.email) throw new Error("user without email?!");
  if (u.age == null) throw new Error("user without age?!");
  if (u.role === "admin" || u.role === "Admin" || u.role === "ADMIN") {
    // role is a string, so every case must be checked
  }
  // ...
}
```

**Design B** (invalid states *unrepresentable*):

```ts
interface User {
  readonly email: Email;       // always present, parsed at construction
  readonly age: Age;           // branded; guaranteed 0..150
  readonly role: Role;         // "admin" | "member" | "guest"
}

function sendBirthdayEmail(u: User): void {
  const email = u.email;       // no undefined check
  const age = u.age;           // no range check
  if (u.role === "admin") {
    // role is a literal union, exhaustively switchable
  }
}
```

Design A pushes correctness onto every caller. Design B pushes it
to `User`'s construction — once, at the boundary. After that, the
compiler enforces the invariants.

Minsky's principle: **make invalid states syntactically impossible**.
King's reformulation: **parse, don't validate** — convert raw input
into a type that carries the proof of validity, then never
re-validate.

## Why TypeScript is the sweet spot

- **Structural + literal types**: `"admin" | "member" | "guest"` is
  a first-class type. No enum boilerplate.
- **Discriminated unions** match Rust enums in expressive power for
  most use cases, with no extra syntax.
- **Exhaustiveness via `never`** — the `assertNever` pattern gives
  you compile errors when you forget a case.
- **`strictNullChecks`** turns `T | undefined` into a real distinction
  the compiler enforces.
- **Zod / Valibot / Effect Schema** parse JSON at the boundary and
  return precisely-typed values; no parallel "validator + interface"
  drift.
- **`readonly`, `as const`, `satisfies`** lock down literal-ness and
  immutability without runtime cost.

The price: TypeScript types are erased at runtime. You need a
runtime validator (Zod, etc.) at every untrusted boundary. Inside
the boundary, the type system is your truth.

## In TypeScript

### 1. Discriminated unions instead of stringly-typed enums

```ts
// Bad
interface Request {
  method: string;           // "GET", "POST", "get", "POSt", ...
  body?: Uint8Array;
  url: string;
}

// Good
type Request =
  | { kind: "get";    url: URL }
  | { kind: "post";   url: URL; body: Uint8Array }
  | { kind: "delete"; url: URL };
```

A `{ kind: "get" }` literally cannot have a body — the variant has
no `body` field. The state "GET with a body" is unrepresentable.

When you `switch` on `kind`, TS narrows each branch:

```ts
function send(req: Request): void {
  switch (req.kind) {
    case "get":    return doGet(req.url);
    case "post":   return doPost(req.url, req.body);
    case "delete": return doDelete(req.url);
    default:       return assertNever(req);
  }
}

function assertNever(x: never): never {
  throw new Error(`Unhandled variant: ${JSON.stringify(x)}`);
}
```

Add a new variant later and every `switch` lacking it becomes a
compile error. This is the closest TS analog to Rust's
`#[non_exhaustive]` + exhaustive `match`.

### 2. Branded (newtype) types

TypeScript is structural — two `string`s are interchangeable. Brands
restore nominal-ish typing:

```ts
declare const __brand: unique symbol;
type Brand<T, B> = T & { readonly [__brand]: B };

export type Email  = Brand<string, "Email">;
export type UserId = Brand<string, "UserId">;
export type Age    = Brand<number, "Age">;

export function parseEmail(raw: string): Email | undefined {
  if (!/^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(raw)) return undefined;
  return raw as Email;
}

export function parseAge(n: number): Age | undefined {
  if (!Number.isInteger(n) || n < 0 || n > 150) return undefined;
  return n as Age;
}
```

The only way to obtain an `Email` is through `parseEmail`. The cast
inside `parseEmail` is a contained, audited act; everywhere else,
`Email` is a proof of validity. (See also
[Composition over Inheritance](CoI.md) for
brand-as-newtype context.)

Equivalent shorter form, no symbol:

```ts
type UserId = string & { readonly __userId: unique symbol };
```

### 3. `T | undefined` instead of nullable bags

`strictNullChecks: true` (and ideally `exactOptionalPropertyTypes`)
is non-negotiable. With it, `string | undefined` and `string` are
different types and the compiler tracks them.

```ts
function greet(name: string): string {
  return `Hello, ${name.toUpperCase()}`;  // safe; not "Hello, undefined"
}

function maybeGreet(name: string | undefined): string {
  if (name === undefined) return "Hello, stranger";
  return greet(name);                     // narrowed
}
```

Pick a convention (`undefined` xor `null`) and stick to it. Most
codebases prefer `undefined` for "absent" because that's what
optional properties and missing JSON keys produce.

### 4. `Result<T, E>` as a discriminated union

```ts
export type Result<T, E> =
  | { readonly ok: true;  readonly value: T }
  | { readonly ok: false; readonly error: E };

export const ok    = <T>(value: T):  Result<T, never> => ({ ok: true,  value });
export const err   = <E>(error: E): Result<never, E> => ({ ok: false, error });

function parseUser(raw: unknown): Result<User, ParseError> {
  // ...
}

const r = parseUser(input);
if (r.ok) {
  use(r.value);    // T
} else {
  log(r.error);    // E
}
```

For more structure, reach for `neverthrow`, `fp-ts` (`Either`), or
`effect-ts` (`Effect.Either`). They add `.map`, `.flatMap`,
combinator chains, and async variants.

### 5. Typestate via phantom type parameters

State machines whose legal operations differ per state:

```ts
type State = "open" | "closed";

class Connection<S extends State> {
  private constructor(private readonly socket: Socket, private readonly _s: S) {}

  static create(): Connection<"closed"> {
    return new Connection(makeSocket(), "closed" as const);
  }

  open(this: Connection<"closed">): Connection<"open"> {
    this.socket.connect();
    return new Connection(this.socket, "open" as const);
  }

  query(this: Connection<"open">, sql: string): Rows {
    return this.socket.execute(sql);
  }

  close(this: Connection<"open">): Connection<"closed"> {
    this.socket.end();
    return new Connection(this.socket, "closed" as const);
  }
}

const c = Connection.create();
c.query("SELECT 1");      // compile error: query requires <"open">
const open = c.open();
open.query("SELECT 1");   // ok
```

The `this` parameter constrains which methods exist on which state.
`query` on a `Connection<"closed">` does not type-check.

### 6. Replace boolean parameters with literal unions

```ts
// Bad
function save(record: Record, force: boolean): void {}
// Call sites read: save(r, true) -- what does `true` mean here?

// Good
function save(record: Record, behaviour: "errorIfExists" | "overwriteIfExists"): void {}
save(r, "overwriteIfExists");
```

Self-documenting at the call site, exhaustively switchable inside.

### 7. `as const` and `satisfies` to lock literal-ness

```ts
// Without `as const`, TS widens to string[]
const ROLES = ["admin", "member", "guest"] as const;
type Role = typeof ROLES[number];   // "admin" | "member" | "guest"

const config = {
  retries: 3,
  mode: "strict",
} satisfies { retries: number; mode: "strict" | "lax" };
// `config.mode` keeps type "strict", not widened to "strict" | "lax"
```

`satisfies` is the TS 4.9+ tool for "check this conforms to a type,
but keep the narrow inferred type". Use it to retain literal types
on configuration objects.

### 8. Zod (or Effect Schema, Valibot, ArkType) at the boundary

```ts
import { z } from "zod";

const UserSchema = z.object({
  email: z.string().email().brand<"Email">(),
  age:   z.number().int().min(0).max(150).brand<"Age">(),
  role:  z.enum(["admin", "member", "guest"]),
});
export type User = z.infer<typeof UserSchema>;

export function parseUser(raw: unknown): User {
  return UserSchema.parse(raw);   // throws on invalid; or .safeParse for Result
}
```

`User` is inferred *from the schema*. There is no parallel
hand-written interface to drift. Parse once at the boundary; the
internal code sees only `User`.

This is the cleanest TS realization of King's "parse, don't
validate".

### 9. `readonly` everywhere; `Object.freeze` for runtime

```ts
interface OrderItem {
  readonly sku: SKU;
  readonly quantity: PositiveInt;
}
interface Order {
  readonly id: OrderId;
  readonly items: readonly OrderItem[];
}

const order: Order = Object.freeze({ id, items: Object.freeze([...]) });
```

`readonly` is type-only; `Object.freeze` enforces at runtime. Use
both where mutation would corrupt invariants. Libraries like Immer
or immutable.js fill the structural-sharing niche if you need it.

### 10. React: one state union instead of multiple booleans

```ts
// Bad
const [isLoading, setIsLoading] = useState(false);
const [isError,   setIsError]   = useState(false);
const [isSuccess, setIsSuccess] = useState(false);
const [data,      setData]      = useState<Data | null>(null);
// Representable: isLoading && isError && isSuccess all true at once.

// Good
type FetchState =
  | { status: "idle" }
  | { status: "loading" }
  | { status: "success"; data: Data }
  | { status: "error";   error: Error };

const [state, setState] = useState<FetchState>({ status: "idle" });
```

The "success but also loading" state cannot exist. Render code
becomes an exhaustive `switch` on `state.status`. (This is also the
shape XState produces.)

## Violations and remedies

### Anti-pattern: optional fields for required data

```ts
interface OrderRequest {
  customerId?: string;
  items?: Item[];
  total?: number;
}

function process(req: OrderRequest): void {
  if (!req.customerId) throw new Error("missing customer");
  if (!req.items)      throw new Error("missing items");
  if (req.total == null) throw new Error("missing total");
  // ...
}
```

Every consumer must check. The interface says "an order, maybe".

### Idiomatic fix: a wire schema and a domain type

```ts
const OrderRequestWire = z.object({
  customerId: z.string().uuid().optional(),
  items:      z.array(ItemSchema).optional(),
  total:      z.number().optional(),
});

const OrderRequest = z.object({
  customerId: CustomerId,
  items:      z.array(ItemSchema).min(1),
  total:      Money,
});
type OrderRequest = z.infer<typeof OrderRequest>;

function intoDomain(raw: z.infer<typeof OrderRequestWire>): OrderRequest {
  return OrderRequest.parse(raw);   // single validation step
}
```

After `intoDomain`, downstream code sees no optionals.

### Anti-pattern: state encoded in a flag

```ts
class Connection {
  private isOpen = false;
  query(sql: string): Rows {
    if (!this.isOpen) throw new Error("closed");
    // ...
  }
  close() { this.isOpen = false; }
}
```

Every method needs the `isOpen` check; the compiler cannot help.

### Idiomatic fix: typestate (see §5 above)

### Anti-pattern: parallel arrays that must stay in sync

```ts
interface Catalog {
  names:  string[];
  prices: number[];
  inStock: boolean[];
}
```

The invariant "lengths are equal" is unstated. Pushing to two
arrays but not the third desynchronizes silently.

### Idiomatic fix: one record per row

```ts
interface CatalogItem {
  readonly name: string;
  readonly price: Money;
  readonly inStock: boolean;
}
interface Catalog {
  readonly items: readonly CatalogItem[];
}
```

### Anti-pattern: `string` for "kind-of typed" identifiers

```ts
function deactivate(userId: string, by: string): Promise<void> { /* ... */ }
// deactivate(by, userId) -- arguments swapped, still compiles
```

### Idiomatic fix: branded types

```ts
type UserId  = Brand<string, "UserId">;
type AdminId = Brand<string, "AdminId">;

function deactivate(user: UserId, by: AdminId): Promise<void> { /* ... */ }
// Swapping fails to compile.
```

### Anti-pattern: builder whose `.build()` works on incomplete state

```ts
class UserBuilder {
  private email?: Email;
  private age?: Age;
  withEmail(e: Email) { this.email = e; return this; }
  withAge(a: Age)     { this.age = a;   return this; }
  build(): User {
    if (!this.email) throw new Error("email required");
    if (!this.age)   throw new Error("age required");
    return { email: this.email, age: this.age };
  }
}
```

Forgetting `.withEmail()` is a runtime crash.

### Idiomatic fix: typestate builder via phantom generics

```ts
type Unset = { readonly __unset: unique symbol };

class UserBuilder<E, A> {
  private constructor(private readonly e: E, private readonly a: A) {}
  static empty(): UserBuilder<Unset, Unset> {
    return new UserBuilder({} as Unset, {} as Unset);
  }
  withEmail(this: UserBuilder<Unset, A>, e: Email): UserBuilder<Email, A> {
    return new UserBuilder(e, this.a);
  }
  withAge(this: UserBuilder<E, Unset>, a: Age): UserBuilder<E, Age> {
    return new UserBuilder(this.e, a);
  }
  build(this: UserBuilder<Email, Age>): User {
    return { email: this.e, age: this.a };
  }
}

const u = UserBuilder.empty()
  .withEmail(email)
  .withAge(age)
  .build();   // ok; omit either step → compile error
```

(See [OCP](OCP.md) — adding a required field becomes
breaking, which is appropriate for genuinely required data.)

## When NOT to use this principle

- **API surface noise** — every branded ID adds a parser. For
  internal-only short-lived values, plain types may be fine.
- **Library boundaries** — exporting typestate or deeply branded
  types forces every consumer into your discipline. Sometimes a
  plain shape is friendlier.
- **Bundle size and compile time** — pathological generic and
  phantom-type chains slow `tsc` and confuse error messages.
- **Erasure** — TS types vanish at runtime. If you need the
  invariant *enforced* against untrusted callers (e.g., a published
  npm package), pair the type with a runtime check.

A pragmatic heuristic: **encode invariants that multiple consumers
need**. A single-function precondition may be cheaper as a
`assert()` call than a branded type.

## How code-ranker detects representable-invalid-state risk

Static AST analysis can flag structural smells:

| Signal | Interpretation |
|---|---|
| Many `?` optional properties on a single interface, all unwrapped/asserted by callers | "Parse, don't validate" candidate — split wire vs domain. |
| `as` casts to a non-`unknown` type | Possible escape hatch around a type the compiler couldn't prove. |
| Three+ booleans on the same state object | Likely a discriminated-union candidate (`isLoading`/`isError`/`isSuccess`). |
| String-typed identifier params across many call sites | Brand candidates. |
| Functions with multiple same-type parameters (e.g., `(s1: string, s2: string)`) | Swapping risk. |
| `switch` on a union without `default: assertNever(x)` | Missing exhaustiveness guard. |

Code Ranker's LLM-verification mode can also surface "this `useState`
has multiple booleans that should be a union" or "this `interface`
has many optionals; consider a Zod schema".

## Suggested recommendation template

> **Make-Invalid-States-Unrepresentable candidate**: interface
> `OrderRequest` has 5 optional fields, all of which downstream
> code non-null-asserts. This is a "parse, don't validate"
> candidate: define a Zod schema `OrderRequestWire` (all optional)
> for the boundary, and an inferred `OrderRequest` (all required)
> for the domain, with a single `OrderRequest.parse(wire)` step at
> the API edge.
>
> Source: King, "Parse, don't validate" (2019); Wlaschin, *Domain
> Modeling Made Functional* (2018).

## Related principles

- [LSP](LSP.md) — types that encode invariants
  make LSP contracts implicit. No JSDoc note saying "email must be
  valid" — the brand says so.
- [OCP](OCP.md) — discriminated unions plus
  exhaustiveness give you "open for extension, closed for
  modification" on variant sets.
- [Composition over Inheritance](CoI.md) —
  brands and discriminated unions are the structural-composition
  toolkit that replaces class hierarchies for "kinds of thing".
- [KISS](KISS.md) — encoding *every* invariant in types can violate
  KISS. Pick your battles.

## References

1. Minsky, Y. "Effective ML". Jane Street tech talk, 2010.
   <https://blog.janestreet.com/effective-ml-revisited/>
2. King, A. "Parse, don't validate". 2019.
   <https://lexi-lambda.github.io/blog/2019/11/05/parse-don-t-validate/>
3. Wlaschin, S. *Domain Modeling Made Functional*. Pragmatic
   Bookshelf, 2018.
4. Pocock, M. Total TypeScript essays and tips.
   <https://www.totaltypescript.com/>
5. Effect-TS documentation. <https://effect.website/>
6. Zod documentation. <https://zod.dev/>
7. TypeScript Handbook, "Narrowing" and "Discriminated unions".
   <https://www.typescriptlang.org/docs/handbook/2/narrowing.html>
8. TypeScript 4.9 release notes — `satisfies` operator.
   <https://devblogs.microsoft.com/typescript/announcing-typescript-4-9/>
