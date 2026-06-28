# YAGNI — You Aren't Gonna Need It

**TL;DR**: Build for the problem you have now, not the problem you
imagine you might have later. In practice this becomes: don't add an
interface for a hypothetical second implementation; don't add a generic
parameter for a hypothetical second type; don't expose a public API
for an internal use case; don't add a feature toggle for a feature
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

The standard error: "We'll add a feature toggle for this so we can
turn it off in the future." The future comes, the toggle is
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
   frozen by your version contract. Removing or changing it is breaking.

The fourth is especially severe in libraries. A speculative interface
that two downstream packages start implementing becomes a versioning
nightmare even if the original author never wanted it as a public
contract.

YAGNI is partially a humility argument: you cannot predict which
future need will materialize. The history of every library is full
of features added "just in case" that no one used, and missing
features that everyone needed because no one anticipated them.

## Techniques

Most languages accommodate incremental complexity well — you can
*always* add an interface later when you have a second implementation,
*always* add a generic parameter later when you have a second type.
YAGNI takes advantage of this.

### The "interface on demand" pattern

Start with a concrete type:

```
type UserRepository { pool: DbPool }
UserRepository.find(id) -> Option<User>
```

When the second backend appears (e.g. a memory store for tests),
extract an interface:

```
interface UserRepository {
    find(id) -> Option<User>
}

type PostgresUserRepository { pool: DbPool } implements UserRepository
type MemoryUserRepository   { data: Map<UserId, User> } implements UserRepository
```

The refactor is mechanical and small *because the interface is being
extracted from real, working code*. Compare to adding the interface
speculatively before either implementation exists — you'd be
guessing at the right method set.

### The "generic on demand" pattern

```
function parse_user_id(s) -> Result<UserId> { /* ... */ }
```

If you discover the same parsing logic applies to `OrderId`, then
make it generic:

```
function parse_id<T>(s) -> Result<T>  where T can be built from a UUID
```

Don't write the generic version from the start when only `UserId`
exists.

### Visibility on demand

The most common, most expensive YAGNI violation: marking items public
"in case someone needs them". Every public item is a version
commitment. The discipline:

- Default to private.
- Promote to module-private when an intra-package call site needs it.
- Promote to fully public only when an external consumer actually
  exists.

When the public API is small, you can evolve internals freely.

### Feature toggles on demand

Add feature toggles only when the feature has a current consumer who
needs the un-toggled version not to apply to them. A toggle for
"future flexibility" has all of the carrying cost without any of
the value.

## Violations and remedies

### Anti-pattern: interface without a second implementation

```
interface NotificationSender {
    send(to, message) -> Result
}

type EmailNotificationSender implements NotificationSender { /* ... */ }
```

Only `EmailNotificationSender` exists. The interface is dead weight:
it adds a level of indirection at every call site, requires test
doubles, and complicates type signatures.

### Idiomatic fix: drop the interface

```
type EmailNotificationSender { /* ... */ }
EmailNotificationSender.send(to, message) -> Result
```

When SMS or push notifications arrive, *then* extract an interface.

### Anti-pattern: generic where a concrete type is fine

```
function save_user<S>(store: S, u) -> Result  where S is a UserStore {
    store.save(u)
}
```

There is one `UserStore` and one caller. The generic is busywork.

### Idiomatic fix: name the concrete type

```
function save_user(store: UserStore, u) -> Result { store.save(u) }
```

If a second store materializes, the change is small.

### Anti-pattern: configuration knob nobody requested

```
type ServerConfig {
    listen_addr: SocketAddr
    max_connections: int
    idle_timeout: Duration
    buffer_size: int                  // never tuned
    read_chunk_size: int              // never tuned
    write_chunk_size: int             // never tuned
    backpressure_high_water_mark: int // never tuned
    backpressure_low_water_mark: int  // never tuned
    queue_strategy: QueueStrategy     // one variant ever used
}
```

Nine knobs. Three actually move. The other six are speculative
and complicate every config-loading path, every test, every doc page.

### Idiomatic fix: ship with what the user can actually tune

```
type ServerConfig {
    listen_addr: SocketAddr
    max_connections: int
    idle_timeout: Duration
}
```

Add new knobs when a user *asks* for them (i.e., when a real
performance investigation produces "we needed to tune X"). Adding
a field to an extensible `ServerConfig` is non-breaking; removing
one later is breaking.

### Anti-pattern: speculative package split

```
packages/
├── domain-types/      # ID wrappers only
├── domain-contracts/  # interface declarations only
├── domain-logic/      # the actual logic
├── domain-codegen/    # code generation over domain types
├── domain-error/      # errors only
└── domain-config/     # configuration only
```

Six packages because "they might be useful separately". They never
are. Every change touches three of them. Builds slow down.
Dependents pick one of the six and pull all of them transitively.

### Idiomatic fix: one `domain` package

```
packages/
└── domain/
    ├── types
    ├── contracts
    ├── service
    ├── error
    └── entry point
```

If a real consumer needs only `domain-types`, *then* extract it.
Until then, one package is one cohesive thing.

### Anti-pattern: "I'll need this for plugin support"

```
// Designed for a plugin system that does not exist yet.
type PluginManager { /* dynamic loading */ }
interface Plugin { /* ... */ }
interface PluginHook { /* ... */ }
interface PluginContext { /* ... */ }
interface PluginLifecycle { /* ... */ }
```

The plugin system is sketched in 400 lines. No plugin has been
written. The actual product has 1.5 use cases that vary, both of
which could be variants of one type.

### Idiomatic fix: ship two variants now

```
type Behaviour = Strict | Lenient
```

When the third use case arrives and starts diverging significantly,
revisit. If by then plugin loading is real, design that. The
likelihood you'll still want the original plugin system is low.

### Anti-pattern: scaffolding for "future protocols"

```
// build configuration declares optional capabilities
optional: http (default), grpc, ws, mqtt
// grpc, ws, mqtt: nothing uses these

// a module gated on the "grpc" capability
module grpc   // 30 lines of stubs, never exercised
```

The `grpc` module is 30 lines of stubs that have never been
exercised. The capability exists, breaks occasionally in CI, but
provides no value.

### Idiomatic fix: delete the stubs

```
// build configuration declares no speculative capabilities
```

When gRPC is actually needed, design it then. The stub code will be
the wrong shape anyway.

## YAGNI for libraries vs applications

A subtle but important distinction:

- For **applications**, YAGNI is almost always right. Add features
  when users ask.
- For **libraries**, YAGNI is more nuanced. Some flexibility (e.g.
  extensible types, sealed interfaces) is *cheap insurance* that
  costs little now and saves a breaking-change later. The trade-off:
  ergonomic-cost-now versus version-cost-later.

The discriminator is **reversibility**: if a hypothetical future
need can be added later without breaking changes, deferring is safe
YAGNI. If adding it later would require a major version bump,
adding it now (cheaply) may be worth it.

In libraries, the cheap defensive moves are:

- Extensible markers on discriminated types and option types.
- Sealed interfaces when the interface is for consumers, not
  implementers.
- Hidden-from-docs public items for internals that must be reachable
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
| Interface with 1 in-project implementation | Possible speculative interface. Same as KISS rule. |
| Public item with no out-of-package callers | Possible speculative public surface. Detectable from call-graph: any public item whose only callers are in the defining package is a candidate for reduced visibility. |
| Generic parameter unused in body (only used in bounds for hypothetical implementations) | Hard to detect statically; future LLM-verification target. |
| Optional build capabilities with no source files gated on them | Easy to detect. |
| Feature toggle with no internal consumer | Easy to detect. |

A future rule **`unused-public`**: a public item with no
out-of-package calls can probably be made module-private. Severity
low; confidence high. This corresponds to dead-code detection for
public items, but specific to YAGNI semantics.

## Suggested recommendation template

> **YAGNI candidate**: function `process_with_retries` is public
> but has no callers outside the defining package. If no external
> consumer is planned, reduce its visibility to module-private. A
> public surface is a version commitment; the smaller your public
> surface, the more freedom you have to refactor.
>
> Reference: Fowler, "Yagni" — <https://martinfowler.com/bliki/Yagni.html>

## Related principles

- [KISS](KISS.md) — KISS is the *what*: pick the simpler design.
  YAGNI is the *when*: don't pick a design before you need it.
- [DRY](DRY.md) — premature DRY violates YAGNI (extracting a helper
  for a second use that may never materialize).
- [OCP](OCP.md) — OCP demands extension points;
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
   speculative public surface is so dangerous.
