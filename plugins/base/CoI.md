# Composition Over Inheritance

**TL;DR**: Build behaviour by composing small, focused interfaces and
types rather than by extending a base class. Some languages have **no
class inheritance** — there the principle is enforced by the language
itself. The practical question is *how* to compose: interface
bounds, default methods, delegation, type composition, and the
wrapper-type pattern.

## Canonical sources

- *Design Patterns: Elements of Reusable Object-Oriented Software*
  (Gamma, Helm, Johnson, Vlissides, 1994): "Favor object composition
  over class inheritance."
- Allen Holub, "Why extends is evil" (2003):
  <https://www.infoworld.com/article/2073649/why-extends-is-evil.html>
- Yoshua Wuyts, "Combinatorial purity":
  <https://blog.yoshuawuyts.com/combinatorial-purity/>

## The principle

In class-based OOP languages, `class Truck extends Vehicle` makes
`Truck` reuse `Vehicle`'s code by inheriting its members. Decades
of experience showed several systemic problems:

1. **Fragile base class**: changing `Vehicle` may break every
   subclass.
2. **Banana–monkey–jungle problem**: inheriting from `Vehicle` drags
   in every transitive concern of `Vehicle` (logging, persistence,
   serialization, etc.) even when only one method is needed.
3. **Hierarchy rigidity**: a `Truck` cannot be both a `Vehicle` and
   a `Container` if both have a `weight` field — multiple inheritance
   introduces diamonds.
4. **Behaviour reuse coupled to identity reuse**: subclasses are
   "is-a" relationships; inheritance forces "Truck *is a* Vehicle"
   semantics on what was really just "Truck *has* engine code I
   wanted to reuse".

The Gang of Four prescription, repeated by every subsequent OO
authority: **prefer composition** (object holds another object) over
inheritance (class extends class). Languages without inheritance
simply remove it from the menu, leaving the principle as the only
path.

## Why it matters

Whether or not the language enforces this principle, **how** you
compose matters. Each composition idiom — interface bounds, default
methods, delegation, the wrapper-type pattern — has specific
trade-offs.

Done well, composition gives you:

- **Mix-and-match**: a type can implement any combination of
  interfaces without inheritance constraints.
- **Replaceable parts**: each composed component can be substituted
  independently.
- **Testability**: each component is a unit; mocks are scoped.
- **Explicit dependencies**: every relationship is visible in the
  type signature (no hidden inheritance).

Done badly (lots of generic parameters, deeply nested interface
constraints), the trade-off becomes burdensome verbosity. The skill
is composing **at the right grain**.

## Composition mechanisms

Most languages give you several composition mechanisms:

### 1. Interface bounds in generics

The most basic composition: a function asks for the capabilities it
needs.

```
function report(writer: Write + Send, data: Data) -> Result { /* ... */ }
```

`report` does not care whether `writer` is a file, a socket, or
an in-memory buffer. It composes the `Write` and `Send`
capabilities at the call site.

### 2. Interface composition through super-interfaces

```
public interface Animal extends Printable, Cloneable {
    name() -> string
}
```

`Animal` *requires* `Printable` and `Cloneable`. Implementors get
the composed shape; consumers can rely on it. This is composition by
contract, not inheritance — `Animal` does not inherit `Printable`'s
implementation, it just demands one exists.

### 3. Default methods

```
public interface Logger {
    log(msg: string)
}

// Extension: every Logger gets `log_with_timestamp` for free.
public interface LoggerExt extends Logger {
    log_with_timestamp(msg: string) {
        this.log("[" + now() + "] " + msg)
    }
}
// every Logger automatically gains LoggerExt
```

`log_with_timestamp` is added to every `Logger` without any
implementor writing it. This is composition by extension, mirroring
mix-ins in other languages but without the inheritance baggage.

### 4. Type composition

```
public type ConnectionPool {
    inner: Pool,
    metrics: MetricsCollector,
    retry_policy: RetryPolicy,
}
```

`ConnectionPool` *has* a `Pool`, a `MetricsCollector`, a
`RetryPolicy`. Each field is independently testable; each can be
swapped.

### 5. Delegation (use sparingly)

```
public type VerboseFile { inner: File }
// VerboseFile forwards every call to inner (the wrapped File)
```

`VerboseFile` exposes all of `File`'s methods by forwarding. Useful
when wrapping a primitive while adding behaviour; dangerous when
overused (the "smart wrapper" hides which methods are added vs
delegated).

### 6. Wrapper-type pattern

```
public type Email { value: string }
// Email.parse(raw) validates and constructs
```

`Email` *composes* a string's storage without inheriting its
methods. We cover this idiom in its own section below.

## Violations and remedies

### Anti-pattern: simulating inheritance via a "base" field

```
public interface Animal {
    name() -> string
    speak() -> string
}

public type Mammal { name: string, /* ... */ }
// Mammal implements Animal

public type Dog { mammal: Mammal, breed: Breed }
// Dog implements Animal:
//   name()  -> this.mammal.name   (delegated)
//   speak() -> "woof"
```

This *works*, but the `mammal` field is a workaround. Better:

### Idiomatic fix: just compose the data

```
public type Dog {
    name: string,
    breed: Breed,
}
// Dog implements Animal:
//   name()  -> this.name
//   speak() -> "woof"
```

If many animals share fields, factor them into a type:

```
public type Vitals { name: string, age_months: int }

public type Dog { vitals: Vitals, breed: Breed }
public type Cat { vitals: Vitals, indoor: bool }
```

Now `Vitals` is a composable component, not a parent.

### Anti-pattern: god interface hiding inheritance instinct

```
public interface Repository {
    find(id: Id) -> optional Entity
    save(e: Entity) -> Result
    delete(id: Id) -> Result
    count() -> int
    list_paginated(p: Page) -> list<Entity>
    migrate() -> Result
    dump() -> Bytes
    restore(b: Bytes) -> Result
}
```

You wanted "every repository should have all these". In a class
language you would inherit from `BaseRepository`. Here you wrote
a god interface — same anti-pattern wearing a different hat.

### Idiomatic fix: compose small interfaces (ISP)

```
public interface Find { find(id: Id) -> optional Entity }
public interface Save { save(e: Entity) -> Result }
public interface Delete { delete(id: Id) -> Result }
// etc.
```

A concrete repository implements the subset it supports. A consumer
asks for the subset it needs.

See [ISP](ISP.md) for the formal version
of this argument.

### Anti-pattern: deeply nested type composition

```
public type UserService {
    inner: InnerUserService,
}
public type InnerUserService {
    actually: ActuallyUserService,
}
public type ActuallyUserService {
    impl_: ImplUserService,
}
```

Composition turned into inheritance by other means. Each layer adds
indirection without adding capability.

### Idiomatic fix: flatten

```
public type UserService {
    repo: UserRepository,
    cache: Cache,
}
```

If the layers exist for *real reasons* (e.g. tracing, metrics, retry),
they should each be a distinct concern. Otherwise collapse.

## The wrapper-type pattern

A form of composition worth its own section. (Some languages call
this a "newtype"; the idea is the same: a distinct type that wraps a
single underlying value.)

### What it is

```
public type UserId { value: Uuid }
public type OrderId { value: Uuid }
```

`UserId` *composes* a `Uuid` but is a distinct type. `UserId` and
`OrderId` are not interchangeable, even though they wrap the same
underlying data.

### When to use it

- **Distinguishing identifiers**: prevents `deactivate(user: UserId, by: AdminId)`
  from accepting swapped arguments.
- **Encoding invariants**: a wrapper type `Email` with a private
  constructor that validates. (See
  [Make Invalid States Unrepresentable](MISU.md).)
- **Adding capabilities**: implement formatting, parsing, or custom
  arithmetic on the wrapper without polluting the underlying type.
- **Crossing module boundaries with foreign types**: when the rules
  prevent you from attaching behaviour directly to a foreign type,
  wrap it in your own type and attach the behaviour there.

### Implementing it well

```
public type UserId { value: Uuid }
// derive equality, hashing, copy, serialization

function UserId.new() -> UserId { UserId(Uuid.random()) }
function UserId.as_uuid() -> Uuid { this.value }
function UserId.from(u: Uuid) -> UserId { UserId(u) }
function UserId.to_string() -> string { this.value.to_string() }
function UserId.parse(s: string) -> Result<UserId> { Uuid.parse(s).map(UserId) }
```

When the language supports it, a code-generation helper can stamp out
the boilerplate for many identifier types at once:

```
define_id_type(UserId)
define_id_type(OrderId)
define_id_type(TransactionId)
// each expands to a wrapper type with new(), from(), and conversions
```

### Trade-offs

- **Boilerplate**: every wrapper needs conversions, formatting,
  parsing, serialization, etc. Code generation mitigates this.
- **Costless wrapping**: in many languages a single-field wrapper has
  the *same memory layout* as the value it wraps. There is no runtime
  cost.
- **API surface**: callers must write `UserId.new()` rather than
  `Uuid.random()`, which is the entire point — explicit at every
  call site.

## How code-ranker detects composition issues

The graph signals:

| Signal | Composition interpretation |
|---|---|
| Interface with many methods AND many implementations | ISP candidate; suggests breaking into composable interfaces |
| Interface with many methods AND one implementation | KISS / YAGNI candidate; inheritance instinct disguised as an interface |
| Type with one field that has the same effective type as itself | Indirection without composition — flatten |
| Multiple string-typed identifiers passed around | Wrapper-type candidates |
| A single-field wrapper type without a validating constructor | Wrapper with broken encapsulation |

Code Ranker's `god-module-coupling` and `high-fan-in-public-api` rules
indirectly capture the "fat interface" issue. A future rule could flag:

- Interfaces with > N methods AND multiple implementations → ISP
  candidate.
- Functions taking same-type identifiers without wrappers → wrapper
  candidate.

## Suggested recommendation template

> **Composition candidate**: interface `Repository` has 8 methods and
> 4 implementations. Several implementations leave half the methods
> unimplemented. Decompose into capability interfaces (`Find`,
> `Save`, `Delete`, etc.) and let each implementation declare only
> the capabilities it supports. This is "compose, don't inherit"
> at the interface level.
>
> Source: Gang of Four (1994); Wuyts, "Combinatorial purity".

## Related principles

- [ISP](ISP.md) — segregation IS the
  interface-level form of "favor composition".
- [DIP](DIP.md) — composition is what
  makes DIP cheap (no inheritance to drag in).
- [Make Invalid States Unrepresentable](MISU.md)
  — the wrapper type is the workhorse for this.
- [SRP](SRP.md) — each composed piece has
  one responsibility.

## References

1. Gamma, E., Helm, R., Johnson, R., Vlissides, J. *Design Patterns*.
   1994, p.20.
2. Holub, A. "Why extends is evil". *InfoWorld*, 2003.
   <https://www.infoworld.com/article/2073649/why-extends-is-evil.html>
3. Wuyts, Y. "Combinatorial purity".
   <https://blog.yoshuawuyts.com/combinatorial-purity/>
