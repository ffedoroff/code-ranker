# OCP — Open/Closed Principle (in Python)

**TL;DR**: A module is **open for extension** but **closed for
modification**. In Python this means: prefer adding a new class that
satisfies a `Protocol`, a new plugin registered via
`importlib.metadata` entry points, or a new strategy passed as a
parameter over editing existing dispatch code. Hide knobs behind
keyword-only constructors, frozen dataclasses with `__init__`
factories, and exhaustive `match` over closed `Literal` unions.

## Canonical sources

- Bertrand Meyer, *Object-Oriented Software Construction* (1988):
  coined the principle in the inheritance-based form.
- Robert C. Martin, *Agile Software Development, Principles, Patterns,
  and Practices* (2002), Ch. 9 — and his earlier essay "The
  Open-Closed Principle" (1996, *C++ Report*).
  <https://web.archive.org/web/20060822033314/http://www.objectmentor.com/resources/articles/ocp.pdf>
- Martin, *Clean Architecture* (2017), Ch. 8.
- PEP 544 — Protocols: Structural subtyping (static duck typing).
  <https://peps.python.org/pep-0544/>
- PEP 634/635/636 — Structural Pattern Matching.
  <https://peps.python.org/pep-0634/>
- PEP 695 — Type Parameter Syntax (Python 3.12+).
  <https://peps.python.org/pep-0695/>
- PEP 698 — `typing.override` decorator (Python 3.12+).
  <https://peps.python.org/pep-0698/>
- Python Packaging User Guide, "Creating and discovering plugins"
  via entry points.
  <https://packaging.python.org/en/latest/guides/creating-and-discovering-plugins/>

## The principle

A module, class, or package has fulfilled OCP when **its consumers
can add new behaviour without modifying its source**. Modification is
"reaching inside" — monkey-patching a class, adding `elif` branches
to a private dispatch function, changing a method signature.
Extension is "plugging in" — defining a new class that satisfies a
published `Protocol`, registering a strategy on a registry the
package exposes, or installing a package that advertises an entry
point the host discovers at startup.

The deep idea: any line of source you change is a line your existing
users might break on. So make new behaviour additive.

OCP is most often misread as "use inheritance" or "everything must be
abstract". Neither is true. Python's structural typing makes it
particularly easy to confuse "I can subclass anything" with "this
class is designed for extension". The actual prescription is:

1. Identify the **axes of likely change**.
2. For each axis, expose an extension point that varies along it.
3. Keep everything else **closed** — don't allow callers to depend on
   internals that should be free to evolve.

In a Python package, the axes of likely change are usually:

- New cases of a tagged union (logging output formats, network
  protocols, error kinds).
- New implementations of a `Protocol` (new storage backends, new
  authentication schemes).
- New optional fields in a config dataclass (new flags, new tuning
  knobs).
- New keyword-only parameters on a function (new context the caller
  can pass).

For each, Python has an idiomatic "closed for modification" tool.

## Why it matters

OCP is the principle that protects you from **upstream cascades**: a
one-line change to a popular API ripples through every downstream
consumer at semver-breaking magnitude. Modules with 50+ downstream
importers (your `modkit_db.secure.secure`, with 124 callers) must be
designed to evolve additively or every release blocks the workspace.

The opposite of OCP is *not* "no abstraction" — it is "every change
becomes a major version bump". You feel the absence of OCP through
release notes that say "BREAKING: renamed attribute; changed method
signature; added required positional argument".

Python adds a second pressure: because the language itself does not
stop downstream code from reaching into private attributes, replacing
methods on an imported class, or unpacking a dataclass positionally,
OCP discipline lives mostly in convention and type-checker
configuration. The `_leading_underscore`, `__init_subclass__`,
`@final`, and `__slots__` mechanisms are how you make the convention
load-bearing.

## In Python

Python gives you five sharp tools for OCP.

### 1. Exhaustive `match` over a closed `Literal` union — close the alternatives

```python
from typing import Literal, assert_never

Format = Literal["json", "toml", "yaml"]

def render(fmt: Format, data: Data) -> str:
    match fmt:
        case "json": return render_json(data)
        case "toml": return render_toml(data)
        case "yaml": return render_yaml(data)
        case _ as unreachable: assert_never(unreachable)
```

A `Literal` union is **closed**: a type checker rejects any caller
that passes `"cbor"`. Adding a new value is a deliberate, type-checked
edit at every `match` site (the inverse of OCP — and the right choice
when *every* call site really must consider the new case, e.g.
serialization formats with semantic differences).

This is the tool you reach for when the set of alternatives is
genuinely fixed and the cost of forgetting a case is high.

### 2. `Protocol` + registry — close the dispatch, open the set

```python
from typing import Protocol, runtime_checkable

@runtime_checkable
class Renderer(Protocol):
    format_id: str
    def render(self, data: Data) -> str: ...

class RendererRegistry:
    def __init__(self) -> None:
        self._by_id: dict[str, Renderer] = {}

    def register(self, r: Renderer) -> None:
        self._by_id[r.format_id] = r

    def render(self, fmt: str, data: Data) -> str | None:
        r = self._by_id.get(fmt)
        return r.render(data) if r else None
```

A new format is a new class that satisfies `Renderer`, registered at
startup. `RendererRegistry` itself does not change. This is the
dual of the `Literal` approach: useful when the set is genuinely
open (plugins, third-party formats) and the host need not enumerate
the alternatives.

The trade-off, made explicit:

- **`enum.Enum` / `Literal` union**: adding a value touches every
  `match` site that lacks a `case _:` fallback. The type checker
  catches the omissions. Good when the set is small and stable.
- **Protocol + registry**: adding a class touches only the
  registration point. No type checker can tell you that you forgot
  to register. Good when the set is large or grows out-of-tree.

### 3. Plugin discovery via `importlib.metadata` entry points

```python
# host package: render/__init__.py
from importlib.metadata import entry_points

def load_renderers() -> RendererRegistry:
    reg = RendererRegistry()
    for ep in entry_points(group="my_app.renderers"):
        reg.register(ep.load()())
    return reg
```

```toml
# downstream package: pyproject.toml
[project.entry-points."my_app.renderers"]
cbor = "cbor_renderer:CborRenderer"
```

The host crate has zero knowledge of `cbor_renderer`. Installing the
plugin package is the extension act. This is OCP at the *distribution*
boundary — the strongest closure Python offers, because the host
source is not even rebuilt.

### 4. Strategy via Protocol-typed parameter — close the algorithm site

```python
class HashStrategy(Protocol):
    def hash(self, payload: bytes) -> bytes: ...

def store(blob: bytes, *, hasher: HashStrategy) -> str:
    digest = hasher.hash(blob)
    ...
```

`store` is closed: it never grows an `if hash_algo == "sha256"` ladder.
Each new algorithm is a class that satisfies `HashStrategy`, passed
in by the caller. (See
[DIP](DIP.md) — the depends-on-abstractions
half of the same coin.)

### 5. `@final`, `__slots__`, and frozen dataclasses — close the type itself

```python
from dataclasses import dataclass
from typing import final

@final
@dataclass(frozen=True, slots=True, kw_only=True)
class ConnectionOptions:
    host: str
    port: int = 5432
    timeout_s: float = 30.0
```

- `@final` tells type checkers nobody may subclass this. Prevents
  the "I extended your class with one new method" coupling.
- `frozen=True` blocks attribute reassignment after construction.
- `slots=True` blocks adding attributes that were not declared.
- `kw_only=True` means call sites pass `ConnectionOptions(host=..., port=...)`,
  never positionally — so adding a field with a default is non-breaking.

Adding `retries: int = 0` later is additive: existing keyword call
sites continue to work; nothing positional could have depended on the
field order.

## Violations and remedies

### Anti-pattern: matching exhaustively on a foreign Enum

```python
from foreign_pkg import EventKind  # an Enum the foreign package owns

def dispatch(e: EventKind) -> None:
    match e:
        case EventKind.INSERT: ...
        case EventKind.UPDATE: ...
        case EventKind.DELETE: ...
        # foreign_pkg 1.4.0 adds EventKind.TRUNCATE — your code
        # silently falls through with no handler.
```

### Idiomatic fix: defensively fall back or own the dispatch

```python
def dispatch(e: EventKind) -> None:
    match e:
        case EventKind.INSERT: ...
        case EventKind.UPDATE: ...
        case EventKind.DELETE: ...
        case _: default_handler(e)
```

For your own enums you expect to grow, document that callers must
include `case _:`, and consider exposing a `Protocol`-based registry
instead so adding a value is not the only path to add behaviour.

### Anti-pattern: dataclass with positional fields

```python
@dataclass
class ConnectionOptions:
    host: str
    port: int
    timeout_s: float
```

Adding `retries: int` is a breaking change — every call site
that wrote `ConnectionOptions("db", 5432, 30.0)` either takes the
wrong meaning or fails. Worse, if the new field has a default,
`@dataclass` rejects the definition unless you reorder.

### Idiomatic fix: `kw_only=True` plus defaults

```python
@dataclass(frozen=True, slots=True, kw_only=True)
class ConnectionOptions:
    host: str
    port: int = 5432
    timeout_s: float = 30.0
```

Adding `retries: int = 0` later is non-breaking. Pair with `@final`
if subclassing is not part of the contract.

### Anti-pattern: ABC that downstream packages subclass, then you add methods

```python
from abc import ABC, abstractmethod

class Cache(ABC):
    @abstractmethod
    def get(self, k: str) -> bytes | None: ...
    @abstractmethod
    def put(self, k: str, v: bytes) -> None: ...
```

Six downstream packages each subclass `Cache`. You realize you need
eviction control and add an abstract `evict`. Every downstream
subclass raises `TypeError: Can't instantiate abstract class`.

### Idiomatic fix 1 (closed extension): make new methods non-abstract with a default

```python
class Cache(ABC):
    @abstractmethod
    def get(self, k: str) -> bytes | None: ...
    @abstractmethod
    def put(self, k: str, v: bytes) -> None: ...

    def evict(self, k: str) -> None:  # default impl: opt-in
        self.put(k, b"")
```

External subclasses keep working; the keen ones override `evict`.
The cost is giving authors a (sometimes wrong) default.

### Idiomatic fix 2 (open extension): switch to `Protocol`, never subclass

```python
class Cache(Protocol):
    def get(self, k: str) -> bytes | None: ...
    def put(self, k: str, v: bytes) -> None: ...
```

Structural typing means no downstream package declares
`class FooCache(Cache):` — they just satisfy the shape. Adding a
method *does* still break them (their classes no longer satisfy the
protocol per the checker), but the rupture is local to the type
checker, not to runtime instantiation. Combined with a
`@runtime_checkable` registry and default-implemented helpers, this
is usually the right shape for a public extension point.

### Anti-pattern: monkey-patching as "openness"

```python
# in some downstream package, at import time:
import upstream
_orig = upstream.Client.request
def request(self, *a, **kw):
    log(self, a, kw)
    return _orig(self, *a, **kw)
upstream.Client.request = request
```

This is openness of the worst kind: every importer of
`upstream.Client` now silently behaves differently depending on
import order. The upstream maintainer has no idea this exists. Any
refactor of `Client.request` — renaming, changing arity, making it
async — breaks the patcher at a distance.

### Idiomatic fix: upstream exposes a hook; downstream registers

```python
# upstream
class Client:
    _middlewares: list[Middleware] = []
    @classmethod
    def use(cls, mw: Middleware) -> None: cls._middlewares.append(mw)

# downstream
upstream.Client.use(LoggingMiddleware())
```

Now the extension point is part of the contract. The upstream
maintainer can refactor `request` freely as long as the middleware
chain still runs.

### Anti-pattern: hardcoded Enum dispatch in business logic

```python
class Format(Enum):
    JSON = "json"
    TOML = "toml"
    YAML = "yaml"

def render(fmt: Format, data: Data) -> str:
    match fmt:
        case Format.JSON: return render_json(data)
        case Format.TOML: return render_toml(data)
        case Format.YAML: return render_yaml(data)
```

Adding `Format.CBOR` modifies `render` *and* every other `match` on
`Format` in the codebase. Each is a coordination point.

### Idiomatic fix: Protocol + registry (see section 2 above)

A new format is a new class. The dispatch site never grows.

## OCP at the distribution level

The strongest form of OCP at the package boundary in Python is the
**entry-point pattern** (section 3 above) combined with the
**re-export shim**:

```python
# old_pkg/__init__.py  (v2.x, still on PyPI for stragglers)
from new_pkg import Item  # type identity preserved
__all__ = ["Item"]
```

`old_pkg.Item is new_pkg.Item` — the same class object. Downstream
code on `old_pkg` v2 can `isinstance(x, old_pkg.Item)` and still
match objects produced by code on `new_pkg` v3, because there is
exactly one class. The pattern opens a path for additive evolution
across major-version boundaries.

## How code-ranker detects OCP violations

OCP violations are subtler than SRP — they often look like normal
code until upstream-evolution time. Code Ranker can flag the
structural *precursors*:

| Signal | OCP interpretation |
|---|---|
| Public Protocol/ABC with N implementations across multiple packages | Every method addition risks breaking external subclasses. The `high-fan-in-public-api` rule already flags hotspots; OCP advice is to prefer `Protocol` over `ABC` and to ship new methods with defaults. |
| Public `Enum` value used in many `match` statements without a `case _:` arm | Same hazard for variant addition. Code Ranker's `node_visibility` on enum members plus a cross-module match-arm count would catch this in a future rule. |
| Public class matched on `isinstance` in many call sites | Same hazard for class-hierarchy evolution: adding a sibling class means every `isinstance` chain is a modification point. Push the behaviour onto the class. |
| `from foo import *` glob re-exports | Closes nothing — every public name in `foo` becomes part of *your* contract; you cannot rename them without breaking. |
| Public dataclass without `kw_only=True` | Field addition becomes positional-coupling-breaking; flag for review. |

Cross-references in code-ranker's catalog:

- `high-fan-in-public-api` already prescribes Protocol-with-defaults
  patterns. Severity escalates when the API is an ABC with required
  abstract methods.
- A future `enum-without-default-arm` rule would directly map.

## Suggested recommendation template

> **OCP candidate**: ABC `Cache` is public and has 6 subclasses
> across the workspace. Adding an abstract method to the ABC
> currently breaks all 6 subclasses at instantiation. Convert to a
> `typing.Protocol` (PEP 544) if external implementations are part
> of the value proposition; otherwise add new methods with default
> implementations so older subclasses still instantiate. Consider
> `@final` on leaf classes that should not be further extended.
>
> Reference: <https://peps.python.org/pep-0544/>

## Related principles

- [SRP](SRP.md) — splits before OCP defends.
- [LSP](LSP.md) — defines what "extension"
  means precisely: a substitute that behaves like the base.
- [DIP](DIP.md) — provides the Protocol-based
  extension point OCP demands.
- [Composition over inheritance](CoI.md) —
  the structural choice that makes Protocol-based OCP tractable.

## References

1. Meyer, B. *Object-Oriented Software Construction*. 1988.
2. Martin, R. C. "The Open-Closed Principle". *C++ Report*, 1996.
   <https://web.archive.org/web/20060822033314/http://www.objectmentor.com/resources/articles/ocp.pdf>
3. Martin, R. C. *Agile Software Development, Principles, Patterns,
   and Practices*. Prentice Hall, 2002. Ch. 9.
4. Martin, R. C. *Clean Architecture*. Prentice Hall, 2017. Ch. 8.
5. PEP 544 — Protocols: Structural subtyping.
   <https://peps.python.org/pep-0544/>
6. PEP 634 — Structural Pattern Matching: Specification.
   <https://peps.python.org/pep-0634/>
7. PEP 695 — Type Parameter Syntax.
   <https://peps.python.org/pep-0695/>
8. PEP 698 — `typing.override`.
   <https://peps.python.org/pep-0698/>
9. Python Packaging User Guide, "Creating and discovering plugins".
   <https://packaging.python.org/en/latest/guides/creating-and-discovering-plugins/>
