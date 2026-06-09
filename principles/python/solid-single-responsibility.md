# SRP — Single Responsibility Principle (in Python)

**TL;DR**: A module, class, or function should have one reason to change.
In a Python codebase, this most often means: a single `Protocol` or class
should not accumulate responsibilities for unrelated callers; a single
package should not be both the wiring root and the type registry; a
single module should not host both domain logic and IO adapters.

## Canonical sources

- Robert C. Martin, "The Principles of OOD" (originally in *More C++ Gems*,
  1996; later in *Clean Architecture*, 2017): "A class should have only one
  reason to change." Source: <https://blog.cleancoder.com/uncle-bob/2014/05/08/SingleReponsibilityPrinciple.html>
- Robert C. Martin, *Clean Architecture*, Ch. 7: refines the principle to
  "A module should be responsible to one, and only one, actor."
- David L. Parnas, "On the Criteria To Be Used in Decomposing Systems Into
  Modules", *CACM* 15(12), 1972 — the foundational paper on information
  hiding and decomposition by *secret* (i.e. by reason-to-change).
- Mark Seemann, "The Single Responsibility Principle" (2011):
  <https://blog.ploeh.dk/2011/03/22/SOLIDinIntroductoryProgramming/>
- PEP 8 (module layout) and PEP 20 (Zen): "Namespaces are one honking great
  idea — let's do more of those."

## The principle

Martin's later formulation is the most useful: **a module is responsible
to one actor**. An "actor" is any stakeholder whose needs drive changes —
a regulatory body, a product owner, a downstream team. When a module
serves two actors, changes requested by one are forced through review by
the other, and the module accumulates conflicting pressures.

The popular short form — "one reason to change" — is sometimes misread as
"one method per class" or "one function per file". That is not the
principle. The unit of responsibility is **change pressure**: if two
pieces of code consistently change for the same reason, they belong
together; if they change for different reasons, they belong apart.

In Python, the unit of "responsibility" most naturally maps to a package,
then to a module, then to a class or `Protocol`. Functions are usually
too fine-grained — splitting a 50-line function in two does not change
which actor causes it to evolve.

## Why it matters

A module shared by multiple actors becomes a coordination chokepoint:

- Every PR must pass review by every actor's team.
- Tests proliferate because each actor's changes can break the others'.
- Refactoring is "expensive" because so many callers depend on the
  module's exact shape (and Python's lack of compile-time checks means
  breakage shows up at import or run time).
- `git blame` and commit history become hard to read: a single file
  with eight reasons to change has eight times the commit churn.

SRP is the principle that keeps a codebase **navigable**. When you
violate it, the symptom is not a runtime bug — it is the gradual
realization that nobody on the team feels comfortable touching certain
files.

## In Python

Python tooling and conventions encourage SRP at multiple granularities:

| Unit | What "one responsibility" means |
|---|---|
| Distribution (`pyproject.toml`) | The whole product / library family |
| Package | One bounded context (an "actor's worth" of code) |
| Module | One coherent concept inside a package |
| File | One implementation concern (e.g. a single Protocol + its support types) |
| Class / `Protocol` | One thing it represents |
| Function | One mental step the caller performs |

The most common SRP violation in real Python projects is the
**"god module"**: `utils.py`, `helpers.py`, `common.py`, or
`core/__init__.py` imported by many siblings because it collects
unrelated conversions, decorators, constants, and base classes under one
name.

The second most common violation is the **"god class"**: a single class
named `*Service`, `*Manager`, `*Handler`, or `*Client` with 30+ methods
spanning multiple concerns (CRUD, validation, business rules, audit
logging, metrics, retries). A close cousin is the class that is really
"a module in disguise" — a class consisting almost entirely of
`@staticmethod`s, used purely as a namespace.

## Violations and remedies

### Anti-pattern: god `Service` class

```python
# Bad: one class, many actors.
from dataclasses import dataclass

@dataclass
class UserService:
    db: PgPool
    cache: Redis
    audit: AuditSink
    metrics: MetricsClient
    mailer: Mailer

    async def create_user(self, ...) -> User: ...
    async def deactivate_user(self, ...) -> None: ...
    async def record_login(self, ...) -> None: ...
    async def export_for_gdpr(self, ...) -> bytes: ...
    async def send_welcome_email(self, ...) -> None: ...
    async def rotate_password(self, ...) -> None: ...
    async def assign_role(self, ...) -> None: ...
    async def audit_admin_change(self, ...) -> None: ...
    # ... 30 more methods
```

Reasons to change: GDPR compliance (legal), email templates (marketing),
auth flow (security), RBAC (product), audit retention (ops). Five
different actors, one class.

A clustering signal: most methods touch only a *subset* of `self.*`
attributes. `record_login` and `rotate_password` touch `db` and
`cache`; `send_welcome_email` only touches `mailer`; `export_for_gdpr`
only touches `db` and `audit`. Disjoint attribute usage is a strong
hint that the class is really several classes glued together.

### Idiomatic fix: split by actor

```python
from dataclasses import dataclass

@dataclass
class UserRepository:
    db: PgPool

    async def create(self, ...) -> User: ...
    async def deactivate(self, user_id: UserId) -> None: ...

@dataclass
class UserAuthService:
    repo: UserRepository
    hasher: Argon2

    async def rotate_password(self, ...) -> None: ...
    async def record_login(self, ...) -> None: ...

@dataclass
class UserComplianceService:
    repo: UserRepository
    audit: AuditSink

    async def export_for_gdpr(self, ...) -> bytes: ...

@dataclass
class UserNotifier:
    mailer: Mailer

    async def welcome(self, ...) -> None: ...
```

Each type now has one actor. Legal changes touch only
`UserComplianceService`; marketing touches only `UserNotifier`; etc.

If you want to depend on capabilities rather than concretions, define
`Protocol`s in the calling module:

```python
from typing import Protocol

class UserReader(Protocol):
    async def get(self, user_id: UserId) -> User: ...

class UserWriter(Protocol):
    async def create(self, ...) -> User: ...
    async def deactivate(self, user_id: UserId) -> None: ...
```

`UserRepository` satisfies both structurally; callers depend only on
the slice they use.

### Anti-pattern: god module

```python
# package: account/service.py  (4000 LOC)
# Hosts: class Service, class ServiceContext, class ServiceError,
# TypeAliases, re-exports of repos, helpers like `now_utc()`,
# decorators like `@service_log`, and miscellaneous mixins.
```

A particularly Pythonic variant is the **`__init__.py` re-export soup**:
a package `__init__.py` that does `from .a import *; from .b import *;
from .c import *` and is imported by 19 siblings. Every sibling pays
the import-time cost of every change to any unrelated item in the
package. The graph signal is an exceptionally high fan-in module that
sits at the centre of an import SCC.

### Idiomatic fix: pull each concern to its own module

```
account/
└── service/
    ├── __init__.py    # explicit __all__; re-exports only intentional API
    ├── protocol.py    # the Service Protocol
    ├── context.py     # ServiceContext (DI carrier, a @dataclass)
    ├── errors.py      # ServiceError hierarchy
    ├── time.py        # clock helpers
    └── decorators.py  # service_log, etc.
```

```python
# account/service/__init__.py
from .protocol import Service
from .context  import ServiceContext
from .errors   import ServiceError

__all__ = ["Service", "ServiceContext", "ServiceError"]
```

Names not in `__all__` are treated as private (the Python equivalent of
`pub(crate)` is the leading-underscore convention plus an explicit
`__all__`). Now a change to error formatting touches `errors.py` alone;
siblings import what they need from a leaf module, not from a god
package.

### Anti-pattern: namespace class (module-in-a-class)

```python
class StringHelpers:
    @staticmethod
    def slugify(s: str) -> str: ...
    @staticmethod
    def truncate(s: str, n: int) -> str: ...
    @staticmethod
    def parse_csv_row(s: str) -> list[str]: ...
    @staticmethod
    def html_escape(s: str) -> str: ...
```

The class carries no state and no polymorphism — it is a module wearing
a class hat. SRP-wise it is doubly suspect: it advertises one
responsibility ("string things") that is in fact four (slugs,
truncation, CSV parsing, HTML escaping), each with its own actor
(routing, UI, data import, security).

### Idiomatic fix: actual modules, split by concern

```
text/
├── slug.py        # slugify
├── truncate.py    # truncate
├── csv_row.py     # parse_csv_row
└── html.py        # html_escape
```

Use classes when you need state, polymorphism, or `Protocol`
conformance — not as a poor man's namespace.

### Anti-pattern: function with mixed concerns

```python
async def process_payment(order: Order, db: PgPool) -> None:
    # 1. Validate
    if order.total < Money.ZERO:
        raise ValueError(...)
    if not order.items:
        raise ValueError(...)
    # 2. Persist
    await db.execute("INSERT INTO orders ...", ...)
    # 3. Charge
    stripe_resp = await stripe.charge(...)
    # 4. Notify
    await notify_warehouse(order)
    await notify_user(order)
    # 5. Audit
    await audit.record(...)
```

This function has five reasons to change. A new validation rule, a
Stripe SDK bump, a warehouse-integration change, a notification
preference, and an audit-format adjustment all touch the same body.
The IO/logic mix is a particularly toxic Python smell: the function
cannot be unit-tested without mocking five collaborators.

### Idiomatic fix: extract steps; orchestrator just sequences them

```python
async def process_payment(
    order: Order,
    validator: OrderValidator,
    repo: OrderRepository,
    payment: PaymentGateway,
    notifier: Notifier,
    audit: AuditSink,
) -> None:
    validator.check(order)
    await repo.persist(order)
    await payment.charge(order)
    await notifier.notify(order)
    await audit.record_payment(order)
```

Each collaborator is a `Protocol`; the orchestrator depends on
interfaces, not on imports of `stripe`, `smtplib`, or `psycopg`. The
orchestrator's one reason to change is the *order* of steps.

### Aside: `match` for dispatch, not for hidden responsibility

```python
def handle(event: Event) -> None:
    match event:
        case OrderPlaced():    place(event)
        case PaymentRefunded(): refund(event)
        case UserDeleted():    forget(event)
```

`match` is fine as long as each branch delegates to a single-actor
collaborator. It becomes an SRP violation when the branches *inline*
the work — at which point the dispatcher accumulates every actor's
logic. See also
[Open/Closed Principle](solid-open-closed.md).

## SRP at the package level

The same principle scales up. A package is "responsible to one actor"
when its changelog is intelligible: every released version answers a
single question — "what changed for `X`?". When a package has releases
labelled "add OpenAPI registration, fix Postgres reconnect, bump
pydantic, add JWT verification, format errors", it is serving five
actors and is a refactoring candidate.

A reasonable layout for a multi-actor product:

```
src/
├── libs/
│   ├── modkit_db/          # storage actor
│   ├── modkit_security/    # security actor
│   ├── modkit_http/        # transport actor
│   └── modkit_errors/      # error vocabulary actor
└── modules/
    ├── account/            # account product actor
    ├── billing/            # billing product actor
    └── notifications/      # notification product actor
```

Each package has one actor; cross-package dependencies are explicit
imports (and, ideally, enforced by an import-linter contract).

## How code-ranker detects SRP violations

Code Ranker cannot read actors directly, but the graph signatures of an
SRP violation are unambiguous in Python:

| Signal | SRP interpretation |
|---|---|
| Module with high fan-in × fan-out (god-module-coupling rule) | Module serves multiple unrelated siblings |
| File LOC and item-count breaching mega-file thresholds | Single file accumulating multiple concerns |
| Module composed mostly of re-exports + entangled in an SCC (`__init__.py` soup) | Package acts as both a facade and a participant in unrelated subsystems |
| Class with disjoint `self`-attribute usage clusters across methods | God class — each cluster is a latent class |
| Module with very high public-API count (`__all__` length or top-level public names) | Module is several modules pretending to be one |
| Public function with very high fan-in (high-fan-in-public-api rule) | Single API surface used by many unrelated actors — every change is a coordination event |

Cross-references in code-ranker's catalog:

- `god-module-coupling` directly maps to "module-serving-many-actors"
- `mega-file` maps to "file-with-too-many-reasons-to-change"
- `prelude-sibling-cycle` maps to "facade-module-conflated-with-participation"
- `class-attribute-cluster-split` maps to "god-class-by-attribute-usage"

## Suggested recommendation template

When code-ranker detects a candidate SRP violation, the Finding should:

1. Quote Martin's "one reason to change" / "one actor" and reference
   Parnas (1972) for the underlying decomposition criterion.
2. Pin the violation to the offending node (module, package, or class).
3. Ask the user to enumerate the *actors* whose changes touch this
   unit in the last N months (informally — this is qualitative).
4. Suggest a split along those actor lines.
5. Cite Martin's clean-coder post and Parnas's CACM paper.

Example body:

> **SRP violation candidate**: module `account.service` has fan-in 19
> and fan-out 6, with 27 public names in `__all__`. SRP (Martin 1996,
> after Parnas 1972) prescribes one reason to change per module.
> Identify the actors driving recent commits to this module; if more
> than two are visible, split the module along those lines. Suggested
> first move: extract `account.service.context` (the DI carrier, a
> `@dataclass`) and `account.service.errors` (the error vocabulary)
> into leaf modules; have `account/service/__init__.py` re-export only
> the intentional API via `__all__`.

## Related principles

- [Open/Closed Principle](solid-open-closed.md) — what to do once SRP
  has been applied: keep each unit closed to modification.
- [Interface Segregation Principle](solid-interface-segregation.md) —
  same idea applied to `Protocol` surface, not module surface.
- [DRY](dry.md) — distinct: SRP is about *why* code changes; DRY is
  about *whether* knowledge is duplicated.
- [Composition over Inheritance](composition-over-inheritance.md) —
  SRP is the cohesion lever; composition is the coupling lever.

## References

1. Martin, R. C. "The Single Responsibility Principle". Clean Coder
   Blog, 2014. <https://blog.cleancoder.com/uncle-bob/2014/05/08/SingleReponsibilityPrinciple.html>
2. Martin, R. C. *Clean Architecture: A Craftsman's Guide to Software
   Structure and Design*. Prentice Hall, 2017. Ch. 7 — "SRP: The
   Single Responsibility Principle".
3. Parnas, D. L. "On the Criteria To Be Used in Decomposing Systems
   Into Modules". *Communications of the ACM* 15(12), 1972.
4. Seemann, M. "The Single Responsibility Principle". Ploeh blog, 2011.
   <https://blog.ploeh.dk/2011/03/22/SOLIDinIntroductoryProgramming/>
5. PEP 8 — Style Guide for Python Code. <https://peps.python.org/pep-0008/>
6. PEP 20 — The Zen of Python. <https://peps.python.org/pep-0020/>
7. PEP 695 — Type Parameter Syntax. <https://peps.python.org/pep-0695/>
