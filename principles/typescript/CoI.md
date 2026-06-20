# Composition Over Inheritance (in TypeScript)

**TL;DR**: Build behaviour by composing small interfaces, plain
functions, and injected collaborators rather than by extending a base
class. TypeScript *does* have real `class extends` inheritance (it
inherits JavaScript's), but its **structural type system** makes
composition essentially free, and the vast majority of real-world
TS — especially React and modern backends — has moved decisively
towards composition. The hard questions are *when* the few legitimate
uses of `extends` apply (`Error`, framework base classes, discriminated
ADTs via class hierarchies), and how to express mixins, branded types,
and HOC-style composition without re-inventing inheritance.

## Canonical sources

- *Design Patterns: Elements of Reusable Object-Oriented Software*
  (Gamma, Helm, Johnson, Vlissides, 1994): "Favor object composition
  over class inheritance."
- Allen Holub, "Why extends is evil" (2003):
  <https://www.infoworld.com/article/2073649/why-extends-is-evil.html>
- Eric Elliott, *Composing Software* (2018) — book-length treatment
  of function/object composition in JS/TS.
  <https://leanpub.com/composingsoftware>
- Kent C. Dodds, "Inheritance vs Composition in React":
  <https://kentcdodds.com/blog/inheritance-composition-react>
- TypeScript Handbook, "Mixins":
  <https://www.typescriptlang.org/docs/handbook/mixins.html>
- TypeScript Handbook, "Classes" (on `implements` vs `extends`):
  <https://www.typescriptlang.org/docs/handbook/2/classes.html>
- React docs (legacy), "Composition vs Inheritance":
  <https://legacy.reactjs.org/docs/composition-vs-inheritance.html>

## The principle

In classical OOP, `class Truck extends Vehicle` makes `Truck` reuse
`Vehicle`'s code by inheriting its members. Decades of experience
exposed systemic problems:

1. **Fragile base class**: changing `Vehicle` may break every
   subclass — and in TS, since JavaScript prototypes are mutable at
   runtime, the failures can be subtler than in Java.
2. **Banana–monkey–jungle problem**: inheriting from `Vehicle` drags
   in every transitive concern (logging, ORM hooks, lifecycle
   methods) even when you wanted one method.
3. **Hierarchy rigidity**: TS has *no* multiple class inheritance.
   You can `implements` many interfaces but `extends` only one
   class. Once a class extends `BaseController`, it cannot also
   extend `BaseJob`.
4. **Behaviour reuse coupled to identity reuse**: `extends` declares
   "is-a"; most reuse is really "has-a" or "behaves-like-a".

The Gang of Four prescription: **prefer composition** (object holds
or is parameterised by another object) over inheritance (class
extends class). In TS the principle is *not* enforced by the
language — `extends` is right there — so applying it is a discipline,
not a default.

## Why it matters in TypeScript

TypeScript has two features that make composition unusually cheap:

- **Structural typing**: any object with the right shape satisfies an
  interface. No `implements` declaration is required to *be* assignable
  to an interface. Composition by interface has zero ceremony.
- **First-class functions and union types**: behaviour can be a
  function passed in, or a discriminated union dispatched on. You
  rarely *need* a class hierarchy to model variants.

Done well, composition gives you:

- **Mix-and-match**: an object can satisfy any combination of
  interfaces without inheritance constraints.
- **Replaceable parts**: every injected collaborator is independently
  swappable, including in tests.
- **Explicit dependencies**: relationships appear in constructor
  parameters or function arguments, not in a hidden `super` chain.
- **Tree-shakeable**: standalone functions beat class methods for
  bundlers; HOC-style composition disappears unused code that class
  inheritance keeps alive.

Done badly (mixin towers, four-deep wrapper HOCs, branded types
everywhere) the trade-off becomes verbosity and inscrutable error
messages. The skill is composing **at the right grain**.

## Mechanisms in TypeScript

### 1. `implements` multiple interfaces

```ts
interface Readable  { read(n: number): Promise<Buffer>; }
interface Closeable { close(): Promise<void>; }
interface Seekable  { seek(offset: number): Promise<void>; }

class S3Stream implements Readable, Closeable {
  async read(n: number) { /* ... */ return Buffer.alloc(n); }
  async close()         { /* ... */ }
}

class LocalFile implements Readable, Closeable, Seekable {
  async read(n: number)         { /* ... */ return Buffer.alloc(n); }
  async close()                 { /* ... */ }
  async seek(offset: number)    { /* ... */ }
}
```

Consumers ask for the minimal capability they need:

```ts
async function drain(src: Readable & Closeable) { /* ... */ }
```

This is composition by contract. Note that thanks to structural
typing, even a plain object literal with the right shape works —
`implements` is a *check*, not a *requirement*.

### 2. Function composition and higher-order functions

The most JS-native composition: behaviour is a function, and
functions combine.

```ts
type Middleware<T> = (next: (x: T) => Promise<T>) => (x: T) => Promise<T>;

const compose = <T,>(...mws: Middleware<T>[]): Middleware<T> =>
  (next) => mws.reduceRight((acc, mw) => mw(acc), next);

const withLogging:  Middleware<Req> = (next) => async (x) => { log(x); return next(x); };
const withTimeout:  Middleware<Req> = (next) => async (x) => withTimeoutMs(next(x), 5000);
const withRetry:    Middleware<Req> = (next) => async (x) => retry(() => next(x));

const handler = compose(withLogging, withTimeout, withRetry)(baseHandler);
```

No class hierarchy. Each middleware is independently testable and
composes by ordinary function application. Eric Elliott's
*Composing Software* is essentially a 200-page argument that this
should be your default.

### 3. Constructor injection (services)

```ts
class OrderService {
  constructor(
    private readonly repo:   OrderRepository,
    private readonly mailer: Mailer,
    private readonly clock:  Clock,
  ) {}

  async place(o: Order) {
    await this.repo.save(o);
    await this.mailer.send(o.customerEmail, "confirmation", { o });
  }
}
```

`OrderService` *has* a repository, a mailer, and a clock. Each
collaborator is an interface; each is independently mockable. This
is the backend dual of React's HOC pattern: behaviour reuse without
hierarchy.

The anti-pattern this replaces is:

```ts
class OrderService extends BaseService { /* inherits repo, mailer, logger... */ }
class BaseService  extends BaseBaseService { /* inherits config, db... */ }
```

NestJS encourages constructor injection, and that is the part of
NestJS to embrace. NestJS also encourages `extends BaseController`
in some examples — that part is suspect (see below).

### 4. Discriminated unions instead of class hierarchies

```ts
type Shape =
  | { kind: "circle";    radius: number }
  | { kind: "rectangle"; w: number; h: number }
  | { kind: "triangle";  base: number; height: number };

function area(s: Shape): number {
  switch (s.kind) {
    case "circle":    return Math.PI * s.radius ** 2;
    case "rectangle": return s.w * s.h;
    case "triangle":  return 0.5 * s.base * s.height;
  }
}
```

In a class-OOP language you would write `abstract class Shape` with
subclasses. In TS, a discriminated union plus exhaustive `switch` is
nearly always better: no method dispatch, exhaustiveness checked by
the compiler, behaviour added externally without modifying the
"hierarchy". (Open/Closed: see
[OCP](OCP.md) for the trade-off.)

### 5. Mixins (the TS pattern)

When you genuinely want to bolt behaviour onto a class, the TS-blessed
mixin pattern is a function from constructor to constructor:

```ts
type Constructor<T = {}> = new (...args: any[]) => T;

function Timestamped<TBase extends Constructor>(Base: TBase) {
  return class extends Base {
    createdAt = new Date();
    touchedAt = new Date();
    touch() { this.touchedAt = new Date(); }
  };
}

function Tagged<TBase extends Constructor>(Base: TBase) {
  return class extends Base {
    tags: Set<string> = new Set();
    addTag(t: string) { this.tags.add(t); }
  };
}

class User { constructor(public name: string) {} }

class TaggedTimestampedUser extends Tagged(Timestamped(User)) {}
```

**Discuss carefully**: mixins look elegant in a slide deck, but they
have real costs in TS.

- **Type inference is fragile**: deep mixin stacks frequently break
  in subtle ways; the inferred type of `Tagged(Timestamped(User))`
  can degrade to `any` if any layer is mis-typed.
- **`this` typing is a known sore spot**: mixin methods that reference
  `this` often need explicit `this: This` parameters.
- **Decorator alternative**: TC39 decorators (stage 3, supported in
  TS 5.0+) cover many former mixin use-cases more cleanly.
- **Composition usually wins**: instead of `class extends Tagged(...)`,
  hold a `Tags` field and a `Timestamps` field. It is 10% more
  typing and 90% less mystery.

Rule of thumb: reach for the mixin pattern only when you must extend
a **framework-provided base class** and need to layer two cross-cutting
concerns on top. Otherwise, compose with fields.

### 6. Branded (nominal) types — the TS newtype

TypeScript is structural, which is great for composition but bad
when you want `UserId` and `OrderId` (both `string`) to be
non-interchangeable. Brands recover nominal typing:

```ts
declare const brand: unique symbol;
type Brand<T, B> = T & { readonly [brand]: B };

export type UserId  = Brand<string, "UserId">;
export type OrderId = Brand<string, "OrderId">;

export const UserId = {
  parse(raw: string): UserId {
    if (!/^u_[a-z0-9]{16}$/.test(raw)) throw new Error("bad UserId");
    return raw as UserId;
  },
  fresh(): UserId { return ("u_" + randomId(16)) as UserId; },
};
```

Now `deactivate(user: UserId, by: AdminId)` cannot be called with
swapped arguments. The brand exists only at compile time — there is
zero runtime cost.

When to use brands:

- **Distinguishing identifiers**: the swapped-argument bug.
- **Encoding invariants**: `Email`, `NonEmptyString`, `Sanitized<string>`.
  Constructed via a smart constructor that validates. (See
  [Make Invalid States Unrepresentable](MISU.md).)
- **Marking provenance**: `Trusted<string>` vs `UserInput<string>`.

Trade-offs:

- **Cast at the boundary**: somewhere a `string` becomes `UserId`.
  Concentrate this in one parser, never sprinkled around.
- **Serde**: brands disappear through `JSON.parse`. Validate on
  ingress (zod, valibot) and brand the parsed result.
- **Slight ergonomic cost**: type errors mentioning `Brand<...>`
  are uglier. Name the brand carefully (`"UserId"`, not `"u"`).

## Violations and remedies

### Anti-pattern: deep class hierarchies for shared fields

```ts
abstract class Animal {
  constructor(public name: string, public ageMonths: number) {}
  abstract speak(): string;
}
class Mammal extends Animal {}
class Dog    extends Mammal { speak() { return "woof"; } }
class Cat    extends Mammal { speak() { return "meow"; } }
```

The hierarchy exists only to share two fields. Easier:

### Idiomatic fix: compose the shared shape

```ts
interface Vitals { name: string; ageMonths: number; }
interface Animal extends Vitals { speak(): string; }

const dog: Animal = { name: "Rex", ageMonths: 24, speak: () => "woof" };
const cat: Animal = { name: "Mia", ageMonths: 18, speak: () => "meow" };
```

Or, if you must have classes, hold `vitals` as a field:

```ts
class Dog { constructor(public vitals: Vitals, public breed: string) {} speak() { return "woof"; } }
```

### Anti-pattern: god interface

```ts
interface Repository<E> {
  find(id: string):   Promise<E | null>;
  save(e: E):         Promise<void>;
  delete(id: string): Promise<void>;
  count():            Promise<number>;
  listPaginated(p: Page): Promise<E[]>;
  migrate():          Promise<void>;
  dump():             Promise<Buffer>;
  restore(b: Buffer): Promise<void>;
}
```

The "I want every repo to have everything" instinct that produces
`extends BaseRepository` in Java becomes a god interface in TS.
Same anti-pattern, different hat.

### Idiomatic fix: small interfaces, compose with `&`

```ts
interface Find<E>   { find(id: string): Promise<E | null>; }
interface Save<E>   { save(e: E): Promise<void>; }
interface Delete    { delete(id: string): Promise<void>; }
interface Migrate   { migrate(): Promise<void>; }

type CrudRepo<E> = Find<E> & Save<E> & Delete;
```

A consumer asks for exactly the capability it needs. See
[ISP](ISP.md) for the formal version.

### Anti-pattern: React class component inheritance

```tsx
class BaseForm<P, S> extends React.Component<P, S> {
  validate() { /* ... */ }
  submit()   { /* ... */ }
}
class LoginForm extends BaseForm<LoginProps, LoginState> { /* ... */ }
class SignupForm extends BaseForm<SignupProps, SignupState> { /* ... */ }
```

React class components are effectively deprecated for new code, and
even when they were idiomatic the official guidance was *do not
inherit*. (See Dodds, "Inheritance vs Composition in React".)

### Idiomatic fix: hooks + components as props

```tsx
function useFormState<T>(initial: T, validate: (t: T) => Errors) { /* ... */ }

function LoginForm() {
  const { values, errors, submit } = useFormState(initialLogin, validateLogin);
  return <Form values={values} errors={errors} onSubmit={submit} />;
}
```

Hooks are composition incarnate: each hook is a unit of behaviour
combined by calling. HOCs (`withAuth(Component)`) are the same idea
for whole components, though hooks have largely supplanted them.

### Anti-pattern: framework base class doing too much

```ts
@Controller("orders")
class OrdersController extends BaseAuthenticatedController {
  // inherits auth, logging, rate-limiting, audit, ...
}
```

NestJS, Express middleware classes, and similar frameworks invite
`extends Base`. Sometimes acceptable (e.g. a thin
`extends Controller` for routing metadata), often a tarpit (the base
class accumulates every cross-cutting concern).

### Idiomatic fix: decorators, guards, interceptors as composition

```ts
@Controller("orders")
@UseGuards(AuthGuard)
@UseInterceptors(AuditInterceptor, LoggingInterceptor)
class OrdersController {
  constructor(private readonly orders: OrderService) {}
}
```

Each cross-cutting concern is a separate, composable artefact applied
declaratively. Same effect as the base-class version, without the
single-inheritance straitjacket.

## When `extends` is actually fine

A short list of legitimate inheritance in TS:

- **`class extends Error`**: the `instanceof Error` check is built
  into the runtime; subclassing `Error` is the standard way to make a
  typed error. (Remember `Object.setPrototypeOf(this, new.target.prototype)`
  on older targets.)
- **Framework-mandated base classes**: `extends React.Component` in
  legacy code, `extends EventEmitter`, NestJS's
  `extends PassportStrategy(Strategy, "jwt")` — when the framework's
  contract is "subclass me", subclass.
- **One-level abstract base with no logic, just signature**: rare,
  but legitimate when you want classes (for `instanceof`) and the
  base genuinely has nothing to give but shape. Discriminated unions
  usually beat this.
- **ADTs implemented as sealed class hierarchies**: rare in TS
  (unions are better), occasionally appears in DDD codebases.

Note the pattern: each case is a *single, shallow* inheritance step
imposed by the runtime or a framework, not a tower of subclasses.

## How code-ranker detects composition issues

Graph signals:

| Signal | Composition interpretation |
|---|---|
| Class hierarchy more than 2 levels deep | Almost always a refactor candidate |
| Class with many `protected` members | Inheritance leakage; consider composition |
| Interface with many methods AND many implementors | ISP candidate; decompose |
| Interface with many methods AND one implementor | KISS / YAGNI candidate |
| Many functions taking `string` IDs of different semantic kinds | Brand candidates |
| `class X extends Y` where `Y` has only one subclass | Collapse |
| HOC nested more than 2 deep | Likely candidate for hook extraction |

Future Code Ranker rules could specifically flag:

- Inheritance chains > 2 levels.
- Mixin towers (`A(B(C(D)))`) without a stabilising explicit type.
- `string`-typed ID parameters across module boundaries (brand
  candidates).

## Suggested recommendation template

> **Composition candidate**: `class OrderProcessor extends
> AuditedService extends BaseService` is a 3-level hierarchy where
> each level adds one cross-cutting concern. In TypeScript these
> are better expressed as injected collaborators (constructor
> parameters) or as decorators/interceptors at the framework level.
> The hierarchy forbids `OrderProcessor` from also extending any
> other base class, and makes the `AuditedService` contract implicit.
>
> Source: Gang of Four (1994); Elliott, *Composing Software*; Dodds,
> "Inheritance vs Composition in React".

## Related principles

- [ISP](ISP.md) — segregating interfaces is
  the TS-flavoured "favor composition" for contracts.
- [DIP](DIP.md) — constructor injection is
  composition in service of DIP.
- [LSP](LSP.md) — the principle that makes
  most uses of `extends` go wrong.
- [OCP](OCP.md) — discriminated unions vs class
  hierarchies is an OCP trade-off.
- [Make Invalid States Unrepresentable](MISU.md)
  — brands are the TS workhorse for this.
- [SRP](SRP.md) — each composed unit has
  one responsibility.
- [Law of Demeter](LoD.md) — composition makes the
  reach of each collaborator explicit.
- [KISS](KISS.md) / [YAGNI](YAGNI.md) — most class hierarchies fail
  these on day one.

## References

1. Gamma, E., Helm, R., Johnson, R., Vlissides, J. *Design Patterns*.
   1994, p.20.
2. Holub, A. "Why extends is evil". *InfoWorld*, 2003.
   <https://www.infoworld.com/article/2073649/why-extends-is-evil.html>
3. Elliott, E. *Composing Software*. Leanpub, 2018.
   <https://leanpub.com/composingsoftware>
4. Dodds, K. C. "Inheritance vs Composition in React".
   <https://kentcdodds.com/blog/inheritance-composition-react>
5. TypeScript Handbook, "Mixins".
   <https://www.typescriptlang.org/docs/handbook/mixins.html>
6. TypeScript Handbook, "Classes".
   <https://www.typescriptlang.org/docs/handbook/2/classes.html>
7. React docs (legacy), "Composition vs Inheritance".
   <https://legacy.reactjs.org/docs/composition-vs-inheritance.html>
