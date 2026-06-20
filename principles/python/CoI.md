# Composition Over Inheritance (in Python)

**TL;DR**: Build behaviour by composing small objects that satisfy
focused `Protocol`s, not by extending a chain of base classes. Python
*does* support inheritance (single and multiple), so unlike Rust the
language will not stop you — but decades of `MyBaseManagerMixinView`
hierarchies have shown what happens when you take the language up
on its offer. Reach for inheritance only for tight, same-package
framework hooks or sealed sum-type hierarchies; everywhere else,
compose.

## Canonical sources

- *Design Patterns: Elements of Reusable Object-Oriented Software*
  (Gamma, Helm, Johnson, Vlissides, 1994): "Favor object composition
  over class inheritance."
- Allen Holub, "Why extends is evil" (2003):
  <https://www.infoworld.com/article/2073649/why-extends-is-evil.html>
- Raymond Hettinger, "Python's super considered super!":
  <https://rhettinger.wordpress.com/2011/05/26/super-considered-super/>
- Hynek Schlawack, "Subclassing in Python redux":
  <https://hynek.me/articles/python-subclassing-redux/>
- Brandon Rhodes, "The Composition Over Inheritance principle":
  <https://python-patterns.guide/gang-of-four/composition-over-inheritance/>
- *Fluent Python* (Luciano Ramalho), 2nd ed., Ch. 14 ("Inheritance:
  For Better or for Worse").
- PEP 544 — Protocols: Structural subtyping (static duck typing).
- PEP 695 — Type Parameter Syntax (generic `class Foo[T]`).
- PEP 698 — `typing.override`.

## The principle

In class-based OOP, `class Truck(Vehicle):` makes `Truck` reuse
`Vehicle`'s code by inheriting its members. Decades of experience
showed several systemic problems:

1. **Fragile base class**: changing `Vehicle.__init__` or any
   protected method may silently break every subclass — including
   ones in other packages you have never heard of.
2. **Banana–monkey–jungle problem**: inheriting from `Vehicle` drags
   in every transitive concern (logging, persistence, ORM hooks,
   signals, `Meta` inner-class magic) even when only one method is
   needed. Django's `Model`, DRF's `GenericAPIView`, and SQLAlchemy's
   `Base` are the canonical Python examples.
3. **MRO surprises and the diamond problem**: Python *does* allow
   multiple inheritance and resolves it with C3 linearisation. Add
   one mixin to the wrong place and `super().__init__(...)` starts
   calling a sibling's `__init__` you did not know existed.
4. **Behaviour reuse coupled to identity reuse**: `class Truck(Vehicle)`
   says "Truck *is a* Vehicle" — an `isinstance` claim, a Liskov
   contract, a `__subclasscheck__` hook — when all you wanted was
   "Truck *has* engine code I wanted to reuse".

The Gang of Four prescription, repeated by every subsequent OO
authority: **prefer composition** (object holds another object) over
inheritance (class extends class). Python lets you choose. The
default should be: compose; declare structural contracts with
`Protocol`; reach for inheritance only when you genuinely need the
framework hook or the sealed hierarchy.

## Why it matters

Python's `class` keyword is cheap, and once-helpful frameworks
have trained generations to express *any* reuse as a base class.
The result, observable in any 5-year-old Django or Airflow codebase,
is the **mixin pile-up**:

```python
class UserListView(
    LoginRequiredMixin,
    PermissionRequiredMixin,
    AuditLogMixin,
    PaginatedMixin,
    CachedMixin,
    JsonResponseMixin,
    ListView,
):
    ...
```

The behaviour of `dispatch()` is now distributed across seven
classes, ordered by MRO, each calling `super().dispatch(...)` and
trusting the next link in the chain to exist. Adding one mixin
in the wrong slot silently changes the auth check order. This is
the modern shape of "fragile base class".

Done well, composition gives you:

- **Mix-and-match**: a class can satisfy any combination of
  `Protocol`s without being forced into a single ancestor.
- **Replaceable parts**: each composed dependency is a constructor
  argument, swappable in tests with a fake that just satisfies the
  protocol.
- **Testability**: each component is a unit; mocks are scoped to
  the protocol they implement.
- **Explicit dependencies**: every relationship is visible in the
  `__init__` signature or dataclass fields — no hidden inheritance
  chain to grep through.

Done badly (god-classes-as-bags-of-parts, indirection-for-its-own-sake),
composition becomes a Russian-doll of forwarding methods. The skill
is composing **at the right grain**.

## In Python

Python gives you several composition mechanisms. They are not
alternatives — they layer.

### 1. Structural typing via `Protocol`

The Python equivalent of "trait bound on a generic": ask for the
shape, not the lineage.

```python
from typing import Protocol

class SupportsWrite(Protocol):
    def write(self, data: bytes, /) -> int: ...

def report(writer: SupportsWrite, data: bytes) -> None:
    writer.write(data)
```

`report` does not care whether `writer` is a `BytesIO`, a `socket.socket`
wrapper, or a `gzip.GzipFile`. Any object with a matching `write` method
satisfies the protocol — no `class MyWriter(SupportsWrite)` declaration
needed (though `@runtime_checkable` and explicit inheritance are also
allowed when useful).

This is the workhorse: prefer `Protocol` over `ABC` for "the caller
needs this shape" contracts. See
[DIP](DIP.md).

### 2. ABCs for sealed hierarchies and shared default behaviour

`Protocol` is for structural duck typing. `abc.ABC` is for **nominal**
"I am one of *these* things" hierarchies — the Python form of a
sum type.

```python
from abc import ABC, abstractmethod
from dataclasses import dataclass

class Shape(ABC):
    @abstractmethod
    def area(self) -> float: ...

@dataclass(frozen=True)
class Circle(Shape):
    radius: float
    def area(self) -> float: return 3.14159 * self.radius ** 2

@dataclass(frozen=True)
class Square(Shape):
    side: float
    def area(self) -> float: return self.side ** 2
```

Here inheritance is legitimate: a closed set of variants, defined
together, dispatched on by callers. (If you can use
`match` + structural patterns, even better — see
[Make Invalid States Unrepresentable](MISU.md).)

ABCs *may* carry default implementations, but if you find yourself
overriding more than you keep, the ABC is doing the job of an
interface and should probably be a `Protocol`.

### 3. Dataclasses + composition (the workhorse pattern)

```python
from dataclasses import dataclass, field

@dataclass
class ConnectionPool:
    inner: Pool
    metrics: MetricsCollector
    retry_policy: RetryPolicy = field(default_factory=RetryPolicy.default)
```

`ConnectionPool` *has* a `Pool`, a `MetricsCollector`, a
`RetryPolicy`. Each field is independently testable; each can be
swapped in `__init__`. No base class, no MRO, no `super()`.

This is the default. If you cannot articulate a reason your new
class should inherit, this is the shape you want.

### 4. `NewType` and wrapper dataclasses (Python's "newtype")

```python
from typing import NewType
from uuid import UUID, uuid4

UserId = NewType("UserId", UUID)
OrderId = NewType("OrderId", UUID)

def deactivate(user: UserId, by: AdminId) -> None: ...
```

`NewType` is the cheap form: zero runtime cost, type checker
distinguishes `UserId` from `OrderId`, but at runtime it's just a
`UUID`. Useful when you only want **type-level separation**.

When you want **runtime invariants** (validation, normalisation,
custom methods), use a frozen dataclass with a classmethod
constructor:

```python
@dataclass(frozen=True, slots=True)
class Email:
    value: str

    @classmethod
    def parse(cls, raw: str) -> "Email":
        if "@" not in raw:
            raise ValueError(f"not an email: {raw!r}")
        return cls(raw.strip().lower())
```

This composes a `str` without inheriting its methods (so you cannot
accidentally pass an `Email` where a free-form `str` was wanted, and
you cannot accidentally pass a free-form `str` where an `Email` was
required). See [Make Invalid States Unrepresentable](MISU.md).

### 5. Delegation with `__getattr__` (use sparingly)

```python
class VerboseFile:
    def __init__(self, inner: io.IOBase) -> None:
        self._inner = inner
    def write(self, data: bytes) -> int:
        print(f"writing {len(data)} bytes")
        return self._inner.write(data)
    def __getattr__(self, name: str):
        return getattr(self._inner, name)
```

`VerboseFile` adds behaviour to one method and forwards the rest.
Equivalent to Rust's `Deref` trick — and equally dangerous when
overused: type checkers cannot see through `__getattr__`, and
readers cannot tell which methods are wrapped vs forwarded. Prefer
explicit forwarding of just the methods you actually expose.

### 6. `__init_subclass__` for opt-in framework hooks

When you genuinely want to register subclasses (plugin systems,
serialisable variants), `__init_subclass__` keeps the magic in
one place:

```python
class Plugin:
    registry: dict[str, type["Plugin"]] = {}

    def __init_subclass__(cls, *, name: str, **kwargs) -> None:
        super().__init_subclass__(**kwargs)
        Plugin.registry[name] = cls

class JsonPlugin(Plugin, name="json"):
    ...
```

Prefer this over metaclasses for registration. Prefer an explicit
`@register("json")` decorator over both, unless the framework
contract demands subclassing.

### 7. `@override` for safety in the inheritance you *do* keep

```python
from typing import override

class Base:
    def handle(self, event: Event) -> None: ...

class Concrete(Base):
    @override
    def handle(self, event: Event) -> None: ...
```

When inheritance is justified, `@override` (PEP 698) makes the
intent explicit and lets the type checker catch renames in the
base. Use it on *every* override.

## Violations and remedies

### Anti-pattern: deep inheritance chain mining for reuse

```python
class Animal:
    def __init__(self, name: str) -> None: self.name = name
    def speak(self) -> str: raise NotImplementedError

class Mammal(Animal):
    def __init__(self, name: str, fur_colour: str) -> None:
        super().__init__(name)
        self.fur_colour = fur_colour

class Dog(Mammal):
    def __init__(self, name: str, fur_colour: str, breed: str) -> None:
        super().__init__(name, fur_colour)
        self.breed = breed
    def speak(self) -> str: return "woof"
```

Three levels deep, every `__init__` threading arguments through
`super()`. Add a fourth class and one parameter, and every
constructor in the chain changes.

### Idiomatic fix: compose the data, dispatch via Protocol

```python
@dataclass(frozen=True)
class Vitals:
    name: str
    age_months: int

@dataclass(frozen=True)
class Dog:
    vitals: Vitals
    breed: str
    def speak(self) -> str: return "woof"

@dataclass(frozen=True)
class Cat:
    vitals: Vitals
    indoor: bool
    def speak(self) -> str: return "meow"

class Speaks(Protocol):
    def speak(self) -> str: ...
```

`Vitals` is a composable component, not an ancestor. `Speaks` is the
contract callers actually need. No MRO, no `super()`.

### Anti-pattern: mixin pile-up

```python
class UserView(
    LoginRequiredMixin, PermissionMixin, AuditMixin,
    PaginationMixin, CacheMixin, JsonMixin, ListView,
): ...
```

Every mixin's `dispatch` (or `get_queryset`, or `get_context_data`)
calls `super()` and trusts the chain. The order in the bases tuple
silently determines auth precedence. Renaming a method in one mixin
can shadow another via MRO.

### Idiomatic fix: extract collaborators, compose them

```python
@dataclass
class UserView:
    auth: AuthPolicy            # protocol
    audit: AuditSink            # protocol
    paginator: Paginator        # protocol
    cache: ResponseCache        # protocol
    renderer: JsonRenderer      # protocol

    def handle(self, request: Request) -> Response:
        self.auth.require(request)
        cached = self.cache.get(request)
        if cached is not None: return cached
        result = self.paginator.paginate(self.query(request))
        self.audit.record(request, result)
        return self.renderer.render(result)
```

Each collaborator is a constructor argument, mockable in a one-line
fake, reorderable explicitly in `handle`.

### Anti-pattern: god ABC hiding the inheritance instinct

```python
class Repository(ABC):
    @abstractmethod
    def find(self, id: Id) -> Entity | None: ...
    @abstractmethod
    def save(self, e: Entity) -> None: ...
    @abstractmethod
    def delete(self, id: Id) -> None: ...
    @abstractmethod
    def count(self) -> int: ...
    @abstractmethod
    def list_paginated(self, page: Page) -> list[Entity]: ...
    @abstractmethod
    def migrate(self) -> None: ...
    @abstractmethod
    def dump(self) -> bytes: ...
    @abstractmethod
    def restore(self, b: bytes) -> None: ...
```

You wanted "every repository should have all these". Same anti-pattern
as Java's `BaseRepository`, in a Python hat. Concrete implementations
will end up with `raise NotImplementedError` for half the methods.

### Idiomatic fix: small protocols (ISP)

```python
class Find(Protocol):
    def find(self, id: Id) -> Entity | None: ...
class Save(Protocol):
    def save(self, e: Entity) -> None: ...
class Delete(Protocol):
    def delete(self, id: Id) -> None: ...
```

A concrete repository satisfies the subset it supports. A consumer
asks for the subset it needs. See [ISP](ISP.md).

### Anti-pattern: indirection-as-composition (Russian-doll services)

```python
class UserService:
    def __init__(self) -> None: self.inner = InnerUserService()
class InnerUserService:
    def __init__(self) -> None: self.actually = ActuallyUserService()
class ActuallyUserService:
    def __init__(self) -> None: self.impl = ImplUserService()
```

Composition turned into inheritance by other means. Each layer adds
indirection without adding capability. Collapse.

### Anti-pattern: `class` for what should be a function

```python
class EmailSender:
    def __init__(self, smtp: SmtpClient) -> None: self.smtp = smtp
    def send(self, to: str, body: str) -> None: self.smtp.send(to, body)
```

If the class has one method and one dependency, it is a closure with
extra steps. Either inline it, or make it a function that takes the
dependency:

```python
def send_email(smtp: SmtpClient, to: str, body: str) -> None:
    smtp.send(to, body)
```

See [KISS](KISS.md).

## When inheritance IS appropriate

Composition is the default, not the rule. Use inheritance when:

1. **Same-package framework hooks.** You own both the base and the
   subclass, they ship in the same release, and the base class
   contract is documented for extension. `enum.Enum`, `pathlib.PurePath`,
   `pytest` plugin base classes.
2. **Sealed sum types via ABC.** A small, closed set of variants
   defined alongside their ABC, matched on by callers. (PEP 695's
   generic syntax and `match` statements make this cleaner.)
3. **Exception hierarchies.** Python's `except` clause is nominal;
   `class MyValidationError(ValueError)` is the *only* way to make
   `except ValueError` catch it.
4. **Reusing concrete library scaffolding** where the framework
   explicitly says "subclass us": `unittest.TestCase`,
   `collections.abc.Mapping`, `typing.NamedTuple`.

If none of these apply, reach for `Protocol` + dataclass composition.

## How code-ranker detects composition issues

The graph signals adapted for Python:

| Signal | Composition interpretation |
|---|---|
| Inheritance depth > 3 (MRO length > 4 incl. `object`) | Fragile-base-class risk; flatten or extract collaborators |
| Class with > 3 base classes (excluding `object`) | Mixin pile-up; convert mixins to composed collaborators |
| ABC with many abstract methods AND many implementations | ISP candidate — split into protocols |
| ABC with many abstract methods AND one implementation | KISS / YAGNI candidate; the abstraction has one user |
| Class whose only field is another instance of a similar class | Indirection without composition — flatten |
| Multiple `str`/`int`/`UUID` identifiers passed positionally | `NewType` candidates |
| Wrapper dataclass with no `parse`/`from_*` classmethod | Newtype with broken encapsulation |
| High fan-in on a base class (many subclasses across packages) | Fragile base class; any change ripples |
| `super().__init__(*args, **kwargs)` with opaque kwargs | Cooperative-multiple-inheritance smell |

Code Ranker's `god-module-coupling` and `high-fan-in-public-api` rules
indirectly capture the "fat ABC" and "fragile base" issues. Future
Python-specific rules could flag:

- MRO depth > N.
- Number of mixins (bases not directly inherited from `object`) > N.
- ABCs with > N abstract methods AND > M implementations.
- Functions taking same-type identifiers without `NewType`.

## Suggested recommendation template

> **Composition candidate**: `UserListView` inherits from 7 mixins
> plus `ListView`, giving an MRO of length 10. `dispatch` is
> distributed across `LoginRequiredMixin`, `PermissionRequiredMixin`,
> and `AuditLogMixin` via cooperative `super()` calls; reordering
> the bases tuple silently changes auth precedence.
>
> Extract `AuthPolicy`, `AuditSink`, `Paginator`, `ResponseCache`,
> and `JsonRenderer` as `Protocol`s, hold them as dataclass fields,
> and call them explicitly in `handle`. Inheritance reduces to
> `ListView` (framework hook, same package).
>
> Source: Gang of Four (1994); Hettinger, "super considered super!";
> Rhodes, "Composition Over Inheritance".

## Related principles

- [ISP](ISP.md) — small protocols are the
  Python-flavoured "favor composition" principle for interfaces.
- [DIP](DIP.md) — composition is what makes
  DIP cheap; depend on protocols, inject collaborators.
- [LSP](LSP.md) — when you *do* inherit,
  LSP is the contract you owe subclasses' callers.
- [OCP](OCP.md) — composition is the modern way to
  be "open for extension"; inheritance is the 1988 way.
- [Make Invalid States Unrepresentable](MISU.md)
  — wrapper dataclasses and `NewType` are the workhorses for this.
- [SRP](SRP.md) — each composed piece has
  one responsibility.
- [KISS](KISS.md) — a one-method class is a function in disguise.
- [Law of Demeter](LoD.md) — composition without
  forwarding discipline becomes `a.b.c.d.do_thing()`.

## References

1. Gamma, E., Helm, R., Johnson, R., Vlissides, J. *Design Patterns*.
   1994, p.20.
2. Holub, A. "Why extends is evil". *InfoWorld*, 2003.
   <https://www.infoworld.com/article/2073649/why-extends-is-evil.html>
3. Hettinger, R. "Python's super considered super!", 2011.
   <https://rhettinger.wordpress.com/2011/05/26/super-considered-super/>
4. Rhodes, B. "The Composition Over Inheritance principle".
   <https://python-patterns.guide/gang-of-four/composition-over-inheritance/>
5. Schlawack, H. "Subclassing in Python redux".
   <https://hynek.me/articles/python-subclassing-redux/>
6. Ramalho, L. *Fluent Python*, 2nd ed., O'Reilly, 2022, Ch. 14.
7. PEP 544 — Protocols: Structural subtyping.
   <https://peps.python.org/pep-0544/>
8. PEP 695 — Type Parameter Syntax.
   <https://peps.python.org/pep-0695/>
9. PEP 698 — `typing.override`.
   <https://peps.python.org/pep-0698/>
