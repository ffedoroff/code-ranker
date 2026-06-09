# LSP — Liskov Substitution Principle (in Python)

**TL;DR**: A subclass `S` (or any structural subtype) must be usable
everywhere its parent `T` — whether an `abc.ABC`, a `typing.Protocol`,
or a concrete class — is expected, without surprises. In Python, LSP
shows up at two layers: nominal subtyping through inheritance/ABCs,
and structural subtyping through `Protocol`. The type checker
(`mypy`/`pyright`) catches some signature-level violations; behavioural
contract violations slip through and cause runtime astonishment.

## Canonical sources

- Barbara Liskov, "Data Abstraction and Hierarchy" (1988 SIGPLAN
  keynote) and Liskov & Wing, "A Behavioral Notion of Subtyping"
  (1994):
  <https://dl.acm.org/doi/10.1145/197320.197383>
- Robert C. Martin, "The Liskov Substitution Principle" (1996):
  <https://www.labri.fr/perso/clement/enseignements/ao/LSP.pdf>
- Martin, *Clean Architecture*, Ch. 9.
- PEP 544 — Protocols: Structural subtyping (static duck typing).
  <https://peps.python.org/pep-0544/>
- PEP 698 — `@override` decorator. <https://peps.python.org/pep-0698/>
- PEP 695 — Type Parameter Syntax. <https://peps.python.org/pep-0695/>

## The principle

In Liskov's words: if `S` is a subtype of `T`, then objects of type
`T` may be replaced with objects of type `S` without altering any of
the desirable properties of the program.

The crucial word is **desirable** — Liskov is not asking that the
substitute be *identical*, only that it respect the **behavioural
contract** consumers depend on. Two implementations of `Iterator`
may use entirely different data, but both must:

- Raise `StopIteration` at end of iteration and continue to raise it
  on subsequent `__next__` calls (the iterator-protocol contract).
- Not raise exceptions outside the documented set.
- Return values of the declared element type, not `None` "as a
  sentinel" snuck in.

Each of these is part of the iterator contract, even though none are
type-checked. An implementation that violates them is **valid Python**
but semantically a Liskov violation: it imports cleanly and breaks
consumers that relied on the contract.

LSP is enforced by **discipline, type-checker hints, and tests**, not
by the interpreter. Python proves types match (when you run a type
checker); LSP demands that *behaviours match*.

## Why it matters

Python's dynamism makes LSP violations especially easy to write and
especially painful in production:

- Duck typing means a subclass that "looks like" the parent passes
  isinstance checks but may diverge arbitrarily at runtime.
- The absence of `final` (until `typing.final`) means any method is
  overridable, including ones the author never intended to be.
- Exception hierarchies are open: a subclass override can raise
  anything, and the parent's docstring is the only contract.
- Mutable built-in collections (`list`, `dict`, `set`) are invariant
  by design — and the reason is LSP. `list[int]` cannot be substituted
  for `list[object]` even though every `int` is an `object`.

What the interpreter cannot prevent:

- A `__str__` override that returns wildly different formats across
  instances, breaking log parsing.
- An `__iter__` that yields `None` and then continues, breaking
  consumers using `for x in it: if x is None: break`.
- A `__hash__` override inconsistent with `__eq__`, silently
  corrupting `dict` and `set` lookups.
- A `close()` override that raises a broader exception than the base,
  breaking `try/except` blocks in callers.

Each compiles, imports, ships, and gradually erodes the assumption
that "any subclass is interchangeable". Consumers special-case around
the misbehaving subclass, the base type stops being a clean
abstraction, and removing the special case becomes a breaking change.

## In Python

LSP in Python translates to two related disciplines:

1. **Behavioural contracts on ABCs and Protocols.** Every `abc.ABC`
   and `typing.Protocol` you define has, implicitly or explicitly, a
   contract that implementors must honour. State it in the class
   docstring.
2. **Signature compatibility checked statically.** A type checker
   (`mypy --strict`, `pyright`) enforces method signatures are
   compatible across overrides: covariant return types, contravariant
   parameter types, no narrower exception declarations.

The practical rules:

1. **Document every contract requirement** in the class docstring of
   the ABC or Protocol — preconditions, postconditions, raised
   exceptions, idempotence, thread-safety.
2. **Run a type checker in strict mode** and treat override mismatches
   as errors.
3. **Decorate every override with `@override`** (PEP 698, Python 3.12+)
   so the checker flags typo'd or removed parent methods.
4. **Test subclass conformance against the contract**, not just its
   own happy paths. Provide a `contract_test_suite()` helper.
5. **Prefer `Protocol` over inheritance** when the relationship is
   structural — it makes the contract explicit and avoids accidental
   inheritance of unrelated behaviour. See
   [composition-over-inheritance](composition-over-inheritance.md).

## Violations and remedies

### Anti-pattern: the classic Square/Rectangle

```python
class Rectangle:
    def __init__(self, w: float, h: float) -> None:
        self._w = w
        self._h = h

    def set_width(self, w: float) -> None:
        self._w = w

    def set_height(self, h: float) -> None:
        self._h = h

    def area(self) -> float:
        return self._w * self._h


class Square(Rectangle):
    def set_width(self, w: float) -> None:
        self._w = w
        self._h = w  # surprise: also mutates height

    def set_height(self, h: float) -> None:
        self._w = h
        self._h = h
```

Any function written against `Rectangle` that sets width and height
independently and asserts `area == w * h` will fail for `Square`. The
`is-a` relationship from geometry does not survive in code with
mutation: a mutable square is **not** a mutable rectangle.

### Idiomatic fix: model the invariant, not the inheritance

```python
from typing import Protocol


class Shape(Protocol):
    def area(self) -> float: ...


class Rectangle:
    def __init__(self, w: float, h: float) -> None:
        self.w, self.h = w, h

    def area(self) -> float:
        return self.w * self.h


class Square:
    def __init__(self, side: float) -> None:
        self.side = side

    def area(self) -> float:
        return self.side * self.side
```

Neither inherits from the other. Both satisfy `Shape` structurally.
Each is immutable in its defining dimensions, so no mutation can
violate the invariant.

### Anti-pattern: `list[int]` substituted for `list[object]`

```python
def append_string(xs: list[object]) -> None:
    xs.append("hello")

ints: list[int] = [1, 2, 3]
append_string(ints)        # type error — and rightly so
# ints now contains "hello": runtime corruption
```

`list[int]` is **invariant** in `int`, not covariant. If it were
covariant, the call above would compile, and `ints` would silently
contain a `str`. Mutable containers cannot be covariant — this is a
direct application of LSP to generic types.

### Idiomatic fix: use a read-only protocol or `Sequence`

```python
from collections.abc import Sequence

def sum_all(xs: Sequence[object]) -> int:
    return sum(1 for _ in xs)

ints: list[int] = [1, 2, 3]
sum_all(ints)              # fine — Sequence is covariant in its element
```

`Sequence[T]` is covariant because it does not allow mutation. The
LSP-safe substitution holds.

### Anti-pattern: ABC subclass that broadens exceptions

```python
import abc

class Storage(abc.ABC):
    @abc.abstractmethod
    def get(self, key: str) -> bytes | None:
        """Return value for key, or None if missing.

        Raises:
            OSError: on I/O failure.
        """

class S3Storage(Storage):
    def get(self, key: str) -> bytes | None:
        try:
            return self._client.fetch(key)
        except Exception as e:                       # broadened
            raise RuntimeError("s3 failed") from e   # broader than parent
```

Callers writing `try: storage.get(k); except OSError: retry()` will
not catch `RuntimeError`. The override has narrowed the precondition
(`get` may now raise something outside the documented set) while
appearing to satisfy the signature.

### Idiomatic fix: wrap inside the documented exception type

```python
class S3Storage(Storage):
    def get(self, key: str) -> bytes | None:
        try:
            return self._client.fetch(key)
        except S3ClientError as e:
            raise OSError(f"s3 fetch failed: {e}") from e
```

The override raises only exceptions in the parent's contract. If the
new behaviour genuinely needs a new exception, it belongs in a new
method or a new ABC — not as a silent broadening of the parent's
contract.

### Anti-pattern: override returns `None` where parent returned a value

```python
class Cache:
    def get_or_compute(self, key: str) -> bytes:
        ...

class LoggingCache(Cache):
    def get_or_compute(self, key: str) -> bytes | None:   # broader return
        self._log(key)
        if key in self._evicted:
            return None                                   # surprise
        return super().get_or_compute(key)
```

A caller writing `len(cache.get_or_compute(k))` crashes with
`TypeError: object of type 'NoneType' has no len()` the first time
the subclass is used. The return type widened from `bytes` to
`bytes | None`, which is a contravariant change — only legal on
parameters, not return values. `mypy --strict` flags this.

### Idiomatic fix: keep the return type, signal absence differently

```python
class LoggingCache(Cache):
    @override
    def get_or_compute(self, key: str) -> bytes:
        self._log(key)
        return super().get_or_compute(key)
```

If the subclass really needs to report absence, expose a separate
`try_get(key) -> bytes | None` method. Do not narrow or widen the
parent's contract in place.

### Anti-pattern: missing `@override` causes silent contract drift

```python
class Reader:
    def read_chunk(self, n: int) -> bytes: ...

class GzipReader(Reader):
    def read_chunks(self, n: int) -> bytes:    # typo: plural
        ...
```

`GzipReader.read_chunks` does not override anything. The parent's
`read_chunk` is still inherited (and presumably broken or unimplemented).
Without `@override`, the type checker cannot tell the typo from a
deliberate new method.

### Idiomatic fix: `@override` (PEP 698) on every override

```python
from typing import override

class GzipReader(Reader):
    @override
    def read_chunk(self, n: int) -> bytes:
        ...
```

If the parent does not declare `read_chunk`, the type checker raises
an error. Refactors that rename or remove parent methods now break
loudly.

### Anti-pattern: violating `__hash__` / `__eq__` consistency

```python
class User:
    def __init__(self, uid: int, last_seen: float) -> None:
        self.uid, self.last_seen = uid, last_seen

    def __eq__(self, other: object) -> bool:
        return isinstance(other, User) and self.uid == other.uid

    def __hash__(self) -> int:
        return hash((self.uid, self.last_seen))   # last_seen not in __eq__
```

Two `User` values with the same `uid` and different `last_seen` are
`==` but have different hashes. `dict[User, V]` lookups will sometimes
miss them. This is a direct LSP violation against `object`'s
documented contract: equal values must hash equally.

### Idiomatic fix: dataclass with `frozen=True` and `eq=True`

```python
from dataclasses import dataclass

@dataclass(frozen=True, eq=True)
class User:
    uid: int
    last_seen: float
```

`@dataclass` derives `__eq__` and `__hash__` from the same field set,
guaranteeing consistency. If `last_seen` must not affect equality,
declare it with `field(compare=False)` — and the hash will also exclude
it. See [make-invalid-states-unrepresentable](make-invalid-states-unrepresentable.md).

### Anti-pattern: `__init__` signature diverging from parent contract

```python
class Connection:
    def __init__(self, host: str, port: int) -> None: ...

class TlsConnection(Connection):
    def __init__(self, host: str, port: int, cert: Path) -> None:
        super().__init__(host, port)
        self.cert = cert
```

Factory code like `cls(host, port)` works for `Connection` but raises
`TypeError: missing argument 'cert'` for `TlsConnection`. Subclasses
that add required `__init__` parameters cannot be substituted for the
parent in generic construction contexts.

### Idiomatic fix: default value, factory method, or composition

```python
class TlsConnection(Connection):
    def __init__(self, host: str, port: int, *, cert: Path | None = None) -> None:
        super().__init__(host, port)
        self.cert = cert or default_cert()

    @classmethod
    def with_cert(cls, host: str, port: int, cert: Path) -> "TlsConnection":
        return cls(host, port, cert=cert)
```

Either give the new parameter a default (so `TlsConnection(host, port)`
still works) or expose a named constructor. Better still, prefer
composition: a `Connection` field on `TlsConnection` rather than
inheritance.

## Variance and PEP 695 generics

Python 3.12+ supports the inline generic syntax of PEP 695:

```python
class Container[T]:
    def get(self) -> T: ...

class ReadOnlyContainer[T_co]:   # covariant element
    def get(self) -> T_co: ...
```

The variance of a type parameter follows LSP directly:

- **Covariant** (`T_co`): only appears in *output* positions. A
  `ReadOnlyContainer[Cat]` is a `ReadOnlyContainer[Animal]` — safe
  because nothing of type `Animal` can be inserted to violate the
  `Cat` invariant.
- **Contravariant** (`T_contra`): only in *input* positions. A
  `Sink[Animal]` is a `Sink[Cat]` — safe because anything you pass in
  (a `Cat`) is accepted by an `Animal` sink.
- **Invariant** (default, `T`): both input and output. `list[Cat]` is
  not a `list[Animal]` (you could put a `Dog` in) and not a
  `list[Cat]` (you could read it as `Cat` and get a `Dog`). Mutable
  containers must be invariant.

Get this wrong and the type checker rejects it (good) — get it
right by accident and the runtime will reject it later (bad). Always
declare variance explicitly on `Protocol`s and `Generic`s.

## How code-ranker detects LSP violations

LSP violations are usually invisible to a graph analyzer — they live
in method bodies and runtime behaviour. But code-ranker can flag
*structural risk*:

| Signal | LSP interpretation |
|---|---|
| ABC or Protocol with N implementations and a docstring lacking a "Contract" or "Raises" section | Implementors have no shared contract; each impl will diverge. |
| Override method whose signature narrows the return type or broadens declared exceptions vs the parent | Direct LSP violation; detectable by AST analysis of `@override`-decorated methods. |
| Subclass `__init__` adding required positional parameters not present in parent | Substitutability break for factory call sites. |
| Subclass defining `__hash__` without `__eq__` (or vice versa) | Hash/equality consistency risk. |
| Override missing the `@override` decorator | Silent contract drift on parent renames. Recommend adding `@override`. |

The honest answer is that LSP is mostly a documentation discipline —
code-ranker's contribution is to *flag ABCs and Protocols that have no
contract section* and to *recommend writing one*, plus the easy
syntactic checks above. Behavioural verification belongs in tests.

## Suggested recommendation template

> **LSP risk**: `Storage` (in `app/storage/base.py`) is an `abc.ABC`
> with 6 subclasses across the codebase and no `Raises:` or
> `Contract:` section in its docstring. Without a stated behavioural
> contract, subclass implementations diverge silently and consumers
> special-case around them. Add a contract section documenting
> required invariants and the allowed exception set for every
> abstract method, then add a `contract_test_suite(s: Storage)`
> helper in `app/storage/testing.py` that subclasses call from their
> tests. Decorate every override with `@override` and run
> `mypy --strict` so signature drift fails CI.
>
> References:
>  - <https://peps.python.org/pep-0698/>
>  - <https://peps.python.org/pep-0544/>

## Related principles

- [SRP](solid-single-responsibility.md) — narrow ABCs and Protocols
  are easier to write contracts for than broad ones.
- [ISP](solid-interface-segregation.md) — clients depend on small
  contracts, not large ones; LSP gets easier with each split.
- [Composition over inheritance](composition-over-inheritance.md) —
  most LSP violations vanish when subclassing is replaced with a
  `Protocol` plus delegation.
- [Make invalid states unrepresentable](make-invalid-states-unrepresentable.md)
  — encode contract requirements in types (e.g. a `NonEmpty[T]` newtype
  rather than "must not be empty" in a docstring).

## References

1. Liskov, B. and Wing, J. "A Behavioral Notion of Subtyping". ACM
   TOPLAS 16(6), 1994.
   <https://dl.acm.org/doi/10.1145/197320.197383>
2. Martin, R. C. "The Liskov Substitution Principle". 1996.
   <https://www.labri.fr/perso/clement/enseignements/ao/LSP.pdf>
3. Martin, R. C. *Clean Architecture*. Ch. 9.
4. PEP 544 — Protocols: Structural subtyping.
   <https://peps.python.org/pep-0544/>
5. PEP 695 — Type Parameter Syntax.
   <https://peps.python.org/pep-0695/>
6. PEP 698 — `@override` Decorator for Static Typing.
   <https://peps.python.org/pep-0698/>
7. Python docs, `typing` — Generics, variance, `Self`, `Protocol`.
   <https://docs.python.org/3/library/typing.html>
8. Python docs, `collections.abc` — covariance of read-only
   collections vs invariance of mutable ones.
   <https://docs.python.org/3/library/collections.abc.html>
