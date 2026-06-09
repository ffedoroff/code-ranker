# LSP — Liskov Substitution Principle (in TypeScript)

**TL;DR**: A value of type `S` should be usable everywhere a value of
type `T` is expected, without surprises, when `S` is assignable to `T`.
TypeScript's **structural subtyping** is the LSP enforcer: if the shape
matches, the substitution is allowed by the compiler — whether or not
the *behaviour* matches is on you. Violations show up as runtime
astonishment, broken consumers, or silent assignability that hides a
contract mismatch. Target: TS 5.4+ with `strict` enabled.

## Canonical sources

- Barbara Liskov, "Data Abstraction and Hierarchy" (1988 SIGPLAN
  keynote) and Liskov & Wing, "A Behavioral Notion of Subtyping"
  (ACM TOPLAS 16(6), 1994):
  <https://dl.acm.org/doi/10.1145/197320.197383>
- Robert C. Martin, "The Liskov Substitution Principle" (1996):
  <https://www.labri.fr/perso/clement/enseignements/ao/LSP.pdf>
- Martin, *Clean Architecture*, Ch. 9.
- TypeScript Handbook, "Type Compatibility" and "Variance":
  <https://www.typescriptlang.org/docs/handbook/type-compatibility.html>
- TypeScript release notes, `--strictFunctionTypes` (2.6) and
  `override` keyword (4.3).

## The principle

In Liskov's words: if `S` is a subtype of `T`, then objects of type
`T` may be replaced with objects of type `S` without altering any of
the desirable properties of the program.

The crucial word is **desirable** — Liskov is not asking that the
substitute be *identical*, only that it respect the **behavioural
contract** that consumers depend on. Two implementations of
`Iterable<T>` may use entirely different data, but both must:

- Return `{ done: true }` once and stay done.
- Not throw except in documented circumstances.
- Produce values whose types are actually `T`.

Each of those is part of the iterator contract, even though only the
last is type-checked. An implementation that violates the others is
**technically valid TypeScript** but semantically an LSP violation:
it compiles but breaks consumers that relied on the contract.

LSP is enforced by **discipline, documentation, and tests** — the
compiler proves *shapes match*; LSP demands that *behaviours match*.

## Why it matters in TypeScript

TypeScript is structurally typed, so subtyping is *opportunistic*:
any object with the right shape is assignable, whether or not the
author intended to implement your interface. This makes LSP both
easier (no `extends`/`implements` ceremony for assignability) and
harder (no central place to write the contract, no `final` keyword,
no `sealed` interfaces in the runtime sense).

What TypeScript's type system *does* check:

- Method parameter and return types are compatible
  (with variance rules — see below).
- Property types match structurally.
- `readonly` is preserved on the read side.

What it *cannot* check:

- That an `IUserRepo.findById` actually returns `null` for missing
  users rather than throwing, as the JSDoc claims.
- That a `Stream` honours back-pressure rather than buffering
  unboundedly.
- That a subclass `Square` extending `Rectangle` still satisfies
  callers who set `width` and `height` independently.
- That a `toString()` override returns a parseable format the parent
  documented.

Each of these compiles, ships, and erodes the assumption that "any
implementation is interchangeable".

## Plain JavaScript without types

If you remove the types, **LSP is enforced by your test suite alone**.
No compiler will tell you that a subclass widened the thrown errors,
that an override returns `undefined` where the parent returned a
string, or that a duck-typed object is missing a property a consumer
depends on. The mitigations in this document — `override` keyword,
`readonly` arrays, `strictFunctionTypes` — all evaporate. You need:

1. Contract tests that exercise every implementation through the
   shared interface.
2. JSDoc `@implements` and `@throws` annotations as documentation,
   not enforcement.
3. Discipline about not adding new exceptions, return types, or side
   effects in overrides.

This document assumes TypeScript with `strict: true`.

## In TypeScript

### Structural subtyping is the LSP enforcer

```ts
interface Logger {
  log(msg: string): void;
}

// No `implements Logger` — but assignable, because shape matches.
const consoleLogger = {
  log: (m: string) => console.log(m),
  extraField: 42,
};

const logger: Logger = consoleLogger; // OK
```

That's LSP at the type level: `consoleLogger` is a subtype of
`Logger` because it has *at least* what `Logger` requires. The
compiler verifies the shape. Whether `log` actually writes somewhere
useful is a behavioural contract no type system enforces.

### Variance: covariance and contravariance

Under `strictFunctionTypes` (on by default in `strict`), function
**parameters are contravariant** and **return types are covariant**:

```ts
type AnimalHandler = (a: Animal) => Animal;
type DogHandler    = (d: Dog) => Dog;

declare const dh: DogHandler;
const ah: AnimalHandler = dh; // ERROR: Dog parameter is too narrow
```

A function that *only* knows how to handle `Dog` cannot substitute
for a function that must handle any `Animal`. This is LSP at the
function level: a subtype function must accept *at least* what the
supertype accepts (contravariant params) and return *at most* what
the supertype returns (covariant return).

### The method-parameter bivariance gotcha

Inside a *method* declared with method shorthand (not a property
arrow), TypeScript uses **bivariance** even under
`strictFunctionTypes`. This is a deliberate compromise for
`Array<T>` and event-handler ergonomics, but it silently lets LSP
violations through:

```ts
interface AnimalShelter {
  accept(a: Animal): void; // method shorthand — bivariant param
}

class DogOnlyShelter implements AnimalShelter {
  accept(d: Dog): void {  // accepts only Dog — narrower than parent
    d.bark();             // crashes if a Cat is passed
  }
}

const s: AnimalShelter = new DogOnlyShelter();
s.accept(new Cat()); // compiles; throws at runtime
```

Mitigation: declare the method as a property with an explicit
function type, which is checked strictly:

```ts
interface AnimalShelter {
  accept: (a: Animal) => void; // property — strictly contravariant
}
```

### `override` keyword (TS 4.3+)

Enable `noImplicitOverride` and use `override` on every subclass
method that overrides a parent. Typo'd overrides — `tostring` instead
of `toString` — silently become new methods otherwise.

```ts
class Base {
  greet(): string { return "hi"; }
}

class Sub extends Base {
  override greet(): string { return "hello"; } // OK
  override greett(): string { return "oops"; } // ERROR: no parent
}
```

### Mutable arrays are invariant; `ReadonlyArray` is covariant

```ts
const dogs: Dog[] = [new Dog()];
const animals: Animal[] = dogs; // ERROR under strict

const ro: ReadonlyArray<Animal> = dogs; // OK
```

`Dog[]` would let a consumer push a `Cat` and corrupt the underlying
array of dogs. `ReadonlyArray` removes the write side and recovers
covariance — a textbook Liskov-safe widening.

## Violations and remedies

### Anti-pattern: Square extends Rectangle

```ts
class Rectangle {
  constructor(public width: number, public height: number) {}
  setWidth(w: number)  { this.width = w; }
  setHeight(h: number) { this.height = h; }
  area() { return this.width * this.height; }
}

class Square extends Rectangle {
  override setWidth(w: number)  { this.width = w; this.height = w; }
  override setHeight(h: number) { this.width = h; this.height = h; }
}

function grow(r: Rectangle) {
  r.setWidth(5);
  r.setHeight(4);
  console.assert(r.area() === 20); // fails for Square
}
```

`Square` *is-a* `Rectangle` geometrically but not behaviourally.
Consumers that assume independent width/height break.

### Idiomatic fix: separate types, share a `Shape` interface

```ts
interface Shape { area(): number; }

class Rectangle implements Shape {
  constructor(public width: number, public height: number) {}
  area() { return this.width * this.height; }
}

class Square implements Shape {
  constructor(public side: number) {}
  area() { return this.side * this.side; }
}
```

No inheritance, no contract violation; both satisfy `Shape`.

### Anti-pattern: widening thrown errors in an override

```ts
class Repo {
  /** @throws NotFoundError if missing */
  findById(id: string): User { /* ... */ }
}

class S3Repo extends Repo {
  override findById(id: string): User {
    if (Math.random() < 0.01) throw new NetworkError(); // new!
    // ...
  }
}
```

Callers wrote `try { repo.findById(id) } catch (e) { if (e instanceof
NotFoundError) ... }` and silently let `NetworkError` escape. TypeScript
has no checked exceptions, so the compiler is no help.

### Idiomatic fix: encode errors in the return type

```ts
type RepoResult<T> =
  | { ok: true;  value: T }
  | { ok: false; error: "not_found" | "network" };

interface Repo {
  findById(id: string): Promise<RepoResult<User>>;
}
```

Every implementation enumerates its error variants in the type.
Adding `"network"` is a type-level breaking change consumers cannot
miss.

### Anti-pattern: interface extension that changes return type

```ts
interface Producer { next(): { value: number } }

interface BatchedProducer extends Producer {
  next(): { value: number; batchId: string }; // OK — covariant
}
```

This is *allowed* (covariant return) but a Liskov hazard if some
consumer was destructuring `{ value }` and another was checking
`Object.keys(...).length === 1`. The new field can collide with a
caller's spread, break JSON shape assertions, or surprise downstream
schema validators.

### Idiomatic fix: introduce a new method or a new interface

If batching is genuinely a new capability, don't widen `next` —
add `nextBatched()` or define `BatchedProducer` as a separate
interface that does not extend `Producer`. Consumers opt in to the
wider contract.

### Anti-pattern: declaration merging that widens

```ts
// lib.ts
interface Options { timeout: number }

// elsewhere.ts — merges
interface Options { retries: number }
```

A function declared `(o: Options) => void` in `lib.ts` now requires
`retries` too, breaking every existing caller. Declaration merging
silently changes the contract of an existing interface across the
whole program.

### Idiomatic fix: distinct interfaces, explicit composition

```ts
interface BaseOptions { timeout: number }
interface RetryOptions extends BaseOptions { retries: number }
```

Functions opt into the wider type explicitly.

### Anti-pattern: React lifecycle override missing `override`

```ts
class MyComp extends React.Component<Props, State> {
  componentDidMount() { /* ... */ }   // typo'd? no error
  componentdidupdate() { /* ... */ } // silent: never called
}
```

Without `noImplicitOverride`, the typo'd lifecycle method is a fresh
method on the subclass, never invoked by React. The parent contract
("this method is called after every update") is violated.

### Idiomatic fix: `noImplicitOverride` + `override`

```ts
class MyComp extends React.Component<Props, State> {
  override componentDidMount() { /* ... */ }
  override componentDidUpdate(prev: Props) { /* ... */ }
}
```

The compiler now refuses to compile typo'd lifecycle methods.

### Anti-pattern: callback parameter bivariance

```ts
interface EventBus {
  on(event: string, cb: (e: Event) => void): void;
}

class MyBus implements EventBus {
  on(event: string, cb: (e: ClickEvent) => void) { /* ... */ } // narrower!
}
```

Method-parameter bivariance lets this compile. A caller registering a
generic `Event` handler will receive `KeyEvent`s the handler cannot
process.

### Idiomatic fix: property form for strict checking

```ts
interface EventBus {
  on: (event: string, cb: (e: Event) => void) => void;
}
```

Or use a generic event-map pattern so handler types are tied to event
names rather than inherited.

### Anti-pattern: `Hash`/`Eq` equivalents — equals/hashCode drift

```ts
class User {
  constructor(public id: string, public lastSeen: Date) {}
  equals(o: User) { return this.id === o.id; }
  hashCode() { return hash(this.id) ^ hash(this.lastSeen.getTime()); }
}
```

Two `User`s with the same `id` are `equals` but hash differently —
breaks any `Map`-like structure keyed by hash. Direct LSP violation
against the documented contract "equal values hash equally".

### Idiomatic fix: derive both from the same field set

```ts
class User {
  constructor(public id: string, public lastSeen: Date) {}
  equals(o: User) { return this.id === o.id; }
  hashCode() { return hash(this.id); } // same fields as equals
}
```

Or use a value-object library that derives both from a declared key
set.

## LSP across packages

When a downstream package depends on your interface, you ship the
behavioural contract too. Versioned contract changes are breaking
even when types match. Tightening or loosening the contract of
`Repo.findById` makes previously conformant implementations
non-conformant.

The mitigation: state the contract in the interface's TSDoc, version
it, and treat contract changes as semver-major events even when no
type changes.

## How code-ranker detects LSP risk

LSP violations are usually invisible to a graph analyzer — they live
in implementation bodies and runtime behaviour. But code-ranker can
flag *structural risk*:

| Signal | LSP interpretation |
|---|---|
| Interface with N implementations and short TSDoc (no contract section) | Implementors have no shared contract; each impl will diverge. |
| Subclass methods without `override` while `noImplicitOverride` is off | Typo'd overrides hide silently. Recommend enabling the flag. |
| Method-shorthand signatures on interfaces with multiple implementors | Bivariance allows LSP-violating narrowing. Recommend property form. |
| Class extending a domain entity (`class Square extends Rectangle`) | Classic Square/Rectangle smell — flag for review. |
| `equals` and `hashCode` over non-overlapping field sets | Direct consistency violation. |

The honest answer is that LSP is mostly a documentation discipline —
code-ranker's contribution is to *flag interfaces that have no
contract section* and to *recommend writing one*, not to verify
behaviour.

## Suggested recommendation template

> **LSP risk**: interface `UserRepo` has 4 implementations across the
> workspace and no `@contract` section in its TSDoc. Without a stated
> behavioural contract, implementations diverge silently and
> consumers special-case around them. Add a `@contract` block
> documenting required invariants for every method (return semantics,
> error variants, idempotency, concurrency), then export a
> `assertUserRepoContract(repo: UserRepo)` helper that downstream
> implementations can call from their tests.
>
> References:
>  - <https://www.typescriptlang.org/docs/handbook/type-compatibility.html>
>  - <https://www.labri.fr/perso/clement/enseignements/ao/LSP.pdf>

## Related principles

- [SRP](solid-single-responsibility.md) — narrow interfaces are
  easier to write contracts for than broad ones.
- [ISP](solid-interface-segregation.md) — clients depend on small
  contracts, not large ones; LSP gets easier with each split.
- [OCP](solid-open-closed.md) — open-for-extension only works if
  extensions are Liskov-substitutable.
- [DIP](solid-dependency-inversion.md) — depending on abstractions
  is only safe when those abstractions have honest contracts.
- [Make Invalid States Unrepresentable](make-invalid-states-unrepresentable.md)
  — encode contract requirements in types where possible (e.g. a
  `NonEmptyArray<T>` type instead of "must not be empty" in TSDoc).

## References

1. Liskov, B. and Wing, J. "A Behavioral Notion of Subtyping". ACM
   TOPLAS 16(6), 1994.
   <https://dl.acm.org/doi/10.1145/197320.197383>
2. Martin, R. C. "The Liskov Substitution Principle". 1996.
   <https://www.labri.fr/perso/clement/enseignements/ao/LSP.pdf>
3. Martin, R. C. *Clean Architecture*. Ch. 9.
4. TypeScript Handbook, "Type Compatibility".
   <https://www.typescriptlang.org/docs/handbook/type-compatibility.html>
5. TypeScript 2.6 release notes, `--strictFunctionTypes`.
   <https://www.typescriptlang.org/docs/handbook/release-notes/typescript-2-6.html>
6. TypeScript 4.3 release notes, `override` and `noImplicitOverride`.
   <https://www.typescriptlang.org/docs/handbook/release-notes/typescript-4-3.html>
7. TypeScript Handbook, `ReadonlyArray`.
   <https://www.typescriptlang.org/docs/handbook/2/objects.html#the-readonlyarray-type>
