# Composition Over Inheritance (in Rust)

**TL;DR**: Build behaviour by composing small, focused traits and
structs rather than by extending a base class. Rust has **no class
inheritance** — the principle is enforced by the language. The
practical question is *how* to compose: trait bounds, blanket impls,
delegation, type composition, and the newtype pattern.

## Canonical sources

- *Design Patterns: Elements of Reusable Object-Oriented Software*
  (Gamma, Helm, Johnson, Vlissides, 1994): "Favor object composition
  over class inheritance."
- Allen Holub, "Why extends is evil" (2003):
  <https://www.infoworld.com/article/2073649/why-extends-is-evil.html>
- Yoshua Wuyts, "Combinatorial purity":
  <https://blog.yoshuawuyts.com/combinatorial-purity/>
- niko matsakis, "Object types in trait bounds":
  <https://smallcultfollowing.com/babysteps/blog/2019/10/26/async-fn-in-traits-are-hard/>
- Rust API Guidelines, "Flexibility":
  <https://rust-lang.github.io/api-guidelines/flexibility.html>
- *The Rust Programming Language* book, Ch. 17.1 — explicitly
  recommends composition.
  <https://doc.rust-lang.org/book/ch17-01-what-is-oo.html>

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
inheritance (class extends class). Rust simply removes inheritance
from the menu, leaving the principle as the only path.

## Why it matters

The Rust toolchain implicitly enforces this principle, but **how**
you compose matters. The Rust idioms — trait bounds, blanket impls,
delegation via `Deref`, the newtype pattern — each have specific
trade-offs.

Done well, composition gives you:

- **Mix-and-match**: a struct can implement any combination of
  traits without inheritance constraints.
- **Replaceable parts**: each composed component can be substituted
  independently.
- **Testability**: each component is a unit; mocks are scoped.
- **Explicit dependencies**: every relationship is visible in the
  type signature (no hidden inheritance).

Done badly (lots of generic parameters, deep trait bounds), the
trade-off becomes burdensome verbosity. The skill is composing
**at the right grain**.

## In Rust

Rust gives you several composition mechanisms:

### 1. Trait bounds in generics

The most basic composition: a function asks for the capabilities it
needs.

```rust
fn report<W: Write + Send>(writer: W, data: Data) -> io::Result<()> { /* ... */ }
```

`report` does not care whether `writer` is `File`, `TcpStream`, or
`Cursor<Vec<u8>>`. It composes `Write` and `Send` capabilities at
the call site.

### 2. Trait composition through supertraits

```rust
pub trait Animal: Debug + Clone {
    fn name(&self) -> &str;
}
```

`Animal` *requires* `Debug` and `Clone`. Implementors get the
composed shape; consumers can rely on it. This is composition by
contract, not inheritance — `Animal` does not inherit `Debug`'s
implementation, it just demands one exists.

### 3. Blanket implementations

```rust
pub trait Logger {
    fn log(&self, msg: &str);
}

// Blanket impl: every Logger gets `log_with_timestamp` for free.
pub trait LoggerExt: Logger {
    fn log_with_timestamp(&self, msg: &str) {
        self.log(&format!("[{}] {}", chrono::Utc::now(), msg));
    }
}
impl<T: Logger> LoggerExt for T {}
```

`log_with_timestamp` is added to every `Logger` without any
implementor writing it. This is composition by extension, mirroring
mix-ins in other languages but without the inheritance baggage.

### 4. Struct composition

```rust
pub struct ConnectionPool {
    inner: Pool,
    metrics: Arc<MetricsCollector>,
    retry_policy: RetryPolicy,
}
```

`ConnectionPool` *has* a `Pool`, a `MetricsCollector`, a
`RetryPolicy`. Each field is independently testable; each can be
swapped.

### 5. Delegation via `Deref` (use sparingly)

```rust
pub struct VerboseFile { inner: File }
impl Deref for VerboseFile { type Target = File; fn deref(&self) -> &File { &self.inner } }
```

`VerboseFile` exposes all of `File`'s methods. Useful when wrapping
a primitive while adding behaviour; dangerous when overused (the
"smart wrapper" hides which methods are added vs delegated).

### 6. Newtype pattern

```rust
pub struct Email(String);
impl Email {
    pub fn parse(raw: &str) -> Result<Self, _> { /* ... */ }
}
```

`Email` *composes* `String`'s storage without inheriting its
methods. This is a Rust-specific composition idiom we cover in
its own section below.

## Violations and remedies

### Anti-pattern (impossible in Rust, but worth noting): trying to simulate inheritance

```rust
pub trait Animal {
    fn name(&self) -> &str;
    fn speak(&self) -> String;
}

pub struct Mammal { name: String, /* ... */ }
impl Animal for Mammal { /* ... */ }

pub struct Dog { mammal: Mammal, breed: Breed }
impl Animal for Dog {
    fn name(&self) -> &str { &self.mammal.name }       // delegated
    fn speak(&self) -> String { "woof".into() }
}
```

This *works*, but the `mammal` field is a workaround. Better:

### Idiomatic fix: just compose the data

```rust
pub struct Dog {
    name: String,
    breed: Breed,
}
impl Animal for Dog {
    fn name(&self) -> &str { &self.name }
    fn speak(&self) -> String { "woof".into() }
}
```

If many animals share fields, factor them into a struct:

```rust
pub struct Vitals { name: String, age_months: u32 }

pub struct Dog { vitals: Vitals, breed: Breed }
pub struct Cat { vitals: Vitals, indoor: bool }
```

Now `Vitals` is a composable component, not a parent.

### Anti-pattern: god trait hiding inheritance instinct

```rust
pub trait Repository {
    fn find(&self, id: Id) -> Option<Entity>;
    fn save(&self, e: &Entity) -> Result<()>;
    fn delete(&self, id: Id) -> Result<()>;
    fn count(&self) -> u64;
    fn list_paginated(&self, p: Page) -> Vec<Entity>;
    fn migrate(&self) -> Result<()>;
    fn dump(&self) -> Bytes;
    fn restore(&self, b: Bytes) -> Result<()>;
}
```

You wanted "every repository should have all these". In a class
language you would inherit from `BaseRepository`. In Rust you wrote
a god trait — same anti-pattern wearing a different hat.

### Idiomatic fix: compose small traits (ISP)

```rust
pub trait Find { fn find(&self, id: Id) -> Option<Entity>; }
pub trait Save { fn save(&self, e: &Entity) -> Result<()>; }
pub trait Delete { fn delete(&self, id: Id) -> Result<()>; }
// etc.
```

A concrete repository implements the subset it supports. A consumer
asks for the subset it needs.

See [ISP](ISP.md) for the formal version
of this argument.

### Anti-pattern: deeply nested struct composition

```rust
pub struct UserService {
    inner: InnerUserService,
}
pub struct InnerUserService {
    actually: ActuallyUserService,
}
pub struct ActuallyUserService {
    impl_: ImplUserService,
}
```

Composition turned into inheritance by other means. Each layer adds
indirection without adding capability.

### Idiomatic fix: flatten

```rust
pub struct UserService {
    repo: UserRepository,
    cache: Cache,
}
```

If the layers exist for *real reasons* (e.g. tracing, metrics, retry),
they should each be a distinct concern. Otherwise collapse.

## The newtype pattern

A Rust-specific form of composition worth its own section.

### What it is

```rust
pub struct UserId(Uuid);
pub struct OrderId(Uuid);
```

`UserId` *composes* a `Uuid` but is a distinct type. `UserId` and
`OrderId` are not interchangeable, even though they wrap the same
underlying data.

### When to use it

- **Distinguishing identifiers**: prevents `fn deactivate(user: UserId, by: AdminId)`
  from accepting swapped arguments.
- **Encoding invariants**: `pub struct Email(String)` with a private
  constructor that validates. (See
  [Make Invalid States Unrepresentable](MISU.md).)
- **Adding capabilities**: implement `Display`, `FromStr`, custom
  arithmetic on the newtype without polluting the underlying type.
- **Crossing crate boundaries with foreign types**: the orphan rule
  prevents you from implementing `serde::Serialize` for `Vec<u8>`,
  but you can implement it for `MyBuffer(Vec<u8>)`.

### Implementing it well

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct UserId(Uuid);

impl UserId {
    pub fn new() -> Self { Self(Uuid::new_v4()) }
    pub fn as_uuid(&self) -> Uuid { self.0 }
}

impl From<Uuid> for UserId { fn from(u: Uuid) -> Self { Self(u) } }

impl fmt::Display for UserId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl FromStr for UserId {
    type Err = uuid::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> { Uuid::parse_str(s).map(Self) }
}
```

A macro often helps:

```rust
macro_rules! id_newtype {
    ($name:ident) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
        pub struct $name(Uuid);
        impl $name { pub fn new() -> Self { Self(Uuid::new_v4()) } }
        impl From<Uuid> for $name { fn from(u: Uuid) -> Self { Self(u) } }
        // ... etc
    };
}

id_newtype!(UserId);
id_newtype!(OrderId);
id_newtype!(TransactionId);
```

### Trade-offs

- **Boilerplate**: every newtype needs `From`, `Display`, `FromStr`,
  serde derives, etc. The macro mitigates this.
- **Costless wrapping**: a `pub struct UserId(Uuid)` has the *same
  memory layout* as `Uuid`. There is no runtime cost.
- **API surface**: callers must write `UserId::new()` rather than
  `Uuid::new_v4()`, which is the entire point — explicit at every
  call site.

## How code-ranker detects composition issues

The graph signals:

| Signal | Composition interpretation |
|---|---|
| Trait with many methods AND many implementations | ISP candidate; suggests breaking into composable traits |
| Trait with many methods AND one implementation | KISS / YAGNI candidate; inheritance instinct disguised as a trait |
| Struct with one field that has the same effective type as itself | Indirection without composition — flatten |
| Multiple `String`-typed identifiers passed around | Newtype candidates |
| `pub struct X(String)` without `pub fn parse` constructor | Newtype with broken encapsulation |

Code Ranker's `god-module-coupling` and `high-fan-in-public-api` rules
indirectly capture the "fat trait" issue. A future rule could flag:

- Traits with > N methods AND multiple impls → ISP candidate.
- Functions taking same-type identifiers without newtypes → newtype
  candidate.

## Suggested recommendation template

> **Composition candidate**: trait `Repository` has 8 methods and
> 4 implementations. Several implementations leave half the methods
> as `unimplemented!()`. Decompose into capability traits (`Find`,
> `Save`, `Delete`, etc.) and let each implementation declare only
> the capabilities it supports. This is "compose, don't inherit"
> at the trait level.
>
> Source: Gang of Four (1994); Wuyts, "Combinatorial purity".

## Related principles

- [ISP](ISP.md) — segregation IS the
  Rust-flavoured "favor composition" principle for traits.
- [DIP](DIP.md) — composition is what
  makes DIP cheap (no inheritance to drag in).
- [Make Invalid States Unrepresentable](MISU.md)
  — newtype is the workhorse for this.
- [SRP](SRP.md) — each composed piece has
  one responsibility.

## References

1. Gamma, E., Helm, R., Johnson, R., Vlissides, J. *Design Patterns*.
   1994, p.20.
2. Holub, A. "Why extends is evil". *InfoWorld*, 2003.
   <https://www.infoworld.com/article/2073649/why-extends-is-evil.html>
3. Wuyts, Y. "Combinatorial purity".
   <https://blog.yoshuawuyts.com/combinatorial-purity/>
4. *The Rust Programming Language* book, Ch. 17.1.
   <https://doc.rust-lang.org/book/ch17-01-what-is-oo.html>
5. Rust API Guidelines, "Flexibility".
   <https://rust-lang.github.io/api-guidelines/flexibility.html>
6. matklad. "Tiger Style" (newtype + invariants section).
   <https://matklad.github.io/2024/03/22/basic-things.html>
