# YAGNI — You Aren't Gonna Need It (in Rust)

**TL;DR**: Build for the problem you have now, not the problem you
imagine you might have later. In Rust this becomes: don't add a trait
for a hypothetical second implementation; don't add a generic
parameter for a hypothetical second type; don't expose a `pub` API
for an internal use case; don't add a feature flag for a feature
nobody asked for.

## Canonical sources

- Ron Jeffries, "You're NOT Gonna Need It!" (1998): origin of the
  acronym in Extreme Programming.
  <https://ronjeffries.com/xprog/articles/practices/pracnotneed/>
- Kent Beck, *Extreme Programming Explained* (1999): the practice's
  formulation.
- Martin Fowler, "Yagni" (2015):
  <https://martinfowler.com/bliki/Yagni.html>
- Sandi Metz, "The Wrong Abstraction":
  <https://sandimetz.com/blog/2016/1/20/the-wrong-abstraction>
- John Carmack on premature design (various interviews): "Sometimes
  the elegant implementation is just a function. Not a method. Not
  a class. Not a framework. Just a function."

## The principle

YAGNI says: every feature, abstraction, configuration, or
extensibility point that is **not currently needed** has a real,
present cost — code to read, tests to maintain, documentation to
write, version-compatibility constraints — and zero present benefit.
Its benefit is *hypothetical*. The probability of that benefit being
realized is usually lower than engineers estimate.

The standard error: "We'll add a feature flag for this so we can
turn it off in the future." The future comes, the feature flag is
never used, but the build matrix is now twice as large.

YAGNI complements KISS by giving a temporal argument: even when an
abstraction *would* be appropriate eventually, it is the wrong
investment **now** if "eventually" hasn't arrived.

Fowler's clarification: YAGNI is not "never add anything in advance".
It is "the cost of adding it speculatively is usually higher than
the cost of adding it on-demand, and the on-demand version is more
likely to be the right shape because you have real requirements".

## Why it matters

Speculative engineering hurts in four ways:

1. **Direct cost**: code, tests, docs, code review time.
2. **Carrying cost**: every reader pays for the abstraction in
   cognitive load.
3. **Opportunity cost**: time spent on speculation is time not spent
   on the real problem.
4. **Lock-in cost**: once shipped, the speculative shape is
   semver-frozen. Removing or changing it is breaking.

The fourth is especially severe in libraries. A speculative `trait Foo`
that two downstream crates start implementing becomes a versioning
nightmare even if the original author never wanted it as a public
contract.

YAGNI is partially a humility argument: you cannot predict which
future need will materialize. The history of every library is full
of features added "just in case" that no one used, and missing
features that everyone needed because no one anticipated them.

## In Rust

Rust's design accommodates incremental complexity well — you can
*always* add a trait later when you have a second impl, *always*
add a generic parameter later when you have a second type. YAGNI
takes advantage of this.

### The "trait on demand" pattern

Start with a concrete struct:

```rust
pub struct UserRepository { pool: PgPool }
impl UserRepository {
    pub fn find(&self, id: UserId) -> Option<User> { /* ... */ }
}
```

When the second backend appears (e.g. a memory store for tests),
extract a trait:

```rust
pub trait UserRepository {
    fn find(&self, id: UserId) -> Option<User>;
}

pub struct PostgresUserRepository { pool: PgPool }
impl UserRepository for PostgresUserRepository { /* ... */ }

pub struct MemoryUserRepository { data: Mutex<HashMap<UserId, User>> }
impl UserRepository for MemoryUserRepository { /* ... */ }
```

The refactor is mechanical and small *because the trait is being
extracted from real, working code*. Compare to adding the trait
speculatively before either implementation exists — you'd be
guessing at the right method set.

### The "generic on demand" pattern

```rust
pub fn parse_user_id(s: &str) -> Result<UserId> { /* ... */ }
```

If you discover the same parsing logic applies to `OrderId`, then
make it generic:

```rust
pub fn parse_id<T: From<Uuid>>(s: &str) -> Result<T> { /* ... */ }
```

Don't write `parse_id<T: From<Uuid>>` from the start when only
`UserId` exists.

### `pub` on demand

The most common, most expensive YAGNI violation in Rust: marking
items `pub` "in case someone needs them". Every `pub` item is a
semver commitment. The discipline:

- Default to private (`pub(self)`).
- Promote to `pub(super)` or `pub(crate)` when an intra-crate
  call site needs it.
- Promote to `pub` only when an external consumer actually exists.

When the public API is small, you can evolve internals freely.

### Feature flags on demand

```toml
[features]
default = []
```

Add features only when the feature has a current consumer who needs
the un-flagged version not to apply to them. A feature flag for
"future flexibility" has all of the carrying cost without any of
the value.

## Violations and remedies

### Anti-pattern: trait without a second implementation

```rust
pub trait NotificationSender {
    fn send(&self, to: &str, message: &str) -> Result<()>;
}

pub struct EmailNotificationSender { /* ... */ }
impl NotificationSender for EmailNotificationSender { /* ... */ }
```

Only `EmailNotificationSender` exists. The trait is dead weight: it
adds a level of indirection at every call site, requires test
doubles, and complicates type signatures.

### Idiomatic fix: drop the trait

```rust
pub struct EmailNotificationSender { /* ... */ }
impl EmailNotificationSender {
    pub fn send(&self, to: &str, message: &str) -> Result<()> { /* ... */ }
}
```

When SMS or push notifications arrive, *then* extract a trait.

### Anti-pattern: generic where a concrete type is fine

```rust
pub fn save_user<S: UserStore>(store: &S, u: &User) -> Result<()> { store.save(u) }
```

There is one `UserStore` and one caller. The generic is busywork.

### Idiomatic fix: name the concrete type

```rust
pub fn save_user(store: &UserStore, u: &User) -> Result<()> { store.save(u) }
```

If a second store materializes, the change is small.

### Anti-pattern: configuration knob nobody requested

```rust
pub struct ServerConfig {
    pub listen_addr: SocketAddr,
    pub max_connections: usize,
    pub idle_timeout: Duration,
    pub buffer_size: usize,           // never tuned
    pub read_chunk_size: usize,        // never tuned
    pub write_chunk_size: usize,       // never tuned
    pub backpressure_high_water_mark: usize,  // never tuned
    pub backpressure_low_water_mark: usize,   // never tuned
    pub queue_strategy: QueueStrategy, // one variant ever used
}
```

Twelve knobs. Three actually move. The other nine are speculative
and complicate every config-loading path, every test, every doc page.

### Idiomatic fix: ship with what the user can actually tune

```rust
pub struct ServerConfig {
    pub listen_addr: SocketAddr,
    pub max_connections: usize,
    pub idle_timeout: Duration,
}
```

Add new knobs when a user *asks* for them (i.e., when a real
performance investigation produces "we needed to tune X"). Adding
a field to `#[non_exhaustive]` ServerConfig is non-breaking; removing
one later is breaking.

### Anti-pattern: speculative crate split

```
crates/
├── domain-types/      # newtype IDs only
├── domain-traits/     # trait declarations only
├── domain-impl/       # the actual logic
├── domain-derive/     # macros that derive things on domain types
├── domain-error/      # errors only
└── domain-config/     # configuration only
```

Six crates because "they might be useful separately". They never
are. Every PR touches three of them. Workspace builds slow down.
Dependents pick one of the six and pull all of them transitively.

### Idiomatic fix: one `domain` crate

```
crates/
└── domain/
    ├── types.rs
    ├── traits.rs
    ├── service.rs
    ├── error.rs
    └── lib.rs
```

If a real consumer needs only `domain-types`, *then* extract it.
Until then, one crate is one cohesive thing.

### Anti-pattern: "I'll need this for plugin support"

```rust
// Designed for a plugin system that does not exist yet.
pub struct PluginManager { /* dynamic loading via libloading */ }
pub trait Plugin { /* ... */ }
pub trait PluginHook { /* ... */ }
pub trait PluginContext { /* ... */ }
pub trait PluginLifecycle { /* ... */ }
```

The plugin system is sketched in 400 lines. No plugin has been
written. The actual product has 1.5 use cases that vary, both of
which could be `enum` variants.

### Idiomatic fix: ship two `enum` variants now

```rust
pub enum Behaviour { Strict, Lenient }
```

When the third use case arrives and starts diverging significantly,
revisit. If by then plugin loading is real, design that. The
likelihood you'll still want the original plugin system is low.

### Anti-pattern: feature-flag scaffolding for "future protocols"

```toml
[features]
default = ["http"]
http = []
grpc = []          # nothing uses this
ws = []            # nothing uses this
mqtt = []          # nothing uses this
```

```rust
#[cfg(feature = "grpc")]
pub mod grpc;
```

`grpc.rs` is 30 lines of stubs that have never been exercised. The
feature exists, breaks occasionally in CI, but provides no value.

### Idiomatic fix: delete the stubs

```toml
[features]
```

When gRPC is actually needed, design it then. The stub code will be
the wrong shape anyway.

## YAGNI for libraries vs applications

A subtle but important distinction:

- For **applications**, YAGNI is almost always right. Add features
  when users ask.
- For **libraries**, YAGNI is more nuanced. Some flexibility (e.g.
  `#[non_exhaustive]`, sealed traits) is *cheap insurance* that
  costs little now and saves a breaking-change later. The trade-off:
  ergonomic-cost-now versus semver-cost-later.

The discriminator is **reversibility**: if a hypothetical future
need can be added later without breaking changes, deferring is safe
YAGNI. If adding it later would require a major version bump,
adding it now (cheaply) may be worth it.

In Rust libraries, the cheap defensive moves are:

- `#[non_exhaustive]` on enums and option structs.
- Sealed traits when the trait is for consumers, not implementers.
- `#[doc(hidden)]` pub items for internals that must be reachable
  but are not contract.

These are not YAGNI violations — they are cheap-to-add, expensive-to-add-later
guards. The line is: avoid building **scaffolding for features**, but
keep using **escape hatches for evolution**.

## How code-ranker detects YAGNI violations

YAGNI is the hardest to detect because the violation depends on
**who uses what** in the future, which is unknowable. Code Ranker can
flag *present-day signals*:

| Signal | YAGNI interpretation |
|---|---|
| Trait with 1 in-workspace impl | Possible speculative trait. Same as KISS rule. |
| `pub` item with no out-of-crate callers | Possible speculative `pub`. Detectable from call-graph: any `pub` item whose only callers are in the defining crate is a `pub(crate)` candidate. |
| Generic parameter unused in body (only used in bounds for hypothetical impls) | Hard to detect statically; future LLM-verification target. |
| Cargo features with no source files gated on them | Easy to detect. |
| Feature flag with no internal consumer | Easy to detect. |

A future rule **`unused-pub`**: a `pub` item with no out-of-crate
calls can probably be `pub(crate)`. Severity low; confidence high.
This corresponds to the cargo lint `dead_code` for `pub` items, but
specific to YAGNI semantics.

## Suggested recommendation template

> **YAGNI candidate**: function `process_with_retries` is marked
> `pub` but has no callers outside the defining crate. If no external
> consumer is planned, demote to `pub(crate)`. External `pub` is a
> semver commitment; the smaller your public surface, the more
> freedom you have to refactor.
>
> Reference: Fowler, "Yagni" — <https://martinfowler.com/bliki/Yagni.html>

## Related principles

- [KISS](kiss.md) — KISS is the *what*: pick the simpler design.
  YAGNI is the *when*: don't pick a design before you need it.
- [DRY](dry.md) — premature DRY violates YAGNI (extracting a helper
  for a second use that may never materialize).
- [OCP](solid-open-closed.md) — OCP demands extension points;
  YAGNI says don't build extension points speculatively. They are
  in tension; resolve with the reversibility test.

## References

1. Jeffries, R. "You're NOT Gonna Need It!". 1998.
   <https://ronjeffries.com/xprog/articles/practices/pracnotneed/>
2. Beck, K. *Extreme Programming Explained*. 1999.
3. Fowler, M. "Yagni". 2015.
   <https://martinfowler.com/bliki/Yagni.html>
4. Metz, S. "The Wrong Abstraction". 2016.
   <https://sandimetz.com/blog/2016/1/20/the-wrong-abstraction>
5. Hyrum's Law: <https://www.hyrumslaw.com/> — every observable
   behaviour of your system will be depended upon, which is why
   speculative `pub` is so dangerous.
