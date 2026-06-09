# KISS — Keep It Simple, Stupid (in Python)

**TL;DR**: When choosing between two designs that solve the problem,
pick the simpler one. In Python, this most often means: fewer
classes, fewer abstract base classes, fewer protocols, fewer
decorators, fewer levels in the package hierarchy. Reach for a
function before a class; reach for `enum.Enum` + `match` before a
class hierarchy; reach for a dataclass before a builder; reach for
the standard library before an extra dependency.

## Canonical sources

- Kelly Johnson (Lockheed Skunk Works, c. 1960): origin of the
  acronym in engineering folklore.
  <https://en.wikipedia.org/wiki/KISS_principle>
- Edsger Dijkstra, "The Humble Programmer" (1972 ACM Turing Award
  lecture): "Simplicity is prerequisite for reliability."
  <https://www.cs.utexas.edu/~EWD/transcriptions/EWD03xx/EWD340.html>
- Tony Hoare, "The Emperor's Old Clothes" (1980 Turing lecture):
  "I conclude that there are two ways of constructing a software
  design: One way is to make it so simple that there are obviously
  no deficiencies, and the other way is to make it so complicated
  that there are no obvious deficiencies."
  <https://dl.acm.org/doi/10.1145/358549.358561>
- John Ousterhout, *A Philosophy of Software Design* (2018, 2nd ed.
  2021): the concept of **cognitive load** as the modern KISS metric.
- Tim Peters, *The Zen of Python* (PEP 20): "Simple is better than
  complex. Complex is better than complicated. Flat is better than
  nested. Readability counts." <https://peps.python.org/pep-0020/>
- Raymond Hettinger, "Beyond PEP 8 — Best practices for beautiful,
  intelligible code" (PyCon 2015) and "Transforming Code into
  Beautiful, Idiomatic Python".
- Brett Slatkin, *Effective Python* (3rd ed., 2024) — Item 1:
  "Know which version of Python you're using"; the book is
  fundamentally about reaching for the boring tool.
- Brian Kernighan: "Everyone knows that debugging is twice as hard
  as writing a program in the first place. So if you're as clever
  as you can be when you write it, how will you ever debug it?"
  (*The Elements of Programming Style*, 1978)

## The principle

KISS is the discipline of preferring **the boring solution that
works**. It is not "the shortest code" and it is not "the most
expressive code". It is "the design with the least surface area
for surprise".

A Python package violates KISS when:

- It introduces an abstract base class where a function would do.
- It introduces a `Protocol` where a concrete type would do.
- It introduces a class hierarchy where an `Enum` + `match` would do.
- It introduces a builder where a `@dataclass` with defaults would do.
- It introduces a metaclass where a class decorator would do.
- It introduces a decorator where a plain function call would do.
- It introduces a plugin entry point where a direct import would do.
- It introduces an abstraction "in case" a second implementation
  arrives. (See [YAGNI](yagni.md).)

The complexity carries a cost: every additional layer is more code
to read, more names to remember, more import time, more chances
for a type checker (mypy, pyright) to point at the wrong line on an
error, more places for a stack trace to bottom out unhelpfully.

Dijkstra: simplicity is a *prerequisite* for reliability. You
cannot build trustworthy code on top of a design that is too
complex to hold in your head. Tim Peters echoed him three decades
later in *The Zen of Python*: "If the implementation is hard to
explain, it's a bad idea."

## Why it matters

Complexity is **superlinear** in its cost. Each additional
abstraction layer multiplies the reader's mental load: not just by
the size of the layer, but by the interactions with all the layers
above it. Ten layers of three concepts each is harder to understand
than one layer of thirty concepts, because the reader must hold
each layer's invariants in mind while reading the next.

Ousterhout's *Philosophy of Software Design* puts numbers to this:
he calls each non-obvious bit of code a "cognitive load token",
and proposes that good software design minimizes the sum of
cognitive load tokens across all the people who must read the code.

Python is especially vulnerable here because its dynamism makes
*every* abstraction cheap to introduce. You can paper over
anything with `__getattr__`, a metaclass, or a decorator stack.
The discipline is to *not*. A new engineer who can `cd` into your
package and read it top-to-bottom without asking "where is this
class actually instantiated?" or "what does this decorator do?"
or "why is there a registry here?" — that is KISS achieved.

## In Python

The Python ecosystem has a strong simplicity culture, codified in
PEP 20 and reinforced by Hettinger's talks. The canonical examples:

### Standard-library examples of restraint

- `list` is the universal sequence. The standard library does
  ship `deque`, `array`, and `tuple`, but `list` is the default,
  and most code never needs anything else.
- `dict` is the universal mapping. Insertion order is guaranteed
  since 3.7, which killed half the demand for `OrderedDict`.
  There is no `SortedDict` in stdlib because *you don't need one*.
- `None` is the universal "missing value". Python does not ship
  `Option[T]` because `T | None` (PEP 604) with `if x is None`
  is already pattern-matched at the type-checker level.
- Exceptions are the universal error channel. Python does not
  ship `Result[T, E]` because raising and catching is the boring
  default.
- `dataclasses` is one decorator. There is no `Bean`/`Record`/
  `Struct`/`ValueObject` family.

The standard library is shockingly *flat* compared to its peers.
Most of what other languages provide as separate types, Python
expresses through a `dict`, a `list`, a `dataclass`, and a `match`.

### The simpler tool first

A useful mental ladder for choosing the simplest tool:

1. **Function** — does this need any state at all?
2. **Function returning a `@dataclass`** — does it need to
   bundle outputs?
3. **`@dataclass` with methods** — does this object have state?
4. **Plain class** — does it need to participate in inheritance
   or custom `__init__` logic?
5. **`typing.Protocol`** — does the caller need to accept several
   shapes, *and* are those shapes already implemented?
6. **PEP 695 generic `class Foo[T]:`** — is the variation in
   element type rather than behaviour?
7. **`abc.ABC` with `@abstractmethod`** — do you need runtime
   enforcement of the interface, *and* do you have multiple
   implementations *today*?
8. **Class decorator** — is the repetition mechanical and
   per-class?
9. **Metaclass** — is the transformation per-class *and* needs
   to run at class-creation time?
10. **Import hook / AST rewrite / code generation** — is the
    transformation not expressible in any of the above?

Move down only when the rung you are on cannot do the job. Each
step adds significant cost — to readers, to import time, to the
debugger.

### Boring infrastructure choices

The Python ecosystem rewards boring choices:

- `pydantic` for data validation and parsing (instead of
  hand-rolled `__init__` checks).
- `asyncio` for async (instead of `gevent`, `twisted`, or your own
  event loop).
- `click` or `typer` for CLI parsing.
- `structlog` for instrumentation; `logging` from stdlib if you
  want one fewer dependency.
- `sqlalchemy` for databases; `psycopg` if you want raw SQL.
- `httpx` for HTTP; `requests` if you don't need async.
- `pytest` for tests.
- `ruff` for linting and formatting.

Reach for these *before* writing your own. Your codebase becomes a
"normal Python codebase" that any new hire can read.

## Violations and remedies

### Anti-pattern: abstract base class with one implementation

```python
from abc import ABC, abstractmethod

class UserRepository(ABC):
    @abstractmethod
    def find_by_id(self, user_id: UserId) -> User | None: ...
    @abstractmethod
    def save(self, user: User) -> None: ...

class PostgresUserRepository(UserRepository):
    def __init__(self, pool: AsyncConnectionPool) -> None:
        self._pool = pool
    def find_by_id(self, user_id: UserId) -> User | None: ...
    def save(self, user: User) -> None: ...

# No other implementation exists. There is no plan for another.
```

The ABC is overhead with no payoff. Tests must subclass or mock
the ABC; the IDE jumps through indirection; type checkers can't
narrow to the concrete shape.

### Idiomatic fix: drop the ABC until a second impl exists

```python
class UserRepository:
    def __init__(self, pool: AsyncConnectionPool) -> None:
        self._pool = pool
    def find_by_id(self, user_id: UserId) -> User | None: ...
    def save(self, user: User) -> None: ...
```

When the second backend (an in-memory implementation for tests)
is *actually written*, extract a `Protocol` then. Protocols are
structural — you don't even need the concrete class to inherit
from one, so you can defer the abstraction until the moment a
second implementation lands.

### Anti-pattern: deep class hierarchy for variants

```python
class Event: ...
class UserEvent(Event): ...
class UserCreated(UserEvent):
    def __init__(self, user_id: UserId, email: str) -> None: ...
class UserDeleted(UserEvent):
    def __init__(self, user_id: UserId) -> None: ...

def handle(event: Event) -> None:
    if isinstance(event, UserCreated):
        ...
    elif isinstance(event, UserDeleted):
        ...
```

Inheritance encodes the variants, but the dispatch is still
`isinstance` chains. The hierarchy buys nothing.

### Idiomatic fix: a tagged union with `match`

```python
from dataclasses import dataclass

@dataclass(frozen=True, slots=True)
class UserCreated:
    user_id: UserId
    email: str

@dataclass(frozen=True, slots=True)
class UserDeleted:
    user_id: UserId

type Event = UserCreated | UserDeleted

def handle(event: Event) -> None:
    match event:
        case UserCreated(user_id, email):
            ...
        case UserDeleted(user_id):
            ...
```

`match` is exhaustive against the type alias (pyright/mypy flag a
missing case); the dataclasses are flat; there is no inheritance
to chase.

### Anti-pattern: builder with one configurable field

```python
class ClientBuilder:
    def __init__(self) -> None:
        self._timeout: float | None = None
    def timeout(self, t: float) -> "ClientBuilder":
        self._timeout = t
        return self
    def build(self) -> "Client":
        return Client(timeout=self._timeout or DEFAULT_TIMEOUT)
```

A builder buys you flexibility for *N* knobs. With 1, it is
busywork — and Python has keyword arguments and defaults that
already cover the builder use case.

### Idiomatic fix: `@dataclass` with a default

```python
from dataclasses import dataclass

DEFAULT_TIMEOUT = 30.0

@dataclass(frozen=True, slots=True)
class Client:
    timeout: float = DEFAULT_TIMEOUT
```

`Client()`, `Client(timeout=5.0)`. No fluent API. Add a builder
only when there are 4+ optional knobs *and* the call sites
visibly suffer (e.g., construction requires conditional logic).

### Anti-pattern: decorator stack for what a function can do

```python
@retry(times=3)
@timed
@logged
@validated
def fetch_user(user_id: UserId) -> User:
    return _db.get(user_id)
```

Each decorator wraps the function in another frame. Stack traces
balloon, IDE go-to-definition lands on `functools.wraps`, and
behaviour at the boundaries (does `retry` retry on `validated`
failures? does `timed` time the retries?) is non-obvious.

### Idiomatic fix: explicit composition

```python
def fetch_user(user_id: UserId) -> User:
    with log_context(action="fetch_user", user_id=user_id):
        return _with_retry(lambda: _db.get(user_id), times=3)
```

The ordering is visible. Stack frames are flat. Each helper does
one thing. Reach for decorators when the cross-cutting concern is
truly uniform across many call sites and the wrapping is the
*entire* point (`@app.route`, `@pytest.fixture`).

### Anti-pattern: optional-dependency speculation

```toml
# pyproject.toml
[project.optional-dependencies]
postgres = ["psycopg[binary]>=3"]
sqlite = ["aiosqlite>=0.19"]
mysql = ["asyncmy>=0.2"]
redis = ["redis>=5"]
memcached = ["pymemcache>=4"]
```

Five backends, three of which are not used. Every CI matrix entry
multiplies; every test exists in N versions; every contributor
must remember which extra their code lives under.

### Idiomatic fix: ship one backend; add extras only on demand

```toml
[project]
dependencies = ["psycopg[binary]>=3"]
```

If `sqlite` users materialize, *then* add the extra. YAGNI is
KISS's cousin here.

### Anti-pattern: `Protocol` with structural members nobody uses

```python
from typing import Protocol

class Stringy(Protocol):
    def __str__(self) -> str: ...
    def __len__(self) -> int: ...
    def encode(self, encoding: str = "utf-8") -> bytes: ...

def write_line(out: Stringy) -> None: ...
```

If the only caller passes `str`, then `out: str` is the honest
signature. `Protocol`s are powerful, but the moment you write one
you have committed to documenting and stabilising the surface.

### Idiomatic fix: take the concrete type

```python
def write_line(out: str) -> None: ...
```

Promote to a `Protocol` when a second concrete type actually
needs to be accepted.

### Anti-pattern: clever metaclass instead of straightforward class

```python
class _RegistryMeta(type):
    _registry: dict[str, type] = {}
    def __new__(mcs, name, bases, ns):
        cls = super().__new__(mcs, name, bases, ns)
        mcs._registry[name] = cls
        return cls

class Plugin(metaclass=_RegistryMeta): ...
```

A metaclass that auto-registers subclasses moves "what runs" away
from "where it is called". Newcomers cannot follow the control
flow by reading.

### Idiomatic fix: an explicit registry

```python
PLUGINS: dict[str, type[Plugin]] = {}

def register(cls: type[Plugin]) -> type[Plugin]:
    PLUGINS[cls.__name__] = cls
    return cls

@register
class MyPlugin(Plugin): ...
```

The registration is visible at the call site. Removing a plugin
is a one-line edit. No metaclass to understand.

## KISS at the package/module level

The KISS-friendly Python package:

- Has a flat structure (one or two levels under the top package),
  not a deep tree. `mypkg.users` beats `mypkg.domain.users.adapters.persistence`.
- Has module names that match what they do (no `core`, `common`,
  `utils`, `helpers`, `base` — be specific: `string_ops`,
  `time_helpers`).
- Has fewer than 10 runtime dependencies in `pyproject.toml` for
  most libraries.
- Has a single top-level `pyproject.toml` for a monorepo that
  *declares* shared tool configuration; sub-packages inherit it.
- Has a `README.md` per distributable package that explains in
  three paragraphs what the package does and what its main
  symbols are.
- Has `__init__.py` files that re-export a small, deliberate
  public API — not "import everything for convenience".

## How code-ranker detects KISS violations

KISS is qualitative; code-ranker detects its *quantitative shadows*:

| Signal | KISS interpretation |
|---|---|
| Package with many optional extras and few users of each | Speculative complexity. |
| `ABC`/`Protocol` with one implementation in the package | Speculative abstraction. |
| Function signature with 4+ `Protocol`-typed parameters | Caller-side complexity. |
| Package nesting deeper than 4 levels of `__init__.py` | Navigation friction. |
| `pyproject.toml` dependency count above project median × 2 | Heavy dependency footprint. |
| Decorator stack of depth >= 3 on a function | Hidden control flow. |
| Metaclass defined in package, used by < 3 classes | Speculative meta-programming. |

A future rule **`single-impl-abc`**: when an in-package `ABC` or
`Protocol` has exactly one concrete implementor in the same
distribution, suggest collapsing. Severity low, confidence medium
(the human can verify whether a second impl is planned).

## Suggested recommendation template

> **KISS candidate**: abstract base class `UserRepository` has
> exactly one concrete implementation (`PostgresUserRepository`)
> in this package. If no second implementation is planned,
> consider inlining the methods onto `PostgresUserRepository`
> directly and removing the `ABC`. The current shape requires
> callers to type against the ABC or mock it in tests without a
> corresponding benefit. If a second backend appears later, you
> can extract a `typing.Protocol` at that moment — Python's
> structural typing means you do not need the abstraction up
> front.
>
> Source: KISS — Hoare, "The Emperor's Old Clothes" (1980);
> PEP 20, *The Zen of Python*; Hettinger, "Beyond PEP 8".

## Related principles

- [YAGNI](yagni.md) — KISS and YAGNI overlap heavily; YAGNI is
  scoped to features-you-haven't-used-yet.
- [SRP](solid-single-responsibility.md) — KISS at the module
  level often *is* SRP applied.
- [Composition Over Inheritance](composition-over-inheritance.md)
  — composition tends to be simpler than the alternative, and
  Python's duck typing makes composition particularly cheap.

## References

1. Dijkstra, E. W. "The Humble Programmer". 1972 ACM Turing Award.
   <https://www.cs.utexas.edu/~EWD/transcriptions/EWD03xx/EWD340.html>
2. Hoare, C. A. R. "The Emperor's Old Clothes". 1980 Turing lecture.
   <https://dl.acm.org/doi/10.1145/358549.358561>
3. Ousterhout, J. *A Philosophy of Software Design*. 2nd ed., 2021.
4. Peters, T. PEP 20 — *The Zen of Python*. 2004.
   <https://peps.python.org/pep-0020/>
5. Hettinger, R. "Transforming Code into Beautiful, Idiomatic
   Python". PyCon 2013.
6. Hettinger, R. "Beyond PEP 8 — Best practices for beautiful,
   intelligible code". PyCon 2015.
7. Slatkin, B. *Effective Python*. 3rd ed., 2024.
8. Kernighan, B. *The Elements of Programming Style*. 1978.
9. Brooks, F. *The Mythical Man-Month* (anniversary ed.) — the
   "second-system effect" describes the failure mode KISS guards
   against.
10. van Rossum, G., Lehtosalo, J., Langa, Ł. PEP 484 — *Type
    Hints*. 2014. <https://peps.python.org/pep-0484/>
11. PEP 695 — *Type Parameter Syntax*. 2023.
    <https://peps.python.org/pep-0695/>
