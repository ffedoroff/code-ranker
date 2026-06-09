# YAGNI — You Aren't Gonna Need It (in Python)

**TL;DR**: Build for the problem you have now, not the problem you
imagine you might have later. In Python this becomes: don't add a
`Protocol` or `ABC` for a hypothetical second implementation; don't
parameterize a class with `[T]` for a hypothetical second type;
don't expose a name in `__all__` (or as a top-level public symbol)
for an internal use case; don't add a config switch, env-var
toggle, or `if TYPE_CHECKING:` branch for a feature nobody asked
for.

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
- Tim Peters, PEP 20 — *The Zen of Python*: "Simple is better than
  complex", "Special cases aren't special enough to break the
  rules", "If the implementation is hard to explain, it's a bad
  idea."
- Raymond Hettinger, "Beyond PEP 8" and various PyCon talks: prefer
  concrete, readable code over architectural cleverness.
- Brett Slatkin, *Effective Python*: "Prefer simple interfaces over
  configurable ones."

## The principle

YAGNI says: every feature, abstraction, configuration, or
extensibility point that is **not currently needed** has a real,
present cost — code to read, tests to maintain, documentation to
write, version-compatibility constraints — and zero present benefit.
Its benefit is *hypothetical*. The probability of that benefit being
realized is usually lower than engineers estimate.

The standard error: "We'll add an env-var toggle for this so we can
turn it off in the future." The future comes, the toggle is never
flipped, but every code path has to be tested in two modes and
every reader has to ask which branch they're in.

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
   cognitive load — and Python's dynamism means the indirection is
   often invisible until runtime.
3. **Opportunity cost**: time spent on speculation is time not spent
   on the real problem.
4. **Lock-in cost**: once shipped on PyPI, the speculative shape is
   part of your public API. Hyrum's Law guarantees somebody depends
   on it. Removing or changing it is a breaking release.

The fourth is especially severe in libraries. A speculative
`class FooProtocol(Protocol):` that two downstream packages start
implementing becomes a versioning nightmare even if the original
author never wanted it as a public contract. Python has no
`pub(crate)` — once a symbol is importable, it is depended on.

YAGNI is partially a humility argument: you cannot predict which
future need will materialize. The history of every library is full
of features added "just in case" that no one used, and missing
features that everyone needed because no one anticipated them.

## In Python

Python's flexibility accommodates incremental complexity well — you
can *always* extract a `Protocol` later when you have a second
implementation, *always* add a `[T]` type parameter later when you
have a second type. Duck typing means callers don't even need to
change. YAGNI takes advantage of this.

### The "Protocol on demand" pattern

Start with a concrete class:

```python
class UserRepository:
    def __init__(self, pool: AsyncConnectionPool) -> None:
        self._pool = pool

    async def find(self, user_id: UserId) -> User | None:
        ...
```

When the second backend appears (e.g. an in-memory store for tests),
extract a `Protocol`:

```python
from typing import Protocol

class UserRepository(Protocol):
    async def find(self, user_id: UserId) -> User | None: ...

class PostgresUserRepository:
    async def find(self, user_id: UserId) -> User | None: ...

class MemoryUserRepository:
    async def find(self, user_id: UserId) -> User | None: ...
```

The refactor is mechanical and small *because the protocol is being
extracted from real, working code*. Compare to writing the
`Protocol` first before either implementation exists — you'd be
guessing at the right method set, and structural typing means
mismatches surface as silent acceptance, not compile errors.

### The "generic on demand" pattern

```python
def parse_user_id(s: str) -> UserId:
    ...
```

If you discover the same parsing logic applies to `OrderId`, *then*
make it generic using PEP 695 syntax:

```python
def parse_id[T: (UserId, OrderId)](s: str, ctor: type[T]) -> T:
    ...
```

Don't write the parameterized form from the start when only
`UserId` exists. `[T]` parameters on every function "in case" are
the Python equivalent of premature inheritance.

### Public on demand

The most common, most expensive YAGNI violation in Python: adding
a name to `__all__` or omitting the leading underscore "in case
someone needs to import it". Every public name is a de facto
semver commitment.

The discipline:

- Default to a leading underscore (`_helper`, `_InternalState`).
- Promote to module-level public when an intra-package call site
  needs it across modules.
- Add to `__all__` (or re-export from `__init__.py`) only when an
  external consumer actually exists.

When the public API is small, you can evolve internals freely. The
Python community has been bitten enough times (`collections.OrderedDict`,
`asyncio` internals) that this discipline pays off.

### Config on demand

```python
# settings.py
class Settings(BaseSettings):
    pass
```

Add settings only when there is a current consumer who needs the
default not to apply to them. An env-var toggle for "future
flexibility" has all of the carrying cost without any of the value:
every test fixture has to set it, every deployment doc has to
explain it.

## Violations and remedies

### Anti-pattern: Protocol without a second implementation

```python
from typing import Protocol

class NotificationSender(Protocol):
    def send(self, to: str, message: str) -> None: ...

class EmailNotificationSender:
    def send(self, to: str, message: str) -> None:
        ...
```

Only `EmailNotificationSender` exists. The `Protocol` is dead weight:
it adds a level of indirection at every type annotation, requires
test doubles (or `unittest.mock.create_autospec`), and complicates
function signatures.

### Idiomatic fix: drop the Protocol

```python
class EmailNotificationSender:
    def send(self, to: str, message: str) -> None:
        ...
```

When SMS or push notifications arrive, *then* extract a `Protocol`.
Or, given Python's duck typing, *don't* — just annotate with
`EmailNotificationSender | SmsNotificationSender` at call sites
until a third arrives.

### Anti-pattern: generic where a concrete type is fine

```python
def save_user[S: UserStore](store: S, user: User) -> None:
    store.save(user)
```

There is one `UserStore` and one caller. The type parameter is
busywork — Python's structural subtyping already lets you pass a
test double without the generic.

### Idiomatic fix: name the concrete type

```python
def save_user(store: UserStore, user: User) -> None:
    store.save(user)
```

If a second store materializes, the change is small.

### Anti-pattern: configuration knob nobody requested

```python
from dataclasses import dataclass

@dataclass(frozen=True, slots=True)
class ServerConfig:
    listen_addr: str
    max_connections: int
    idle_timeout: float
    buffer_size: int                       # never tuned
    read_chunk_size: int                   # never tuned
    write_chunk_size: int                  # never tuned
    backpressure_high_water_mark: int      # never tuned
    backpressure_low_water_mark: int       # never tuned
    queue_strategy: QueueStrategy          # one variant ever used
```

Nine fields. Three actually move. The other six are speculative and
complicate every config-loading path, every test fixture, every doc
page, every `pydantic` schema regeneration.

### Idiomatic fix: ship with what the user can actually tune

```python
@dataclass(frozen=True, slots=True)
class ServerConfig:
    listen_addr: str
    max_connections: int
    idle_timeout: float
```

Add new fields when a user *asks* for them (i.e., when a real
performance investigation produces "we needed to tune X"). Adding
a field with a default is non-breaking; removing one later is
breaking — especially if anyone is constructing the dataclass
positionally.

### Anti-pattern: speculative package split

```
src/
├── mything_types/          # newtype-style IDs only
├── mything_protocols/      # Protocol declarations only
├── mything_impl/           # the actual logic
├── mything_decorators/     # decorators that operate on types
├── mything_errors/         # exception classes only
└── mything_config/         # configuration only
```

Six distributions because "they might be useful separately". They
never are. Every PR touches three of them. `pip install` resolves
six entries. Importers pick one of the six and pull all of them
transitively via install_requires.

### Idiomatic fix: one `mything` package

```
src/mything/
├── __init__.py
├── types.py
├── protocols.py
├── service.py
├── errors.py
└── config.py
```

One `pyproject.toml`, one version number, one changelog. If a real
consumer needs only `mything.types`, *then* extract it. Until then,
one package is one cohesive thing.

### Anti-pattern: "I'll need this for plugin support"

```python
# Designed for a plugin system that does not exist yet.
class PluginManager:
    """Loads plugins via importlib.metadata entry points."""

class Plugin(Protocol): ...
class PluginHook(Protocol): ...
class PluginContext(Protocol): ...
class PluginLifecycle(Protocol): ...
```

The plugin system is sketched in 400 lines. No plugin has been
written. The actual product has 1.5 use cases that vary, both of
which could be `enum.Enum` members handled with `match`.

### Idiomatic fix: ship two enum members now

```python
from enum import Enum

class Behaviour(Enum):
    STRICT = "strict"
    LENIENT = "lenient"

def handle(b: Behaviour) -> None:
    match b:
        case Behaviour.STRICT:
            ...
        case Behaviour.LENIENT:
            ...
```

When the third use case arrives and starts diverging significantly,
revisit. If by then plugin loading is real, design that. The
likelihood you'll still want the original plugin system is low.

### Anti-pattern: env-var scaffolding for "future protocols"

```python
# settings.py
TRANSPORT = os.environ.get("APP_TRANSPORT", "http")  # "http" | "grpc" | "ws" | "mqtt"

if TRANSPORT == "grpc":
    from . import _grpc_transport     # 30 lines of stubs
elif TRANSPORT == "ws":
    from . import _ws_transport       # 30 lines of stubs
elif TRANSPORT == "mqtt":
    from . import _mqtt_transport     # 30 lines of stubs
else:
    from . import _http_transport
```

The non-`http` branches have never been exercised. They occasionally
fail in CI when an import becomes stale, but provide no value.

### Idiomatic fix: delete the stubs

```python
from . import _http_transport
```

When gRPC is actually needed, design it then. The stub code will be
the wrong shape anyway.

### Anti-pattern: speculative `if TYPE_CHECKING:` machinery

```python
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from .future_module import FutureBackend  # noqa: F401 — reserved for v2
```

Type-checking-only imports for symbols that do not yet exist are
pure speculation. They confuse readers ("where is `FutureBackend`?")
and they break the moment a stub goes wrong.

## YAGNI for libraries vs applications

A subtle but important distinction:

- For **applications**, YAGNI is almost always right. Add features
  when users ask.
- For **libraries** published on PyPI, YAGNI is more nuanced. Some
  flexibility (e.g. `**kwargs` passthroughs, `frozen=True` dataclasses
  with defaults, keyword-only arguments) is *cheap insurance* that
  costs little now and saves a breaking-change later. The trade-off:
  ergonomic-cost-now versus semver-cost-later.

The discriminator is **reversibility**: if a hypothetical future
need can be added later without breaking changes, deferring is safe
YAGNI. If adding it later would require a major version bump,
adding it now (cheaply) may be worth it.

In Python libraries, the cheap defensive moves are:

- Keyword-only arguments (`def f(*, x: int)`) so new params can be
  inserted without positional breakage.
- Leading underscore on every internal helper from day one.
- `__all__` defined explicitly and small.
- `from __future__ import annotations` so signatures can mention
  forward types without runtime cost.
- `@final` on classes you do not want subclassed.

These are not YAGNI violations — they are cheap-to-add,
expensive-to-add-later guards. The line is: avoid building
**scaffolding for features**, but keep using **escape hatches for
evolution**.

## How code-ranker detects YAGNI violations

YAGNI is the hardest to detect because the violation depends on
**who uses what** in the future, which is unknowable. Code Ranker can
flag *present-day signals*:

| Signal | YAGNI interpretation |
|---|---|
| `Protocol` or `ABC` with 1 in-package implementation | Possible speculative abstraction. Same as KISS rule. |
| Public symbol in `__all__` with no out-of-package importers | Possible speculative public API. Detectable from import graph: any name in `__all__` whose only importers live inside the defining package is an underscore-rename candidate. |
| Type parameter `[T]` only constrained, never used positionally in the body | Hard to detect statically; future LLM-verification target. |
| Env-var or setting read but no branch consumes a non-default value | Easy to detect. |
| Module gated by `if TYPE_CHECKING:` that imports a symbol with no runtime use site | Easy to detect. |
| Dead module under `src/` with no importers and no entry-point reference | Easy to detect via grep + AST. |
| `if feature_flag: ...` branch where the flag is hard-coded `False` in every config | Easy to detect. |

A future rule **`unused-public`**: a name listed in `__all__` (or
exposed without a leading underscore at package top level) with no
out-of-package importers can probably be renamed with an underscore.
Severity low; confidence high. This is the Python analogue of
`dead_code` for public symbols, scoped to YAGNI semantics.

## Suggested recommendation template

> **YAGNI candidate**: function `process_with_retries` is exported
> from `mypkg/__init__.py` but has no importers outside `mypkg`. If
> no external consumer is planned, rename to `_process_with_retries`
> and drop it from `__all__`. Public names are a semver commitment;
> the smaller your public surface, the more freedom you have to
> refactor.
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
5. Peters, T. PEP 20 — *The Zen of Python*.
   <https://peps.python.org/pep-0020/>
6. Slatkin, B. *Effective Python*. 3rd ed., Addison-Wesley.
7. Hettinger, R. "Beyond PEP 8 — Best practices for beautiful
   intelligible code". PyCon 2015.
8. Hyrum's Law: <https://www.hyrumslaw.com/> — every observable
   behaviour of your system will be depended upon, which is why
   speculative public symbols are so dangerous in a language with
   no enforced visibility.
