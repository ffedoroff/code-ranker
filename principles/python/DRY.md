# DRY — Don't Repeat Yourself (in Python)

**TL;DR**: Every piece of knowledge must have a single, unambiguous,
authoritative representation within a system. DRY is about **knowledge
duplication**, not **code duplication** — copy-pasted lines that
encode different decisions are not DRY violations; one literal in two
different modules that means "the maximum retry count" is.

## Canonical sources

- Andy Hunt and Dave Thomas, *The Pragmatic Programmer* (1999,
  Addison-Wesley): the source of the principle's name. Topic 9 in
  the 20th-anniversary edition: <https://pragprog.com/titles/tpp20/>
- Jeff Atwood citing Hunt, "DRY is About Knowledge" (2014):
  <https://blog.codinghorror.com/dry-not-just-about-code/>
- matklad, "Three Levels of Repetition" (2024):
  <https://matklad.github.io/2024/02/02/three-levels-of-repetition.html>
- Dan Abramov, "The WET Codebase":
  <https://overreacted.io/the-wet-codebase/> (counterpoint:
  premature DRY is worse than duplication)
- Sandi Metz, "The Wrong Abstraction" (2016):
  <https://sandimetz.com/blog/2016/1/20/the-wrong-abstraction>
- Raymond Hettinger, "It's not duck typing; it's structural typing"
  (PyCon talks, *passim*) — the Python angle on shared shape vs
  shared meaning.
- *The Zen of Python* (PEP 20): "There should be one — and preferably
  only one — obvious way to do it." DRY's spiritual cousin in Python
  culture.

## The principle

The Pragmatic Programmer text:

> Every piece of knowledge must have a single, unambiguous,
> authoritative representation within a system.

The misreading the authors regret most: DRY is not "don't write the
same characters twice". It is "don't encode the same **decision** in
two places where they can drift apart".

Hunt later clarified: if two pieces of code happen to look identical
**because the underlying concept happens to coincide right now**, that
is not a DRY violation. It is *accidental duplication*. Extracting it
into a shared abstraction creates a worse problem — you have welded
two concepts together that are free to diverge later, and the
abstraction will fight every change.

Real DRY violations are about **knowledge**: a constant, a regex, a
business rule, a calculation, a schema. When the regulation says
"customers under 18 cannot purchase alcohol", the number `18` should
appear in exactly one place in your code.

## Why it matters

When the same knowledge lives in N places:

- Updates require finding all N. You will miss some.
- Tests may pass on the locations you remembered and silently fail
  in production for the ones you forgot.
- Reviewers cannot tell whether N differences are intentional or are
  drift.
- Onboarding becomes harder: "Where is the truth about X?" has N
  answers.

When *accidental* duplication is force-extracted (the "wrong
abstraction" failure mode), N use sites are forced to evolve together
when they actually need to diverge. The abstraction grows boolean
flags, special cases, and conditionals until it is harder to read
than the original duplication.

Python amplifies both directions. Its dynamism makes it cheap to
extract a one-line helper — and equally cheap to grow it into a
fourteen-keyword-argument monster that no caller fully understands.
The skill is distinguishing knowledge duplication (which DRY targets)
from accidental similarity (which DRY does not).

## In Python

Python has several mechanisms that make true DRY clean and several
that make false DRY tempting. Use the first set; resist the second.

### Mechanisms for genuine DRY

**Module-level constants**:

```python
# domain/limits.py
from datetime import timedelta
from typing import Final

MIN_ALCOHOL_AGE: Final[int] = 18
MAX_USERNAME_LEN: Final[int] = 64
PASSWORD_RESET_TTL: Final[timedelta] = timedelta(minutes=15)
```

`Final` documents intent; a typo (`MIN_ALCOOL_AGE`) becomes an
`AttributeError` at import time rather than a silent miscompare.

**Functions that name a calculation**:

```python
def effective_tax_rate(subtotal: Money, jurisdiction: Jurisdiction) -> Rate:
    return base_rate(jurisdiction) + surcharge_for(subtotal)
```

The formula has one expression. If the regulation changes, you
change one place.

**PEP 695 generics for true polymorphism**:

```python
from uuid import UUID

def parse_id[T: UUID](s: str, factory: type[T]) -> T:
    return factory(s)
```

Used to derive `UserId`, `OrderId`, `TransactionId` from the same
parsing logic — *which is genuinely the same knowledge*.

**Protocols for structural shared contracts**:

```python
from typing import Protocol, runtime_checkable

@runtime_checkable
class HasId(Protocol):
    id: UUID

def log_entity(e: HasId) -> None:
    print(f"entity {e.id}")
```

A `Protocol` codifies "anything with an `id` field" once, without
forcing inheritance. Compare to defining an ABC and forcing every
caller to subclass.

**Dataclasses and Pydantic models for schema knowledge**:

```python
from dataclasses import dataclass
from datetime import datetime

@dataclass(frozen=True, slots=True)
class UserRow:
    id: UUID
    email: str
    name: str
    created_at: datetime
    deleted_at: datetime | None
```

The column shape lives once. Repositories project from `UserRow`;
serializers project to JSON from `UserRow`. Adding a column is one
field.

**Decorators for cross-cutting concerns**:

```python
from functools import cache, wraps

@cache
def jurisdiction_rate(code: str) -> Rate: ...
```

`@cache`, `@dataclass`, `@override`, `@property` codify "every type
gets these capabilities" once in the standard library. You do not
re-implement memoisation per call site.

**`type` statements for shared aliases**:

```python
type ConfigResult[T] = T | ConfigError
type JsonValue = None | bool | int | float | str | list["JsonValue"] | dict[str, "JsonValue"]
```

The fact that "config operations return `T | ConfigError`" appears
once.

**Exception hierarchies for error shape**:

```python
class DomainError(Exception): ...
class NoItemsError(DomainError): ...
class NegativeTotalError(DomainError): ...
```

"All domain errors share this base" is encoded once; handlers catch
`DomainError`.

### Mechanisms that *tempt* false DRY

**Over-eager helper extraction**:

```python
def validate_user_input(s: str) -> bool:
    return 0 < len(s) < 100 and "\0" not in s

def validate_order_note(s: str) -> bool:
    return 0 < len(s) < 100 and "\0" not in s
```

Tempting to extract `validate_short_text(s: str) -> bool`. But the
two validations *happen* to coincide today. Tomorrow the order note
rule changes to "≤ 500 chars" and now the helper grows a
`max_len` parameter, then a `null_byte_ok` flag, then a `kind: Enum`
argument.

Better: leave them duplicated until the third copy appears. Hunt's
*Rule of Three*: abstract when you have *three* concrete instances
proving the abstraction is real, not two.

**Premature shared package**:

Monorepos accumulate a `packages/common/` or `src/utils/` that
becomes a junk drawer of weakly-related helpers. The package's
"DRY" benefit is illusory — the helpers were never the same
knowledge, just the same shape.

Better: leave the local helpers local. If three packages genuinely
need the same calculation, extract *that calculation*, not "stuff
the three packages might share".

**Forcing identical APIs onto different abstractions**:

```python
class Storage(Protocol):
    def put(self, k: str, v: bytes) -> None: ...
    def get(self, k: str) -> bytes | None: ...

class DictStorage:
    def put(self, k: str, v: bytes) -> None: ...
    def get(self, k: str) -> bytes | None: ...

class S3Storage:
    def put(self, k: str, v: bytes) -> None: ...
    def get(self, k: str) -> bytes | None: ...
```

Memory and S3 do not share a contract (see
[LSP](LSP.md)) — one is sync-microsecond and
infallible, the other is network-bound and can raise five distinct
exception types. The shared protocol is a DRY-shaped illusion
masking incompatible behaviours. This is what Hettinger means by
"it's not duck typing; it's structural typing" — looking alike is
not enough.

## Violations and remedies

### Anti-pattern: magic numbers duplicated

```python
# api/handlers/auth.py
if len(username) > 64:
    raise HTTPException(400, "too long")

# domain/user.py
if len(request.name) > 64:
    raise DomainError("invalid")

# admin/forms.py
def validate(s: str) -> bool:
    return len(s) <= 64
```

If the limit changes, three places must be edited and someone will
miss the third.

### Idiomatic fix: single source of truth in a domain module

```python
# domain/limits.py
from typing import Final
MAX_USERNAME_LEN: Final[int] = 64
```

```python
# everywhere else
from domain.limits import MAX_USERNAME_LEN
if len(username) > MAX_USERNAME_LEN: ...
```

### Anti-pattern: duplicated if/elif dispatch

```python
def render(event: Event) -> str:
    if event.kind == "created":
        return f"{event.user} created {event.target}"
    elif event.kind == "updated":
        return f"{event.user} updated {event.target}"
    elif event.kind == "deleted":
        return f"{event.user} deleted {event.target}"
    elif event.kind == "archived":
        return f"{event.user} archived {event.target}"
    else:
        raise ValueError(event.kind)
```

The shape `"{user} {verb} {target}"` is repeated four times. The
knowledge "every event renders as user-verb-target" lives nowhere.

### Idiomatic fix: dispatch table or `match` + a typed enum

```python
from enum import StrEnum

class EventKind(StrEnum):
    CREATED = "created"
    UPDATED = "updated"
    DELETED = "deleted"
    ARCHIVED = "archived"

def render(event: Event) -> str:
    match event.kind:
        case EventKind.CREATED | EventKind.UPDATED \
           | EventKind.DELETED | EventKind.ARCHIVED:
            return f"{event.user} {event.kind.value} {event.target}"
```

Or a dispatch table when bodies are heterogeneous:

```python
RENDERERS: dict[EventKind, Callable[[Event], str]] = {
    EventKind.CREATED: _render_created,
    EventKind.UPDATED: _render_updated,
    ...
}
```

### Anti-pattern: duplicated dataclasses across layers

```python
# api/schemas.py
@dataclass
class UserResponse:
    id: UUID
    email: str
    name: str

# domain/user.py
@dataclass
class User:
    id: UUID
    email: str
    name: str

# repo/rows.py
@dataclass
class UserRow:
    id: UUID
    email: str
    name: str
```

Three structs of identical shape, three places to add a column.

### Idiomatic fix: one model, projections at boundaries

Keep one `User` in the domain. Use `pydantic` (or `dataclasses` +
`asdict`) to project to API responses and to map from DB rows. Add a
field once; projections inherit it automatically.

```python
from pydantic import BaseModel

class User(BaseModel):
    id: UUID
    email: str
    name: str

# api response is just `User.model_dump()`
# repo row hydration is `User.model_validate(row)`
```

### Anti-pattern: parallel validation in API and domain

```python
# api/handlers/orders.py
async def create_order_handler(req: CreateOrderRequest) -> Response:
    if not req.items:
        return reject("no items")
    if req.total < 0:
        return reject("negative total")
    # ... 12 more checks ...

# domain/order.py
class Order:
    def __init__(self, items: list[Item], total: Money) -> None:
        if not items:
            raise NoItemsError
        if total < 0:
            raise NegativeTotalError
        # ... 12 more checks ...
```

Every validation rule exists twice. They drift.

### Idiomatic fix: validation lives in the domain (or Pydantic); API delegates

```python
# domain/order.py
from pydantic import BaseModel, model_validator

class Order(BaseModel):
    items: list[Item]
    total: Money

    @model_validator(mode="after")
    def _validate(self) -> "Order":
        if not self.items:
            raise NoItemsError
        if self.total < 0:
            raise NegativeTotalError
        return self
```

```python
# api/handlers/orders.py
async def create_order_handler(req: CreateOrderRequest) -> Response:
    try:
        order = Order(items=req.items, total=req.total)
    except DomainError as e:
        return reject(e)
```

The API performs *no business validation*. It translates errors. A
new rule is added in one place — in the domain model.

### Anti-pattern: copy-pasted sync/async pairs

```python
def fetch_user_sync(id: UUID) -> User:
    row = db.execute("SELECT ... FROM users WHERE id=?", id).fetchone()
    return User.model_validate(row)

async def fetch_user_async(id: UUID) -> User:
    row = await adb.execute("SELECT ... FROM users WHERE id=?", id).fetchone()
    return User.model_validate(row)
```

The SQL, the projection, and the error mapping are duplicated. If
the query changes, both must change.

### Idiomatic fix: pick one colour, or extract the *non-IO* knowledge

Decide which colour your codebase is (async-first is the common
choice for new code). If you genuinely need both, extract the
non-IO parts:

```python
_SQL = "SELECT id, email, name, created_at FROM users WHERE id = ?"

def _to_user(row: Row) -> User:
    return User.model_validate(row)

def fetch_user_sync(id: UUID) -> User:
    return _to_user(db.execute(_SQL, id).fetchone())

async def fetch_user_async(id: UUID) -> User:
    return _to_user(await adb.execute(_SQL, id).fetchone())
```

The SQL and the mapping are stated once.

### Anti-pattern: copy-pasted code that ISN'T DRY

```python
def calculate_tax_us(amount: Money) -> Money: return amount * 0.07
def calculate_tax_eu(amount: Money) -> Money: return amount * 0.21
def calculate_tax_uk(amount: Money) -> Money: return amount * 0.20
```

It would be tempting to extract `calculate_tax(rate: float, amount: Money)`.
Should you?

**No** — for two reasons:

1. The three tax rates are not the same knowledge. They are
   independent regulations. If the EU rate changes, the US rate is
   unaffected.
2. The functions communicate intent. `calculate_tax_us(amount)` reads
   better at the call site than `calculate_tax(0.07, amount)`.

When VAT rates split by region into 27 individual values that vary
together (per EU directive), THEN extract — into a `dict[Region,
Rate]` table. The Rule of Three applies.

### Idiomatic fix: leave as-is

Resist the urge. Three lookups in a table is fine. (See Sandi Metz,
"The Wrong Abstraction".)

### Anti-pattern: repeated `from __future__ import annotations` + repeated regex

```python
# fifteen modules each opening with:
from __future__ import annotations
import re
EMAIL_RE = re.compile(r"^[^@\s]+@[^@\s]+\.[^@\s]+$")
```

The `from __future__` line is mechanical — accept it as boilerplate
(it is one decision: "this codebase uses PEP 563 string annotations
until 3.14"). The regex is *knowledge*: an email-address rule. It
must live exactly once.

### Idiomatic fix: hoist the regex to a `patterns` module

```python
# domain/patterns.py
import re
EMAIL_RE = re.compile(r"^[^@\s]+@[^@\s]+\.[^@\s]+$")
```

Every caller imports `EMAIL_RE`. The pattern is updated once.

## DRY at the package/module level

Cross-package DRY in a Python monorepo shows up as:

- A version string duplicated in multiple `pyproject.toml`s. Fix:
  dynamic version (`hatch-vcs`, `setuptools-scm`) or a workspace
  tool (`uv`'s workspace, Rye, Pants) that resolves versions once.
- A dependency pin duplicated in multiple `pyproject.toml`s. Fix:
  a shared constraints file, or a workspace's `[tool.uv.sources]`
  declared once.
- A dataclass duplicated across packages because both need "the
  same" shape. Fix: one defining package, the others depend on it.
  (Or *don't fix* if the two structs happen to look alike but mean
  different things.)
- A `conftest.py` fixture re-declared in every test subdirectory.
  Fix: hoist to a parent `conftest.py`; pytest discovers it.

## How code-ranker detects DRY violations

DRY is the hardest principle to detect automatically — knowledge
duplication does not have a graph signature. Code Ranker can flag
*candidates*:

| Signal | DRY interpretation |
|---|---|
| Identical function names across multiple modules (e.g. `validate`, `parse`, `format`) | Possible knowledge duplication. Requires fn-name overlap analysis. |
| Module-level constants with identical *values* across multiple packages | Strong DRY-violation candidate. Requires AST inspection. |
| Repeated string-literal regex patterns (`re.compile(...)` in N files) | Textbook DRY violation; regexes encode a rule. |
| Near-identical function bodies (tokenised clone detection) within the same package | Possible Rule-of-Three trigger. |
| Repeated `__init__` parameter lists across dataclasses/Pydantic models | Possible duplicated schema. |
| Multiple `pyproject.toml`s pinning the same dependency at the same version | Workspace consolidation opportunity. |

Code Ranker's static graph cannot tell you whether two functions
*encode the same knowledge* — that requires understanding the
function bodies. A future rule could flag literal duplication and
let the LLM-verification step (see `cpt-code-ranker-fr-prompt-composer`)
decide.

## Suggested recommendation template

> **DRY candidate** (low confidence): the constant `64` appears as a
> max-length check in 5 places across the monorepo (`api/auth.py`,
> `domain/user.py`, `admin/forms.py`, `infra/email/templates.py`,
> `shared/limits.py`). If these are encoding the same business rule
> ("usernames must be ≤ 64 chars"), consolidate to a single
> `domain.limits.MAX_USERNAME_LEN`. If they are independent (a
> column width, an email subject limit, a UI hint), keep them
> separate.
>
> Code Ranker cannot tell which case applies. See *Pragmatic Programmer*
> Topic 9 and matklad's "Three Levels of Repetition" for guidance
> on the call.

## Related principles

- [KISS](KISS.md) — DRY can violate KISS when premature abstraction
  introduces a more complex shape than the duplication.
- [YAGNI](YAGNI.md) — don't DRY for a hypothetical second instance
  that may never appear.
- [SRP](SRP.md) — SRP is the discipline that
  produces *true* DRY by aligning code-units with reasons-to-change.

## References

1. Hunt, A. and Thomas, D. *The Pragmatic Programmer: From Journeyman
   to Master*. Addison-Wesley, 1999 (20th anniv. ed., 2019).
   <https://pragprog.com/titles/tpp20/>
2. matklad. "Three Levels of Repetition". 2024.
   <https://matklad.github.io/2024/02/02/three-levels-of-repetition.html>
3. Abramov, D. "The WET Codebase".
   <https://overreacted.io/the-wet-codebase/>
4. Metz, S. "The Wrong Abstraction". 2016.
   <https://sandimetz.com/blog/2016/1/20/the-wrong-abstraction>
5. Atwood, J. "DRY: It's About Knowledge". 2014.
   <https://blog.codinghorror.com/dry-not-just-about-code/>
6. Peters, T. *PEP 20 — The Zen of Python*.
   <https://peps.python.org/pep-0020/>
7. Hettinger, R. "Beyond PEP 8 — Best practices for beautiful
   intelligible code" and "Transforming Code into Beautiful, Idiomatic
   Python". PyCon US.
