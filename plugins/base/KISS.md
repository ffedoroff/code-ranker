# KISS — Keep It Simple, Stupid

**TL;DR**: When choosing between two designs that solve the problem,
pick the simpler one. This most often means: fewer type parameters,
fewer interface abstractions, fewer indirection layers, fewer levels
in the module hierarchy. Reach for a tagged union and a switch before
a runtime-polymorphic handle; reach for a function before an
interface; reach for one plain type before a builder.

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
- Brian Kernighan: "Everyone knows that debugging is twice as hard
  as writing a program in the first place. So if you're as clever
  as you can be when you write it, how will you ever debug it?"
  (*The Elements of Programming Style*, 1978)

## The principle

KISS is the discipline of preferring **the boring solution that
works**. It is not "the shortest code". It is "the design with the
least surface area for surprise".

A module violates KISS when:

- It introduces a type parameter where a function with a single type
  would do.
- It introduces an interface where a tagged union would do.
- It introduces a builder where a plain constructor would do.
- It introduces metaprogramming where a function would do.
- It introduces a feature flag where unconditional code would do.
- It introduces an abstraction "in case" a second implementation
  arrives. (See [YAGNI](YAGNI.md).)

The complexity carries a cost: every additional layer is more code
to read, more types to remember, more build time, more chances for
the type checker to point at the wrong line on an error.

Dijkstra: simplicity is a *prerequisite* for reliability. You cannot
build dependable code on top of a design that is too complex to
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

KISS is what keeps onboarding manageable. A new engineer who can
read your code without asking "why is this an interface?" or "where
does this type parameter resolve?" or "what does this feature flag
enable in this build?" — that is KISS achieved.

## In practice

### Standard-library examples of restraint

Well-designed standard libraries lean on a few sharp tools instead
of a sprawling type zoo:

- An optional value is a tagged union with two cases, not a
  polymorphic hierarchy. Pattern matching beats dynamic dispatch for
  "two cases".
- A success-or-error result is the same. There is no `IsError`
  interface hierarchy.
- A growable array is the universal sequence. You pick the array
  first, then specialize only if you actually need a deque or a
  ring buffer.
- A hash map is one type. There is no `Map`/`SortedMap`/
  `OrderedMap`/`MultiMap` family forced on every user.

The leanest libraries are shockingly *small* compared to their
peers. Most of what other languages provide as separate types, they
express through a tagged union, a switch, and a handful of methods.

### The simpler tool first

A useful mental ladder for choosing the simplest tool:

1. **Function** — does this need any state at all?
2. **Function returning a value object** — does it need to bundle
   outputs?
3. **Type with methods** — does this object have state?
4. **Type with methods behind an interface** — does this object need
   to be substitutable?
5. **Interface with multiple implementations** — do you actually have
   multiple implementations *today*?
6. **Parameterized over an interface** — is the variation in types or
   in behaviour?
7. **Runtime-polymorphic handle** — is the variation discovered at
   runtime?
8. **Metaprogramming / code generation** — is the repetition large
   enough that an ordinary function cannot express it?
9. **Custom build step** — is the transformation not expressible in
   any of the above?

Move down only when the rung you are on cannot do the job. Each step
adds significant cost — to readers, to build time, to debuggers.

### Boring infrastructure choices

A healthy ecosystem rewards boring choices: use the well-worn,
widely-understood library for serialization, async, CLI parsing,
error handling, logging/instrumentation, and database access instead
of writing your own.

Reach for these *before* writing your own. Your codebase becomes a
"normal codebase" that any new hire can read.

## Violations and remedies

### Anti-pattern: interface with one implementation

```
interface UserRepository:
    find_by_id(id) -> Optional<User>
    save(u)

type PostgresUserRepository: pool
PostgresUserRepository implements UserRepository  # ...

# No other implementation exists. There is no plan for another.
```

The interface is overhead with no payoff. Calls need a type parameter
or a runtime-polymorphic handle; tests must mock the interface; the
IDE jumps through indirection.

### Idiomatic fix: drop the interface until a second implementation exists

```
type UserRepository: pool

function UserRepository.find_by_id(id) -> Optional<User>:  # ...
function UserRepository.save(u):  # ...
```

When the second backend (an in-memory implementation for tests) is
*actually written*, then extract an interface. Until then, the
concrete type is simpler in every way.

### Anti-pattern: deep type-parameter chain

```
function process(state, repo, cache, metrics)
where
    state:   AppStateLike
    repo:    UserRepository + Send + Sync + Clone
    cache:   Cache<UserId, User> + Send + Sync
    metrics: MetricsRecorder + Send + Sync
:  # ...
```

Six capability bounds, four type parameters. Calling code is verbose;
compiler error messages reference all bounds; small changes to bounds
cascade.

### Idiomatic fix: pass a single state object carrying the wired collaborators

```
type AppState:
    repo:    UserRepository
    cache:   Cache
    metrics: MetricsRecorder

function process(state: AppState):  # ...
```

A runtime-polymorphic handle adds one indirect call per method, which
is almost always negligible. Build times improve drastically;
signatures are readable; downstream callers no longer have to thread
bounds through their own type parameters. Reach for full
specialization only when profiling shows it matters.

### Anti-pattern: builder with one configurable field

```
type ClientBuilder: timeout (optional)
    new():                this.timeout = none
    timeout(t):           this.timeout = t; return this
    build():              return Client(this.timeout or DEFAULT_TIMEOUT)
```

A builder buys you flexibility for *N* knobs. With 1, it is busywork.

### Idiomatic fix: `new(timeout)` plus a default constructor

```
type Client: timeout
    new(timeout):  return Client(timeout)
    default():     return Client(DEFAULT_TIMEOUT)
```

Two function calls. No fluent API. Add a builder when there are 4+
optional knobs and the call sites *visibly suffer*.

### Anti-pattern: metaprogramming for what a function can do

```
macro sum_squared(xs...):
    expand to: sum(x * x for x in [xs...])
```

Metaprogramming is harder to read, harder to debug (no
step-through), harder to autocomplete. Reach for it only when
ordinary types refuse to cooperate (variadic args, code-gen from
external schemas).

### Idiomatic fix: function

```
function sum_squared(xs) -> int:
    return sum(x * x for x in xs)
```

### Anti-pattern: feature-gated speculation

```
# dependency config
default  = []
postgres = depends on postgres driver
sqlite   = depends on sqlite driver
mysql    = depends on mysql driver
redis    = depends on redis driver
memcached = depends on memcached driver
```

Five backends, three of which are not used. Every CI matrix entry
multiplies; every test exists in N versions; every contributor must
remember which flag their code lives under.

### Idiomatic fix: ship one backend; add more only if real demand appears

```
# dependency config
default  = [postgres]
postgres = depends on postgres driver
```

If `sqlite` users materialize, *then* add the option. YAGNI is
KISS's cousin here.

### Anti-pattern: clever shared references instead of straightforward copying

```
function parse_id(s: borrowed string) -> borrowed string:  # ...
```

A function returning a borrowed reference forces every caller to
keep the source alive and manage its lifetime. Useful when the data
is large or frequently copied; overhead when the data is small. For
a 36-byte UUID, just return an owned value (or, better, a `UserId`
value type).

### Idiomatic fix: own the data when copying is cheap

```
function parse_id(s) -> UserId:  # ...
```

The cost of one allocation per parse is negligible; the benefit of
having no shared-reference bookkeeping in the signature is
significant.

## KISS at the package level

The KISS-friendly project:

- Has a flat structure (one or two levels), not a deep tree.
- Has package names that match what they do (no "core", "common",
  "utils" — be specific: "string-ops", "time-helpers").
- Has few dependencies in most packages' manifests.
- Declares shared dependency versions once for the whole project;
  individual packages inherit them.
- Has a short README per package that explains in three paragraphs
  what the package does and what its main types are.

## How code-ranker detects KISS violations

KISS is qualitative; code-ranker detects its *quantitative shadows*:

| Signal | KISS interpretation |
|---|---|
| Package with many feature flags and few users of each | Speculative complexity. |
| Interface with one implementation (in a package) | Speculative abstraction. |
| Function with many capability bounds | Caller-side complexity. |
| Module nesting deeper than 4 levels | Navigation friction. |
| Dependency count above project median × 2 | Heavy dependency footprint. |

A future rule **`single-implementation-interface`**: when an in-package
interface has exactly one implementor in the same project, suggest
collapsing. Severity low, confidence medium (the human can verify
whether a second implementation is planned).

## Suggested recommendation template

> **KISS candidate**: interface `UserRepository` has exactly one
> implementation (`PostgresUserRepository`) in this project. If no
> second implementation is planned, consider inlining the methods
> onto `PostgresUserRepository` directly. The current shape requires
> capability bounds or runtime-polymorphic handles at every call site
> without a corresponding benefit.
>
> Source: KISS — Hoare, "The Emperor's Old Clothes" (1980).

## Related principles

- [YAGNI](YAGNI.md) — KISS and YAGNI overlap heavily; YAGNI is
  scoped to features-you-haven't-used-yet.
- [SRP](SRP.md) — KISS at the module level
  often *is* SRP applied.
- [Composition Over Inheritance](CoI.md)
  — composition tends to be simpler than the alternative.

## References

1. Dijkstra, E. W. "The Humble Programmer". 1972 ACM Turing Award.
   <https://www.cs.utexas.edu/~EWD/transcriptions/EWD03xx/EWD340.html>
2. Hoare, C. A. R. "The Emperor's Old Clothes". 1980 Turing lecture.
   <https://dl.acm.org/doi/10.1145/358549.358561>
3. Ousterhout, J. *A Philosophy of Software Design*. 2nd ed., 2021.
4. Kernighan, B. *The Elements of Programming Style*. 1978.
5. Brooks, F. *The Mythical Man-Month* (anniversary ed.) — the
   "second-system effect" describes the failure mode KISS guards
   against.
