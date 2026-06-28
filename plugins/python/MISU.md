# Make Invalid States Unrepresentable (in Python)

**TL;DR**: Move correctness from runtime checks into the type system
and into validated constructors. A `User` cannot have a missing email;
a `Connection` cannot be queried before being opened; a parsed JSON
value cannot also be a parse error. Python's type system is weaker
than Rust's, but with mypy/pyright, `Literal` unions, `NewType`,
`Protocol`, frozen dataclasses, and Pydantic v2 discriminated unions,
you can get most of the way there — and the remaining gap is closed
by validation in `__post_init__` / Pydantic validators that runs
*once* at the boundary.

## Canonical sources

- Yaron Minsky, "Effective ML: Make Illegal States Unrepresentable"
  (2010 Jane Street tech talk). The phrase originates here.
  <https://blog.janestreet.com/effective-ml-revisited/>
- Alexis King, "Parse, don't validate" (2019):
  <https://lexi-lambda.github.io/blog/2019/11/05/parse-don-t-validate/>
- Scott Wlaschin, *Domain Modeling Made Functional* (2018) — the
  canonical book-length treatment, with F# examples that translate
  almost line-for-line into Python with `Literal` + dataclasses.
- Pydantic v2 docs, "Discriminated Unions":
  <https://docs.pydantic.dev/latest/concepts/unions/#discriminated-unions>
- PEP 695 (type parameter syntax), PEP 647 (`TypeGuard`), PEP 692
  (`TypedDict` for `**kwargs`).

## The principle

Two designs of the same feature can differ dramatically in how many
runtime checks they require:

**Design A** (invalid states representable):

```python
from dataclasses import dataclass

@dataclass
class User:
    email: str | None = None        # may be None
    age: int | None = None          # may be None
    role: str = "member"            # any string

def send_birthday_email(u: User) -> None:
    if u.email is None:
        raise RuntimeError("user without email?!")
    if u.age is None:
        raise RuntimeError("user without age?!")
    if u.role in ("admin", "Admin", "ADMIN"):
        # role is a string, so every case must be checked
        ...
```

**Design B** (invalid states *unrepresentable*):

```python
from dataclasses import dataclass
from enum import Enum

class Role(Enum):
    ADMIN = "admin"
    MEMBER = "member"
    GUEST = "guest"

@dataclass(frozen=True, slots=True)
class User:
    email: Email      # validated at construction, never None
    age: Age          # int newtype, guaranteed 0..=150
    role: Role        # closed enum, exhaustively matchable

def send_birthday_email(u: User) -> None:
    email = u.email             # no None
    age = u.age                 # no None, no range check
    match u.role:
        case Role.ADMIN: ...
        case Role.MEMBER: ...
        case Role.GUEST: ...
```

Design A pushes correctness onto every caller. Design B pushes it
into `User`'s construction — once, in one place. After that,
mypy/pyright and the frozen invariant enforce it.

Minsky's principle: **make invalid states syntactically impossible**.
King's reformulation: **parse, don't validate** — convert raw data
into a type that carries the proof of validity, then never re-validate.

## Why it matters

Bugs cluster around "this case shouldn't happen but the code allows
it". Every `assert x is not None`, every `cast(...)`, every defensive
`if isinstance(x, ...)` is an invariant living in the author's head
rather than in the code.

When you encode the invariant in a type or in a validated constructor:

- **The type checker enforces it** — every call site is checked by
  mypy/pyright; `Literal` and `Enum` give exhaustive `match` analysis.
- **The invariant is visible** — readers see `Email` and know it's
  validated, no need to trace back to the constructor.
- **Tests don't have to repeat it** — you don't write 50 tests
  asserting "email is well-formed at every public entry point",
  because every entry point's signature already says so.
- **Refactoring is safe** — extracting code that takes a `User`
  preserves its invariants.

Python's type system is weaker than Rust's: there is no borrow
checker, no affine types, no `#[non_exhaustive]`. But there is
enough to encode most domain invariants — and what the type system
cannot catch, a `__post_init__` or a Pydantic validator catches
*once*, at construction, after which the frozen instance carries
the proof.

## In Python

The Python tools for this principle:

### 1. Tagged unions instead of stringly-typed enums

```python
from dataclasses import dataclass
from typing import Literal

# Bad
@dataclass
class Request:
    method: str                # "GET", "POST", "get", "POSt", etc.
    body: bytes | None = None

# Good — closed Literal union, exhaustively matchable
@dataclass(frozen=True, slots=True)
class Get:
    kind: Literal["get"] = "get"
    url: str = ""

@dataclass(frozen=True, slots=True)
class Post:
    url: str
    body: bytes
    kind: Literal["post"] = "post"

@dataclass(frozen=True, slots=True)
class Delete:
    url: str
    kind: Literal["delete"] = "delete"

Request = Get | Post | Delete

def handle(req: Request) -> None:
    match req:
        case Get(url=u): ...
        case Post(url=u, body=b): ...
        case Delete(url=u): ...
        # mypy/pyright flag missing variants as a type error
```

A `Get` literally has no `body` field, because the dataclass doesn't
declare one. The state "GET with a body" is unrepresentable.

For Pydantic the same pattern is a **discriminated union**:

```python
from pydantic import BaseModel, Field
from typing import Annotated, Literal

class Get(BaseModel):
    kind: Literal["get"]
    url: str

class Post(BaseModel):
    kind: Literal["post"]
    url: str
    body: bytes

Request = Annotated[Get | Post, Field(discriminator="kind")]
```

Pydantic parses the right variant at the boundary and rejects
ill-formed inputs without you writing a single `if`.

### 2. Newtype with a validated constructor

```python
from typing import NewType

UserId = NewType("UserId", int)
# UserId is *just* an int at runtime; mypy treats it as distinct.
```

`NewType` is zero-cost but gives no runtime validation. For domain
types where construction must validate, use a frozen dataclass with
`__post_init__`:

```python
from dataclasses import dataclass
import re

_EMAIL_RE = re.compile(r"^[^@\s]+@[^@\s]+\.[^@\s]+$")

@dataclass(frozen=True, slots=True)
class Email:
    value: str

    def __post_init__(self) -> None:
        if not _EMAIL_RE.match(self.value):
            raise ValueError(f"invalid email: {self.value!r}")
```

You cannot construct an `Email` without going through `__post_init__`.
Once constructed, every downstream consumer can rely on it being
well-formed. No re-validation, no defensive checks. The `frozen=True`
means the invariant cannot be broken by later mutation. (Cross-reference:
[Composition over Inheritance](CoI.md).)

For Pydantic the same idea is a model with a `field_validator`:

```python
from pydantic import BaseModel, field_validator

class Email(BaseModel, frozen=True):
    value: str

    @field_validator("value")
    @classmethod
    def _check(cls, v: str) -> str:
        if not _EMAIL_RE.match(v):
            raise ValueError("invalid email")
        return v
```

### 3. State machines via class hierarchies (poor man's typestate)

```python
from dataclasses import dataclass
from typing import Self

@dataclass(frozen=True, slots=True)
class ClosedConnection:
    dsn: str

    def open(self) -> "OpenConnection":
        socket = _dial(self.dsn)
        return OpenConnection(socket=socket)

@dataclass(frozen=True, slots=True)
class OpenConnection:
    socket: object

    def query(self, sql: str) -> list[tuple[object, ...]]:
        ...

    def close(self) -> ClosedConnection:
        _shutdown(self.socket)
        return ClosedConnection(dsn="...")
```

`ClosedConnection.query` does not exist. mypy rejects `query` on a
closed connection. The state machine is encoded in *which class*
exposes *which method*, not in `if self.is_open: ... else: raise`.

This is weaker than Rust's typestate (the caller can keep using the
old `ClosedConnection` reference after `open()`), but `frozen=True`
plus returning a *new* instance is the idiomatic Python approximation.

### 4. `Literal` and `Annotated` for invariant-bearing scalars

```python
from typing import Annotated, Literal
from pydantic import Field

PositiveInt = Annotated[int, Field(gt=0)]
NonEmptyStr = Annotated[str, Field(min_length=1)]

def allocate(count: PositiveInt) -> list[Slot]: ...
```

`allocate(0)` is rejected by Pydantic at the boundary. The function
body need not check.

For pure typing (no Pydantic), `Literal[1, 2, 3]` closes the set:

```python
LogLevel = Literal["debug", "info", "warn", "error"]
def log(level: LogLevel, msg: str) -> None: ...

log("inof", "...")   # mypy error: not a LogLevel
```

### 5. Smart enums replacing booleans

```python
from enum import Enum

# Bad
def save(record: Record, force: bool) -> None: ...
# What does `force=True` mean?  When?

# Good
class SaveBehaviour(Enum):
    ERROR_IF_EXISTS = "error_if_exists"
    OVERWRITE_IF_EXISTS = "overwrite_if_exists"

def save(record: Record, behaviour: SaveBehaviour) -> None: ...
```

Call sites become self-documenting:
`save(r, SaveBehaviour.OVERWRITE_IF_EXISTS)` versus `save(r, True)`.

### 6. `Final` and `frozen=True` for invariants the compiler can lock down

```python
from typing import Final

MAX_RETRIES: Final = 5             # mypy forbids reassignment

@dataclass(frozen=True, slots=True)
class Config:
    timeout_s: float
    region: str
    # frozen=True forbids attribute reassignment at runtime
```

`Final` is Python's closest analogue to "this invariant cannot drift
after initialization".

### 7. `Protocol` for structural contracts

```python
from typing import Protocol

class SupportsClose(Protocol):
    def close(self) -> None: ...

def with_resource(r: SupportsClose) -> None: ...
```

A `Protocol` says "anything with this shape" — useful when an
invariant is "must support `close`" without forcing inheritance.

## Violations and remedies

### Anti-pattern: `T | None` for required fields

```python
from dataclasses import dataclass

@dataclass
class OrderRequest:
    customer_id: CustomerId | None = None   # required, but Optional "for easier deserialization"
    items: list[Item] | None = None         # required
    total: Money | None = None              # required

def process(req: OrderRequest) -> None:
    if req.customer_id is None: raise Error("missing customer")
    if req.items is None:       raise Error("missing items")
    if req.total is None:       raise Error("missing total")
    ...
```

Every consumer must check for `None`. The `OrderRequest` dataclass is
semantically "an order, but maybe not really".

### Idiomatic fix: required fields, a separate wire-level model

```python
from pydantic import BaseModel
from dataclasses import dataclass

# Wire-level (deserialization target) — fields may be missing
class OrderRequestWire(BaseModel):
    customer_id: CustomerId | None = None
    items: list[Item] | None = None
    total: Money | None = None

    def into_domain(self) -> "OrderRequest":
        if self.customer_id is None: raise RequestError("missing customer")
        if self.items is None:       raise RequestError("missing items")
        if self.total is None:       raise RequestError("missing total")
        return OrderRequest(
            customer_id=self.customer_id,
            items=self.items,
            total=self.total,
        )

# Domain-level (validated) — every field present
@dataclass(frozen=True, slots=True)
class OrderRequest:
    customer_id: CustomerId
    items: list[Item]
    total: Money
```

Validation happens once at the wire boundary. After that, `OrderRequest`
has no `None` anywhere, and every downstream function can rely on the
fields being present.

This is King's "parse, don't validate" applied at the API boundary.
With Pydantic v2 you can often skip the manual split: declare required
fields without defaults on the wire model and Pydantic will reject
missing fields at parse time.

### Anti-pattern: state encoded in a flag

```python
@dataclass
class Connection:
    socket: object
    is_open: bool = False

    def query(self, sql: str) -> list[tuple]:
        if not self.is_open:
            raise ConnectionError("closed")
        ...

    def close(self) -> None:
        self.is_open = False
```

Every method needs the `is_open` check. The type checker cannot help.

### Idiomatic fix: separate classes per state

```python
@dataclass(frozen=True, slots=True)
class ClosedConnection:
    dsn: str
    def open(self) -> "OpenConnection": ...

@dataclass(frozen=True, slots=True)
class OpenConnection:
    socket: object
    def query(self, sql: str) -> list[tuple]: ...
    def close(self) -> ClosedConnection: ...
```

`query` on a `ClosedConnection` does not type-check.

### Anti-pattern: parallel lists that must stay in sync

```python
@dataclass
class Catalog:
    names: list[str]
    prices: list[Money]
    in_stock: list[bool]
```

The invariant "lengths are equal" is unstated. A bug that appends to
two lists but not the third desynchronizes silently.

### Idiomatic fix: one dataclass per row

```python
@dataclass(frozen=True, slots=True)
class CatalogItem:
    name: str
    price: Money
    in_stock: bool

@dataclass(frozen=True, slots=True)
class Catalog:
    items: tuple[CatalogItem, ...]
```

The invariant is built in: there is exactly one of each field per
item.

### Anti-pattern: `str` for "kind-of typed" identifiers (primitive obsession)

```python
def deactivate(user_id: str, by: str) -> None: ...
```

`deactivate(by_admin, user_id)` (arguments swapped) type-checks and
ships. Production bug.

### Idiomatic fix: `NewType` per identifier

```python
from typing import NewType
from uuid import UUID

UserId  = NewType("UserId",  UUID)
AdminId = NewType("AdminId", UUID)

def deactivate(user: UserId, by: AdminId) -> None: ...
```

Swapping arguments is a mypy/pyright error. Runtime cost: zero.

### Anti-pattern: `dict[str, Any]` flowing through call chains

```python
def handle_event(evt: dict[str, Any]) -> None:
    user_id = evt["user_id"]          # KeyError waiting to happen
    payload = evt.get("payload", {})  # any shape
    forward(payload)                  # what does `forward` expect?
```

A `dict[str, Any]` is the Python equivalent of the stringly-typed
catch-all: it tells the reader nothing, and the type checker checks
nothing.

### Idiomatic fix: a parsed model at the boundary

```python
from pydantic import BaseModel

class Event(BaseModel):
    user_id: UserId
    payload: Payload

def handle_event(raw: dict[str, Any]) -> None:
    evt = Event.model_validate(raw)   # parse once, at the boundary
    forward(evt.payload)              # forward takes a Payload, not Any
```

### Anti-pattern: builder that allows `build()` on incomplete state

```python
@dataclass
class UserBuilder:
    email: str | None = None
    age:   int | None = None

    def with_email(self, e: str) -> "UserBuilder":
        self.email = e
        return self

    def with_age(self, a: int) -> "UserBuilder":
        self.age = a
        return self

    def build(self) -> User:
        if self.email is None: raise BuildError("missing email")
        if self.age is None:   raise BuildError("missing age")
        return User(email=Email(self.email), age=Age(self.age))
```

Forgetting `.with_email(...)` is caught at runtime.

### Idiomatic fix: require the fields in `__init__` (no builder)

Python's keyword arguments + `frozen=True` dataclasses are usually a
better fit than a builder pattern: the constructor *is* the typestate
endpoint. If you genuinely need staged construction (for example,
because some fields are async-loaded), encode the stages as separate
classes:

```python
@dataclass(frozen=True, slots=True)
class UserDraftEmail:
    email: Email
    def with_age(self, age: Age) -> "UserDraftEmailAge":
        return UserDraftEmailAge(email=self.email, age=age)

@dataclass(frozen=True, slots=True)
class UserDraftEmailAge:
    email: Email
    age: Age
    def build(self) -> User:
        return User(email=self.email, age=self.age)
```

`build()` only exists on `UserDraftEmailAge`. Forgetting a step is a
type error. (See [OCP](OCP.md) for the trade-off:
adding a new required field is breaking — reserve staged builders
for genuinely required fields.)

## When NOT to use this principle

Encoding *every* invariant in types becomes counter-productive:

- **Runtime cost** — `__post_init__` validation, Pydantic models, and
  immutable container conversion (`tuple` for `list`) all have cost.
- **API ergonomics** — staged builders and many `NewType` aliases can
  make call sites verbose.
- **Type-checker friction** — Pydantic + dataclasses + Protocols can
  produce inscrutable mypy errors; pyright is usually friendlier.
- **Diminishing returns** — sometimes a single `assert` at the entry
  point is genuinely cheaper than a domain type.

A pragmatic heuristic: encode invariants that **multiple consumers**
need. A single-use invariant ("this helper takes a list with an even
number of elements") may be cheaper as an `assert len(xs) % 2 == 0`
at the top of the function.

## How code-ranker detects representable-invalid-state risk

Code Ranker's static graph cannot directly read invariants. It can flag
*structural risk*:

| Signal | Interpretation |
|---|---|
| Functions with many `assert x is not None` / `cast(...)` on parameters | Signals invariants in the head of the author, not in the types. AST rule. |
| Public dataclass / `BaseModel` with many `T \| None` fields | Possibly invalid-state-representable. Check whether construction goes through a `parse`/`model_validate`-style boundary. |
| `str`-typed identifiers across many call sites | `NewType` candidates. Detectable from AST. |
| Functions taking same-type positional arguments | Swapping risk. AST analysis. |
| `dict[str, Any]` parameter or return type flowing through call chains | "Stringly typed" smell. Candidate for a Pydantic model at the boundary. |
| `status: str` parameters with a small closed set of accepted values | `Literal[...]` or `Enum` candidate. |

Code Ranker's current rule set does not catch these directly. The
**LLM-verification** prompt mode (see
`cpt-code-ranker-fr-prompt-composer`) can ask an LLM reading the code
to flag these patterns.

## Suggested recommendation template

> **Make-Invalid-States-Unrepresentable candidate**: dataclass
> `OrderRequest` has 5 `T | None` fields, all of which downstream
> code asserts non-None. This is a "parse, don't validate" candidate:
> split `OrderRequest` into `OrderRequestWire` (Pydantic, all
> `Optional`) and `OrderRequest` (frozen dataclass, all required),
> with a single `into_domain` / `model_validate` parse-step at the
> boundary.
>
> Source: King, "Parse, don't validate" (2019); Minsky, "Effective
> ML" (2010); Wlaschin, *Domain Modeling Made Functional* (2018).

## Related principles

- [LSP](LSP.md) — types that encode
  invariants make LSP contracts implicit (no docstring needed for
  "email must be valid" — the type says so).
- [Composition over Inheritance](CoI.md) —
  the `NewType` and frozen-dataclass-wrapper pattern is the Python
  workhorse for this principle.
- [KISS](KISS.md) — encoding too many invariants in types (deeply
  nested `Annotated`, staged builders, Protocol hierarchies) can
  violate KISS. Pick your battles.
- [SRP](SRP.md) — a domain type's single
  responsibility is to carry its invariant; mixing wire-format
  concerns into it violates SRP.

## References

1. Minsky, Y. "Effective ML". Jane Street tech talk, 2010.
   <https://blog.janestreet.com/effective-ml-revisited/>
2. King, A. "Parse, don't validate". 2019.
   <https://lexi-lambda.github.io/blog/2019/11/05/parse-don-t-validate/>
3. Wlaschin, S. *Domain Modeling Made Functional*. Pragmatic
   Bookshelf, 2018.
4. Pydantic v2 documentation, "Discriminated Unions" and
   "Validators". <https://docs.pydantic.dev/latest/>
5. PEP 695 — Type Parameter Syntax (Python 3.12+).
   <https://peps.python.org/pep-0695/>
6. PEP 647 — User-Defined Type Guards.
   <https://peps.python.org/pep-0647/>
7. Python `typing` module: `NewType`, `Literal`, `Protocol`, `Final`,
   `Annotated`. <https://docs.python.org/3/library/typing.html>
