# ISP — Interface Segregation Principle (in Python)

**TL;DR**: Clients should not be forced to depend on methods they do
not use. In Python: prefer many small `Protocol`s with focused
responsibility over one wide ABC; let consumers ask for a `Reader`
(one `read` method) rather than a full `io.IOBase`.

## Canonical sources

- Robert C. Martin, *Agile Software Development: Principles, Patterns,
  and Practices* (2002), Ch. 12 — "The Interface Segregation Principle".
- Robert C. Martin, "The Interface Segregation Principle" (1996):
  <https://web.archive.org/web/20060822033314/http://www.objectmentor.com/resources/articles/isp.pdf>
- Martin, *Clean Architecture*, Ch. 10.
- PEP 544 — Protocols: Structural subtyping (static duck typing):
  <https://peps.python.org/pep-0544/>
- PEP 695 — Type Parameter Syntax (Python 3.12+):
  <https://peps.python.org/pep-0695/>
- Python standard library: the `collections.abc` ladder
  (`Iterable` → `Iterator` → `Reversible` → `Collection` → `Sequence`):
  <https://docs.python.org/3/library/collections.abc.html>
- `typing.Protocol` documentation:
  <https://docs.python.org/3/library/typing.html#typing.Protocol>

## The principle

A class or `Protocol` that bundles too many methods forces consumers
to depend on all of them even when only one is needed. Mock
implementations, test doubles, and type annotations all pay the price
of the largest member of the interface.

Martin's original framing was in terms of Java/C# interfaces — large
interfaces caused implementors to leave methods unimplemented or to
throw `UnsupportedOperationException`. Python's failure mode is
*looser* but no less painful:

- Implementors raise `NotImplementedError` from methods their backend
  cannot honour, deferring the error to runtime.
- Implementors satisfy methods awkwardly (e.g. an `S3Storage` forced
  to implement `seek` because it inherits from `io.IOBase`).
- Static type checkers (`mypy`, `pyright`) flag missing methods even
  when the call site only uses one.
- Test mocks become bloated — `MagicMock(spec=Service)` exposes 20
  attributes when the unit under test touches two.

ISP says: **fold a fat interface into several thin ones**, then let
each consumer declare exactly the surface it needs.

## Why it matters

In a project with many implementors, fat interfaces create a
**double bind**:

1. **Implementors are penalized** — they must implement every method
   even when only one is meaningful for their backend.
2. **Consumers are penalized** — they cannot use the interface in
   contexts where only one method is needed (e.g. a function that
   only iterates cannot accept a generator because the type hint
   demands `list[T]`).

ISP also has a strong interaction with [LSP](LSP.md):
small Protocols have small contracts that are easier to write down,
easier to test, and easier to honour. A 12-method Protocol has a
12-fold larger contract surface; an implementation that gets 11 right
and one slightly wrong still passes the type check.

In Python specifically, ISP is the **Protocol counterpart of SRP**:
SRP is about modules and the actors they serve; ISP is about
Protocols/ABCs and the consumers they serve.

## In Python

Python's structural typing rewards ISP-style factoring at every
level. Because `typing.Protocol` does not require subclassing, you
can define narrow interfaces *post-hoc* — even for classes you do
not own.

### The collections.abc exemplar

```python
from collections.abc import Iterable, Iterator, Collection, Sequence

# Each ABC adds exactly one capability over the previous:
#   Iterable   → __iter__
#   Iterator   → __iter__ + __next__
#   Collection → Iterable + __len__ + __contains__
#   Sequence   → Collection + __getitem__ + index + count
```

A function that sums numbers asks for `Iterable[int]`. A function
that needs random access asks for `Sequence[int]`. A function that
needs membership testing without iteration asks for `Container[int]`.
Each consumer declares exactly the surface it needs; each
implementor provides only what its underlying data structure can
support efficiently.

`list` satisfies all four because lists support all four. A
generator satisfies `Iterable` and `Iterator` but not `Sequence`.
A function written against `Sequence[int]` will not type-check
against a generator, which is the desired outcome — you cannot
index a generator.

### Narrow Protocols beat wide ABCs

```python
from typing import Protocol

class Reader(Protocol):
    def read(self, n: int = -1, /) -> bytes: ...

class Writer(Protocol):
    def write(self, data: bytes, /) -> int: ...

class Seeker(Protocol):
    def seek(self, offset: int, whence: int = 0, /) -> int: ...

def copy(src: Reader, dst: Writer) -> int:
    total = 0
    while chunk := src.read(8192):
        total += dst.write(chunk)
    return total
```

`copy` accepts *anything* with `read` and `write` methods of the
right shape — `io.BytesIO`, an open file, a custom socket wrapper,
a test double — without those types declaring `Reader` or `Writer`
as a base class. Structural subtyping makes ISP nearly free.

### Generic Protocols with PEP 695 syntax

```python
class Repository[T](Protocol):
    def get(self, id: str, /) -> T | None: ...
    def put(self, id: str, value: T, /) -> None: ...
```

A function that needs three capabilities lists them as an
intersection-ish union via multiple parameters or via a combined
Protocol:

```python
class ReadSeek(Reader, Seeker, Protocol): ...

def parse_header(src: ReadSeek) -> Header: ...
```

The composed Protocol exists only because the consumer needed both
capabilities together — it is *ad-hoc* and exactly describes the
consumer's needs.

### `@override` keeps the contract honest

```python
from typing import override

class FileReader:
    @override
    def read(self, n: int = -1, /) -> bytes:
        ...
```

`@override` (PEP 698, Python 3.12+) catches the case where the base
Protocol method signature drifts and a class silently stops
satisfying it.

## Violations and remedies

### Anti-pattern: fat ABC covering every backend feature

```python
from abc import ABC, abstractmethod

class Database(ABC):
    @abstractmethod
    def query(self, sql: str) -> Rows: ...
    @abstractmethod
    def execute(self, sql: str) -> int: ...
    @abstractmethod
    def begin_transaction(self) -> Tx: ...
    @abstractmethod
    def commit(self, tx: Tx) -> None: ...
    @abstractmethod
    def rollback(self, tx: Tx) -> None: ...
    @abstractmethod
    def migrate(self, m: Migration) -> None: ...
    @abstractmethod
    def dump(self) -> bytes: ...
    @abstractmethod
    def restore(self, data: bytes) -> None: ...
    @abstractmethod
    def vacuum(self) -> None: ...
    @abstractmethod
    def metrics(self) -> DbMetrics: ...
    @abstractmethod
    def health(self) -> Health: ...
    @abstractmethod
    def subscribe(self, channel: str) -> Iterator[Notification]: ...
```

A `SqliteDatabase` subclass is forced to fake `subscribe` (no
pub/sub). A read-only replica is forced to fake `execute`. A
migration runner that only needs `migrate` must accept the whole
surface.

### Idiomatic fix: split by capability into Protocols

```python
class Query(Protocol):
    def query(self, sql: str) -> Rows: ...

class Execute(Protocol):
    def execute(self, sql: str) -> int: ...

class Transactional(Protocol):
    def begin(self) -> Tx: ...
    def commit(self, tx: Tx) -> None: ...
    def rollback(self, tx: Tx) -> None: ...

class Migratable(Protocol):
    def migrate(self, m: Migration) -> None: ...

class Backup(Protocol):
    def dump(self) -> bytes: ...
    def restore(self, data: bytes) -> None: ...

class PubSub(Protocol):
    def subscribe(self, channel: str) -> Iterator[Notification]: ...
```

The concrete `SqliteDatabase` class structurally satisfies
`Query + Execute + Transactional + Migratable + Backup` but not
`PubSub`. A `read_only_replica()` returns something satisfying only
`Query`. The migration runner accepts `db: Migratable` — and a
mock for testing it is two lines.

If consumers commonly need three or four together, define a
composed Protocol:

```python
class DatabaseFull(Query, Execute, Transactional, Migratable, Protocol):
    pass
```

But avoid making `DatabaseFull` the *primary* type — it should be a
convenience over the segregated parts.

### Anti-pattern: god `UserService` class as a type hint

```python
class UserService:
    def create(self, ...) -> User: ...
    def deactivate(self, ...) -> None: ...
    def rotate_password(self, ...) -> None: ...
    def export_gdpr(self, ...) -> bytes: ...
    def send_welcome_email(self, ...) -> None: ...
    def assign_role(self, ...) -> None: ...

def signup_handler(svc: UserService, ...) -> User:  # too wide
    return svc.create(...)
```

A test for `signup_handler` must mock all six methods on `UserService`
(or use `MagicMock(spec=UserService)`, which silently allows typos).
A notification component that only sends welcome emails must accept
the whole `UserService`.

### Idiomatic fix: Protocols per use case

```python
class CreateUser(Protocol):
    def create(self, ...) -> User: ...

class DeactivateUser(Protocol):
    def deactivate(self, ...) -> None: ...

class WelcomeMailer(Protocol):
    def welcome(self, ...) -> None: ...

# ... and so on

def signup_handler(svc: CreateUser, ...) -> User:
    return svc.create(...)
```

The concrete `UserService` still implements all six methods.
Consumers ask for the Protocol they actually need. Mocks become
trivially small — any object with a `create` method satisfies
`CreateUser`.

### Anti-pattern: `raise NotImplementedError` in a method body

```python
class Cache(ABC):
    @abstractmethod
    def get(self, k: str) -> bytes | None: ...
    @abstractmethod
    def put(self, k: str, v: bytes) -> None: ...
    @abstractmethod
    def evict(self, k: str) -> None: ...
    @abstractmethod
    def evict_all(self) -> None: ...
    @abstractmethod
    def ttl_seconds(self) -> int | None: ...

class FakeCacheForTest(Cache):
    def get(self, k): return self._data.get(k)
    def put(self, k, v): self._data[k] = v
    def evict(self, k): raise NotImplementedError    # smell
    def evict_all(self): raise NotImplementedError    # smell
    def ttl_seconds(self): return None
```

The `NotImplementedError` raises are runtime ISP debt — the ABC is
too broad for the test.

### Idiomatic fix: split

```python
class CacheGet(Protocol):
    def get(self, k: str) -> bytes | None: ...

class CachePut(Protocol):
    def put(self, k: str, v: bytes) -> None: ...

class CacheEvict(Protocol):
    def evict(self, k: str) -> None: ...
    def evict_all(self) -> None: ...

class CacheTtl(Protocol):
    def ttl_seconds(self) -> int | None: ...
```

The test fake implements only what the test needs (e.g. `CacheGet`
- `CachePut`). The type checker enforces it.

## ISP at the package level

The same principle applies to **packages and modules**: a package's
public surface should be focused. The classic anti-pattern is a
"kitchen sink" module (`utils.py`, `common.py`, `helpers.py`) that
becomes a dependency of everything and hard to update.

Apply ISP at the package level by splitting:

```
mypkg/utils.py        ←  becomes  →   mypkg/strings.py
                                      mypkg/timeutil.py
                                      mypkg/collections_ext.py
```

Now a consumer that needs only string helpers imports
`from mypkg.strings import slugify`, not the whole drawer — and a
refactor to `timeutil` does not invalidate imports of `slugify`.

The narrowest signal is the **prefer `Iterable[T]` over `list[T]`**
rule: if a function only iterates its argument, accepting `list[T]`
(or `Sequence[T]`) over-constrains every caller.

## How code-ranker detects ISP violations

The structural signals:

| Signal | ISP interpretation |
|---|---|
| Protocol/ABC with > N methods (high method-count) | Possible fat interface. Threshold tunable per project. Future rule. |
| Multiple implementations raising `NotImplementedError` in method bodies | Direct ISP smell. Requires AST inspection. Future rule. |
| Protocol imported by many modules but only one method called from most call sites | Fan-out asymmetry — most callers want a smaller surface. Detectable via call-graph aggregation per method. |
| Module consumed by N modules where each only uses 1–2 of its M public names | "Kitchen sink" module. Detectable from existing import graph. |
| Function parameter typed `list[T]` / concrete class while body only iterates | Over-narrow input. Could be widened to `Iterable[T]`. |

A concrete future rule code-ranker could add:

**`fat-protocol`**: `Protocol` or ABC has ≥ 7 public methods AND
has ≥ 2 implementations across the project AND no segregated
narrower Protocols exist. Severity: low / medium. Citation: this
document + Martin's ISP paper.

## Suggested recommendation template

> **ISP candidate**: ABC `Database` exposes 12 abstract methods and
> has 4 implementations across the project. Several implementations
> raise `NotImplementedError` for methods their backend cannot
> support. Split the ABC into capability-segregated Protocols
> (`Query`, `Execute`, `Transactional`, `Migratable`, `Backup`,
> `PubSub`) and let each consumer ask for exactly the capabilities
> it needs. The Python `collections.abc` ladder
> (`Iterable` → `Collection` → `Sequence`) is the canonical model.
>
> References:
>  - <https://web.archive.org/web/20060822033314/http://www.objectmentor.com/resources/articles/isp.pdf>
>  - <https://docs.python.org/3/library/collections.abc.html>

## Related principles

- [SRP](SRP.md) — SRP segregates *modules*;
  ISP segregates *Protocols*. They reinforce each other.
- [LSP](LSP.md) — small Protocols have small
  contracts; ISP makes LSP affordable.
- [DIP](DIP.md) — DIP wants consumers to
  depend on Protocols; ISP keeps those Protocols small enough to be
  worth depending on.
- [Composition Over Inheritance](CoI.md)
  — composing small Protocol bounds (a parameter that satisfies
  `Reader` and `Seeker`) is the Python expression of "compose,
  don't inherit", and structural subtyping makes the composition
  free.

## References

1. Martin, R. C. *Agile Software Development: Principles, Patterns,
   and Practices*. Prentice Hall, 2002. Ch. 12.
2. Martin, R. C. "The Interface Segregation Principle". 1996.
   <https://web.archive.org/web/20060822033314/http://www.objectmentor.com/resources/articles/isp.pdf>
3. Martin, R. C. *Clean Architecture*. Ch. 10.
4. PEP 544 — Protocols: Structural subtyping (static duck typing).
   <https://peps.python.org/pep-0544/>
5. PEP 695 — Type Parameter Syntax.
   <https://peps.python.org/pep-0695/>
6. PEP 698 — Override Decorator for Static Typing.
   <https://peps.python.org/pep-0698/>
7. Python standard library `collections.abc` documentation.
   <https://docs.python.org/3/library/collections.abc.html>
8. Python `typing.Protocol` documentation.
   <https://docs.python.org/3/library/typing.html#typing.Protocol>
