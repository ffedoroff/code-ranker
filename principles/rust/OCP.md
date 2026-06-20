# OCP — Open/Closed Principle (in Rust)

**TL;DR**: A module is **open for extension** but **closed for
modification**. In Rust this means: prefer adding a new `impl`, a new
trait, or a new feature flag over editing existing code paths;
hide knobs behind `#[non_exhaustive]`, sealed traits, and typestate.

## Canonical sources

- Bertrand Meyer, *Object-Oriented Software Construction* (1988):
  coined the principle in the inheritance-based form.
- Robert C. Martin, "The Open-Closed Principle" (1996, *C++ Report*):
  reframed for polymorphism rather than inheritance, the version most
  cited today. <https://web.archive.org/web/20060822033314/http://www.objectmentor.com/resources/articles/ocp.pdf>
- Martin, *Clean Architecture* (2017), Ch. 8.
- Rust RFC 1105 — "API Evolution": defines what change is
  semver-compatible. <https://rust-lang.github.io/rfcs/1105-api-evolution.html>
- Rust API Guidelines, "Future Proofing":
  <https://rust-lang.github.io/api-guidelines/future-proofing.html>
- predr.ag, "The Definitive Guide to Sealed Traits in Rust":
  <https://predr.ag/blog/definitive-guide-to-sealed-traits-in-rust/>
- David Tolnay, "the semver trick":
  <https://github.com/dtolnay/semver-trick>

## The principle

A type, module, or crate has fulfilled OCP when **its consumers can
add new behaviour without modifying its source**. Modification is
"reaching inside" — touching fields, adding match arms to private
enums, changing trait method signatures. Extension is "plugging in" —
implementing a trait the type publishes, adding a new module that
uses the type, or activating a Cargo feature.

The deep idea: any line of source you change is a line your existing
users might break on. So make new behaviour additive.

OCP is most often misread as "use inheritance" or "everything must be
abstract". Neither is true. The actual prescription is:

1. Identify the **axes of likely change**.
2. For each axis, expose an extension point that varies along it.
3. Keep everything else **closed** — don't allow callers to depend on
   internals that should be free to evolve.

In a Rust crate, the axes of likely change are usually:

- New variants of an enum (logging output formats, network
  protocols, error kinds).
- New implementations of a trait (new storage backends, new
  authentication schemes).
- New optional fields in a config struct (new flags, new tuning
  knobs).
- New parameters on a function (new context the caller can pass).

For each, Rust has an idiomatic "closed for modification" tool.

## Why it matters

OCP is the principle that protects you from **upstream cascades**:
a one-line change to a popular API ripples through every downstream
consumer at semver-breaking magnitude. Crates with 50+ downstream
crates (your `modkit-db::secure::secure`, with 124 callers) must be
designed to evolve additively or every release blocks the workspace.

The opposite of OCP is *not* "no abstraction" — it is "every change
becomes a major version bump". You feel the absence of OCP through
release notes that say "BREAKING: renamed field; updated method
signature; added required parameter".

## In Rust

The Rust toolchain gives you four sharp tools for OCP.

### 1. `#[non_exhaustive]` — close enums and structs to direct construction

```rust
#[non_exhaustive]
pub enum DatabaseError {
    Connection(io::Error),
    Query(String),
    Migration(MigrationFailure),
}
```

`#[non_exhaustive]` does two things outside the defining crate:

- `match` on the enum must include a wildcard arm.
- A struct with `#[non_exhaustive]` cannot be constructed with a
  struct literal (`Foo { .. }`); only a constructor can.

Concretely: adding a variant (or a field) is no longer a breaking
change for downstream callers. You have **closed** the enum for
exhaustive matching while **opening** it for additional variants.

### 2. Sealed traits — close trait implementations

```rust
// Public trait users may *call*, but cannot *implement*.
mod private { pub trait Sealed {} }
pub trait Storage: private::Sealed {
    fn put(&self, key: &str, value: &[u8]) -> Result<()>;
}

// Internal blanket impl makes it actually sealed.
impl<T> private::Sealed for T where T: Storage {}
```

A user can call `Storage::put` on anything that implements it, but
they cannot write their own implementation. You are free to add
methods to the trait without breaking external code — because no
external code implements the trait. The trait is **closed** for
external implementation while **open** for the crate to add methods.

Source: predr.ag's definitive guide to sealed traits.

### 3. Typestate — close construction paths

```rust
pub struct RequestBuilder<S> { /* state-specific fields */ _marker: PhantomData<S> }

pub struct NoMethod;
pub struct WithMethod;

impl RequestBuilder<NoMethod> {
    pub fn method(self, m: Method) -> RequestBuilder<WithMethod> { /* ... */ }
}

impl RequestBuilder<WithMethod> {
    pub fn send(self) -> impl Future<Output = Response> { /* ... */ }
}
```

`.send()` is only callable after `.method()`. Adding a new
**optional** step (e.g. `.timeout()`) does not change the typestate;
adding a new **required** step would (so reserve typestate for
genuinely required steps).

Source: Will Crichton, "State machines as Rust enums"; greyblake's
"Builder with typestate" tutorial.

### 4. `pub use` from a façade module — close the import paths

```rust
// crates/foo/src/lib.rs
mod internal;            // private
mod another_internal;    // private

pub use internal::PublicApi;
pub use another_internal::OtherApi;
```

Consumers depend on `foo::PublicApi`, not on `foo::internal::PublicApi`.
You can rename or move `internal` without breaking anyone.

(See [DIP](DIP.md) and the note in
[Composition](CoI.md) about avoiding glob
re-exports — those are the *un*closed kind.)

## Violations and remedies

### Anti-pattern: matching exhaustively on a foreign enum

```rust
// In your crate
use foreign_crate::EventKind;

fn dispatch(e: EventKind) {
    match e {
        EventKind::Insert => /* ... */,
        EventKind::Update => /* ... */,
        EventKind::Delete => /* ... */,
        // Foreign crate adds EventKind::Truncate in 1.4.0 — your code panics
        // at compile time without a wildcard arm.
    }
}
```

### Idiomatic fix: defensively wildcard or own the dispatch

```rust
fn dispatch(e: EventKind) {
    match e {
        EventKind::Insert => /* ... */,
        EventKind::Update => /* ... */,
        EventKind::Delete => /* ... */,
        _ => default_handler(e),  // open to upstream additions
    }
}
```

For your own enums you expect to grow, add `#[non_exhaustive]` so
callers are forced into the wildcard pattern.

### Anti-pattern: builder accepting fields one at a time

```rust
pub struct ConnectionOptions {
    pub host: String,
    pub port: u16,
    pub timeout: Duration,
}
```

Adding a field is a breaking change because all the call sites that
build `ConnectionOptions { host, port, timeout }` lack the new field.

### Idiomatic fix: builder + `#[non_exhaustive]` on the options

```rust
#[non_exhaustive]
pub struct ConnectionOptions {
    pub host: String,
    pub port: u16,
    pub timeout: Duration,
}

impl ConnectionOptions {
    pub fn new(host: String) -> Self {
        Self { host, port: 5432, timeout: Duration::from_secs(30) }
    }
    pub fn port(mut self, p: u16) -> Self { self.port = p; self }
    pub fn timeout(mut self, t: Duration) -> Self { self.timeout = t; self }
}
```

Adding `retries: u8` later is non-breaking — the struct cannot be
literal-constructed externally, and the builder gains a new method.

### Anti-pattern: trait that downstream crates implement, then you add methods

```rust
pub trait Cache {
    fn get(&self, k: &str) -> Option<Vec<u8>>;
    fn put(&self, k: &str, v: Vec<u8>);
}
```

Six downstream crates each have their own `impl Cache for FooCache`.
You realize you need eviction control and add `fn evict(&self,
k: &str)`. Every downstream crate fails to compile.

### Idiomatic fix: seal the trait

```rust
mod private { pub trait Sealed {} }

pub trait Cache: private::Sealed {
    fn get(&self, k: &str) -> Option<Vec<u8>>;
    fn put(&self, k: &str, v: Vec<u8>);
}

impl private::Sealed for InMemoryCache {}
impl Cache for InMemoryCache { /* ... */ }

impl private::Sealed for RedisCache {}
impl Cache for RedisCache { /* ... */ }
```

External crates can call `cache.get(k)` but cannot implement `Cache`
for their own types. Adding `evict` is now an additive change inside
the defining crate.

If external implementations are part of the value proposition (e.g.
a plugin system), do NOT seal — instead, provide a default
implementation for new methods so older impls still compile:

```rust
pub trait Cache {
    fn get(&self, k: &str) -> Option<Vec<u8>>;
    fn put(&self, k: &str, v: Vec<u8>);
    fn evict(&self, k: &str) {  // default impl: opt-in
        let _ = self.put(k, Vec::new());
    }
}
```

This keeps the *unsealed* trait closed for breakage at the cost of
giving authors a (sometimes wrong) default.

### Anti-pattern: hardcoded enum dispatch in business logic

```rust
fn render(format: Format, data: &Data) -> String {
    match format {
        Format::Json => render_json(data),
        Format::Toml => render_toml(data),
        Format::Yaml => render_yaml(data),
    }
}
```

Adding `Format::Cbor` modifies `render`. Every place that performs a
match on `Format` is a modification point.

### Idiomatic fix: trait + registry

```rust
pub trait Renderer: Send + Sync {
    fn render(&self, data: &Data) -> String;
    fn format_id(&self) -> &'static str;
}

pub struct RendererRegistry {
    renderers: Vec<Box<dyn Renderer>>,
}

impl RendererRegistry {
    pub fn register(&mut self, r: Box<dyn Renderer>) { self.renderers.push(r); }
    pub fn render(&self, fmt: &str, data: &Data) -> Option<String> {
        self.renderers.iter().find(|r| r.format_id() == fmt).map(|r| r.render(data))
    }
}
```

A new format is a new impl, registered at startup. `RendererRegistry`
itself does not change.

## OCP at the crate level

The strongest form of OCP at the crate boundary is the
**semver trick** (David Tolnay):

```toml
# old_crate v2.0.0  (still maintained for stragglers)
[dependencies]
new_crate = "3.0"
```

```rust
// old_crate src/lib.rs
pub use new_crate::Item;
```

`old_crate::Item` and `new_crate::Item` are now the *same type*.
Downstream code on `old_crate` v1 can still interoperate with code on
`new_crate` v3 because the type identity is preserved. The pattern
opens a path for additive evolution across major-version boundaries.

## How code-ranker detects OCP violations

OCP violations are subtler than SRP — they often look like normal
code until upstream-evolution time. Code Ranker can flag the structural
*precursors*:

| Signal | OCP interpretation |
|---|---|
| Public trait with N implementations across multiple crates | If unsealed, every method addition is breaking. The `high-fan-in-public-api` rule already flags hotspots; OCP advice is to seal. |
| Public enum without `#[non_exhaustive]` matched in many places | Same hazard for variant addition. Code Ranker's `node_visibility` on enums + cross-crate match-count would catch this in a future rule. |
| Public struct with literal-construction sites across crates | Same hazard for field addition. |
| `pub use foo::*` glob re-exports | Closes nothing — every public item of `foo` becomes part of *your* contract; you cannot rename them without breaking. |

Cross-references in code-ranker's catalog:

- `high-fan-in-public-api` already prescribes sealed traits +
  `#[non_exhaustive]`. Severity escalates when the API is unsealed.
- A future `unsealed-public-trait` rule would directly map.

## Suggested recommendation template

> **OCP candidate**: trait `Cache` is public and has 6
> implementations across the workspace. Adding a method to the trait
> currently breaks all 6 implementors. Seal the trait via a private
> supertrait (predr.ag's definitive guide) if external implementations
> are not part of the value proposition; otherwise add `#[non_exhaustive]`
> markers and use default-implemented methods when extending.
>
> Reference: <https://rust-lang.github.io/api-guidelines/future-proofing.html>

## Related principles

- [SRP](SRP.md) — splits before OCP defends.
- [LSP](LSP.md) — defines what "extension" means
  precisely: a substitute that behaves like the base.
- [DIP](DIP.md) — provides the trait-based
  extension point OCP demands.

## References

1. Meyer, B. *Object-Oriented Software Construction*. 1988.
2. Martin, R. C. "The Open-Closed Principle". *C++ Report*, 1996.
   <https://web.archive.org/web/20060822033314/http://www.objectmentor.com/resources/articles/ocp.pdf>
3. Martin, R. C. *Clean Architecture*. Prentice Hall, 2017. Ch. 8.
4. Rust RFC 1105 — "API Evolution".
   <https://rust-lang.github.io/rfcs/1105-api-evolution.html>
5. Rust API Guidelines, future-proofing.
   <https://rust-lang.github.io/api-guidelines/future-proofing.html>
6. predr.ag. "The Definitive Guide to Sealed Traits in Rust".
   <https://predr.ag/blog/definitive-guide-to-sealed-traits-in-rust/>
7. Tolnay, D. "The semver trick".
   <https://github.com/dtolnay/semver-trick>
8. Crichton, W. "State machines as Rust enums (typestate)".
   <https://will-crichton.net/notes/rust-typestate/>
