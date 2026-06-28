# KISS — Keep It Simple, Stupid (in Rust)

**TL;DR**: When choosing between two designs that solve the problem,
pick the simpler one. In Rust, this most often means: fewer generic
parameters, fewer trait abstractions, fewer indirection layers, fewer
levels in the module hierarchy. Reach for `enum + match` before
`Box<dyn Trait>`; reach for a function before a trait; reach for one
struct before a builder.

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
- matklad, "Almost Always Always Use a Vector": prefer the simpler
  data structure. <https://matklad.github.io/2024/06/10/almost-always-always-vector.html>
- Brian Kernighan: "Everyone knows that debugging is twice as hard
  as writing a program in the first place. So if you're as clever
  as you can be when you write it, how will you ever debug it?"
  (*The Elements of Programming Style*, 1978)

## The principle

KISS is the discipline of preferring **the boring solution that
works**. It is not "the shortest code". It is "the design with the
least surface area for surprise".

A Rust crate violates KISS when:

- It introduces a generic where a function with a single type would
  do.
- It introduces a trait where an `enum` would do.
- It introduces a builder where a struct literal would do.
- It introduces a macro where a function would do.
- It introduces a feature flag where unconditional code would do.
- It introduces an abstraction "in case" a second implementation
  arrives. (See [YAGNI](YAGNI.md).)

The complexity carries a cost: every additional layer is more code
to read, more types to remember, more compile time, more chances for
the type checker to point at the wrong line on an error.

Dijkstra: simplicity is a *prerequisite* for reliability. You cannot
build trustworthy code on top of a design that is too complex to
hold in your head.

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

In a Rust workspace, KISS is what keeps onboarding manageable. A
new engineer who can read your code without asking "why is this a
trait?" or "where does this generic resolve?" or "what does this
feature flag enable in this build?" — that is KISS achieved.

## In Rust

The Rust ecosystem has, paradoxically, a strong simplicity culture
inside a complex language. The canonical examples:

### Std library examples of restraint

- `Option<T>` is an enum, not a `Box<dyn Maybe<T>>`. Pattern matching
  beats trait dispatch for "two cases".
- `Result<T, E>` is the same. There is no `trait IsError` hierarchy.
- `Vec<T>` is the universal sequence. The std library does not
  ship "lists" and "deques" and "ropes" as separate first-class
  types; you pick `Vec`, then specialize if you actually need
  `VecDeque`. (See matklad's "Almost Always Always Use a Vector".)
- `HashMap<K, V>` is one type. There is no `Map`/`SortedMap`/
  `OrderedMap`/`MultiMap` family.

The Rust standard library is shockingly *small* compared to its
peers. Most of what other languages provide as separate types,
Rust expresses through `enum + match` + a handful of methods.

### The simpler tool first

A useful mental ladder for choosing the simplest tool:

1. **Function** — does this need any state at all?
2. **Function returning a struct** — does it need to bundle outputs?
3. **Struct + impl** — does this object have state?
4. **Struct + impl + trait** — does this object need to be
   substitutable?
5. **Trait with multiple impls** — do you actually have multiple
   implementations *today*?
6. **Generic over a trait** — is the variation in types or in
   behaviour?
7. **`Box<dyn Trait>`** — is the variation discovered at runtime?
8. **Macro** — is the repetition large enough that a function would
   require unusable trait bounds?
9. **proc-macro** — is the textual transformation large enough that
   `macro_rules!` cannot express it?
10. **Custom build script (`build.rs`)** — is the transformation
    not expressible in any of the above?

Move down only when the rung you are on cannot do the job. Each step
adds significant cost — to readers, to compile time, to debuggers.

### Boring infrastructure choices

The Rust ecosystem rewards boring choices:

- `serde` for serialization (instead of hand-written parsers).
- `tokio` for async (instead of one-off executors).
- `clap` for CLI parsing.
- `thiserror` for error enums, `anyhow` for opaque application
  errors.
- `tracing` for instrumentation.
- `sqlx` or `sea-orm` for databases.

Reach for these *before* writing your own. Your codebase becomes a
"normal Rust codebase" that any new hire can read.

## Violations and remedies

### Anti-pattern: trait with one implementation

```rust
pub trait UserRepository {
    fn find_by_id(&self, id: UserId) -> Option<User>;
    fn save(&self, u: &User) -> Result<()>;
}

pub struct PostgresUserRepository { pool: PgPool }
impl UserRepository for PostgresUserRepository { /* ... */ }

// No other impl exists. There is no plan for another.
```

The trait is overhead with no payoff. Calls need a generic bound or
a `Box<dyn>`; tests must mock the trait; the IDE jumps through
indirection.

### Idiomatic fix: drop the trait until a second impl exists

```rust
pub struct UserRepository { pool: PgPool }
impl UserRepository {
    pub fn find_by_id(&self, id: UserId) -> Option<User> { /* ... */ }
    pub fn save(&self, u: &User) -> Result<()> { /* ... */ }
}
```

When the second backend (an in-memory implementation for tests) is
*actually written*, then extract a trait. Until then, the concrete
type is simpler in every way.

### Anti-pattern: deep generic chain

```rust
pub fn process<S, R, C, M>(
    state: S,
    repo: R,
    cache: C,
    metrics: M,
) -> Result<()>
where
    S: AsRef<AppState>,
    R: UserRepository + Send + Sync + Clone + 'static,
    C: Cache<K = UserId, V = User> + Send + Sync + 'static,
    M: MetricsRecorder + Send + Sync + 'static,
{ /* ... */ }
```

Six trait bounds, four generic parameters. Calling code is verbose;
error messages from the compiler reference all bounds; small changes
to bounds cascade.

### Idiomatic fix: take an `Arc<AppState>` (or `&AppState`) carrying the wired collaborators

```rust
pub struct AppState {
    pub repo: Arc<dyn UserRepository>,
    pub cache: Arc<dyn Cache>,
    pub metrics: Arc<dyn MetricsRecorder>,
}

pub fn process(state: &AppState) -> Result<()> { /* ... */ }
```

`dyn` adds one vtable call per method, which is almost always
negligible. Compile times improve drastically; signatures are
readable; downstream callers no longer have to thread bounds
through their own generics. Reach for full monomorphisation only
when profiling shows it matters.

### Anti-pattern: builder with one configurable field

```rust
pub struct ClientBuilder { timeout: Option<Duration> }
impl ClientBuilder {
    pub fn new() -> Self { Self { timeout: None } }
    pub fn timeout(mut self, t: Duration) -> Self { self.timeout = Some(t); self }
    pub fn build(self) -> Client { Client { timeout: self.timeout.unwrap_or(DEFAULT_TIMEOUT) } }
}
```

A builder buys you flexibility for *N* knobs. With 1, it is busywork.

### Idiomatic fix: `new(timeout)` plus a `Client::default()`

```rust
pub struct Client { timeout: Duration }
impl Client {
    pub fn new(timeout: Duration) -> Self { Self { timeout } }
}
impl Default for Client {
    fn default() -> Self { Self { timeout: DEFAULT_TIMEOUT } }
}
```

Two function calls. No fluent API. Add a builder when there are 4+
optional knobs and the call sites *visibly suffer*.

### Anti-pattern: macro for what a function can do

```rust
macro_rules! sum_squared {
    ($($x:expr),+) => {{
        let v = vec![$($x),+];
        v.iter().map(|x| x * x).sum::<i32>()
    }};
}
```

Macros are harder to read, harder to debug (no step-through), harder
to autocomplete. Reach for them only when types refuse to cooperate
(variadic args, code-gen from external schemas).

### Idiomatic fix: function

```rust
pub fn sum_squared(xs: &[i32]) -> i32 {
    xs.iter().map(|x| x * x).sum()
}
```

### Anti-pattern: feature-gated speculation

```toml
[features]
default = []
postgres = ["dep:tokio-postgres"]
sqlite = ["dep:rusqlite"]
mysql = ["dep:mysql_async"]
redis = ["dep:redis"]
memcached = ["dep:memcache"]
```

Five backends, three of which are not used. Every CI matrix entry
multiplies; every test exists in N versions; every contributor must
remember which feature their code lives under.

### Idiomatic fix: ship one backend; carve more crates only if real demand appears

```toml
[features]
default = ["postgres"]
postgres = ["dep:tokio-postgres"]
```

If `sqlite` users materialize, *then* add the feature. YAGNI is
KISS's cousin here.

### Anti-pattern: clever ownership instead of straightforward `Clone`

```rust
fn parse_id(s: &'_ str) -> Result<&'_ str> { /* ... */ }
```

A function returning a `&str` borrow forces every caller to manage
lifetimes. Useful when the data is large or frequently copied;
overhead when the data is small. For a 36-byte UUID, just return a
`String` (or, better, a `UserId` newtype).

### Idiomatic fix: own the data when ownership is cheap

```rust
fn parse_id(s: &str) -> Result<UserId> { /* ... */ }
```

The cost of one allocation per parse is negligible; the benefit of
having no lifetimes in the signature is significant.

## KISS at the crate level

The KISS-friendly Rust workspace:

- Has a flat structure (one or two levels), not a deep tree.
- Has crate names that match what they do (no "core", "common",
  "utils" — be specific: "string-ops", "time-helpers").
- Has fewer than 10 dependencies in most crates' `Cargo.toml`s.
- Has a single workspace `Cargo.toml` that *declares* shared
  dependency versions; individual crates inherit them.
- Has a `README.md` per crate that explains in three paragraphs
  what the crate does and what its main types are.

## How code-ranker detects KISS violations

KISS is qualitative; code-ranker detects its *quantitative shadows*:

| Signal | KISS interpretation |
|---|---|
| Crate with many features and few users of each | Speculative complexity. |
| Trait with one impl (in a crate) | Speculative abstraction. |
| Function with many trait bounds | Caller-side complexity. |
| Module nesting deeper than 4 levels | Navigation friction. |
| Cargo.toml dependency count above project median × 2 | Heavy dependency footprint. |

A future rule **`single-impl-trait`**: when an in-crate trait has
exactly one implementor in the same workspace, suggest collapsing.
Severity low, confidence medium (the human can verify whether a
second impl is planned).

## Suggested recommendation template

> **KISS candidate**: trait `UserRepository` has exactly one
> implementation (`PostgresUserRepository`) in this workspace. If no
> second implementation is planned, consider inlining the methods
> onto `PostgresUserRepository` directly. The current shape requires
> generic bounds or `dyn` at every call site without a corresponding
> benefit.
>
> Source: KISS — Hoare, "The Emperor's Old Clothes" (1980);
> matklad, "Almost Always Always Use a Vector".
<!-- doc:base "Related principles" -->

## References

1. Dijkstra, E. W. "The Humble Programmer". 1972 ACM Turing Award.
   <https://www.cs.utexas.edu/~EWD/transcriptions/EWD03xx/EWD340.html>
2. Hoare, C. A. R. "The Emperor's Old Clothes". 1980 Turing lecture.
   <https://dl.acm.org/doi/10.1145/358549.358561>
3. Ousterhout, J. *A Philosophy of Software Design*. 2nd ed., 2021.
4. matklad. "Almost Always Always Use a Vector". 2024.
   <https://matklad.github.io/2024/06/10/almost-always-always-vector.html>
5. Kernighan, B. *The Elements of Programming Style*. 1978.
6. Brooks, F. *The Mythical Man-Month* (anniversary ed.) — the
   "second-system effect" describes the failure mode KISS guards
   against.
