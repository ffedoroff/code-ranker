# Law of Demeter — Principle of Least Knowledge (in Python)

**TL;DR**: A method `f` on class `T` should only call methods on:
(a) `self`, (b) `self`'s direct attributes, (c) parameters passed to
`f`, (d) objects `f` constructs locally. In short: "talk to friends,
not to strangers". In Python this maps to: avoid
`user.profile.address.city.name` attribute chains that traverse
multiple objects; prefer narrow methods that expose exactly what the
caller needs. Python's dynamism makes the violation easy and the
breakage silent — `AttributeError` at runtime, not at type-check
time.

## Canonical sources

- Ian Holland, Karl Lieberherr et al., "Object-Oriented Programming:
  An Objective Sense of Style" (1988): the formal statement of the
  Law of Demeter. <https://dl.acm.org/doi/10.1145/62083.62113>
- Lieberherr, K. "Programming with Propagation Patterns" (1989): the
  canonical academic reference for the Law of Demeter and its
  generalisation to module/package boundaries.
- Northeastern University Demeter Project, "The Law of Demeter":
  <https://www2.ccs.neu.edu/research/demeter/papers/law-of-demeter/oopsla88-law-of-demeter.pdf>
- David Bock, "The Paperboy, The Wallet, and The Law of Demeter"
  (2001): the canonical metaphor.
  <https://www2.ccs.neu.edu/research/demeter/demeter-method/LawOfDemeter/paper-boy/demeter.pdf>
- Hunt and Thomas, *The Pragmatic Programmer*, Topic 28 "Coupling
  and the Law of Demeter".

## The principle

The original Demeter Project formulation gives a method `M` of class
`C` permission to invoke methods only on:

1. The object `M` is a method of (`self`).
2. Arguments of `M`.
3. Objects created by `M`.
4. Direct attributes of the object `M` is a method of.
5. Module-level globals accessible to `C`.

**Not** allowed: invoking methods on objects returned from methods
of any of the above. That is: `a.b().c()` traverses *two* objects;
LoD says you've reached too far. The same applies to attribute
access in Python: `a.b.c` is morally equivalent to `a.b().c()` —
the dot is a method call on the descriptor protocol.

Bock's "Paperboy" metaphor: when the paperboy is collecting money,
he should not say "give me your wallet so I can take what you owe
me". He should say "you owe me $5". The customer manages their own
wallet. The paperboy talks to a friend (the customer), not to a
stranger (the wallet).

In Python:

```python
# Demeter violation: 4-level traversal
username = order.customer.contact.email.local_part
```

The function holding `order` is now coupled to the structure of
`Order`, `Customer`, `Contact`, and `Email`. Any rename in any of
them raises `AttributeError` at runtime — and your type checker may
not catch it if any link in the chain is `Any`, a duck-typed dict,
or a Pydantic model loaded from JSON.

LoD says: ask `Order` for the username (or whatever you actually
need), and let `Order` decide how to traverse:

```python
username = order.customer_email_local_part()
```

`Order` now talks to its `Customer`, which talks to its `Contact`,
each layer responsible for its own knowledge.

## Why it matters

LoD-violating chains are **change amplifiers**:

- Rename `Contact.email` → `Contact.email_address`. Every call site
  that wrote `order.customer.contact.email.local_part` breaks at
  runtime — possibly only on the code path that exercises it.
- Change `Email` from a dataclass with `local_part` to an opaque
  type. Same.
- Add validation that some emails are non-public; the call site has
  bypassed the validation.

In Python the failure mode is worse than in statically-typed
languages: the chain may type-check (or skip checking entirely if
`mypy` is not strict about `Any`), and only blow up in the one
production code path that hits a `None` or a missing attribute.

LoD also enforces a form of encapsulation: your code expresses
**what you want**, not **how to reach it**. The how is hidden behind
the boundary of each type.

## In Python

Python has few natural enforcements — unlike Rust's `pub` keyword
or Java's `private`, every attribute is reachable by convention
(leading underscore) only. That makes LoD discipline more important,
not less.

Useful Python-specific tools:

- **`@dataclass(frozen=True, slots=True)`** plus underscore-prefixed
  fields makes accidental mutation harder and signals "this is
  internal".
- **`typing.Protocol`** lets you define narrow structural interfaces
  for what a function *actually needs*, rather than passing the
  whole aggregate.
- **`@property`** allows you to replace a public attribute with a
  computed accessor without changing call sites — but a property
  that returns a deep sub-object is still a Demeter violation.

Method chains on iterables (`map`, `filter`, list comprehensions)
are NOT LoD violations — they operate on a single conceptual
sequence. Same friend, repeated.

The Python-idiomatic LoD discipline:

1. **Public attributes are not free.** If you expose `self.profile`
   as public on a `User`, every caller can write
   `user.profile.address.city.name`. Prefer methods that name the
   operation: `user.city_name()`.
2. **Methods take what they need, not the kitchen sink.** Pass
   `Email` if you need an email; don't pass `Workspace` and dig.
3. **Don't traverse into details you don't own.** If you need a
   user's email format check, ask the user — don't pull the email
   out and check it yourself.

## Violations and remedies

### Anti-pattern: deep attribute traversal

```python
def send_welcome(workspace: Workspace, user_id: UserId) -> None:
    user = workspace.users[user_id]
    email = user.profile.contact.email.address
    smtp = workspace.config.notifications.email.smtp
    send_email_via(smtp, email)
```

`send_welcome` knows the full shape of `Workspace`, `User`,
`Profile`, `Contact`, `Email`, `Config`, `NotificationsConfig`,
`EmailConfig`, `SmtpConfig`. Touching any of them is a breaking
change for `send_welcome`.

### Idiomatic fix: pass what's needed; let the owner traverse

```python
class Notifier(Protocol):
    def send_welcome_email(self, user: User) -> None: ...

def send_welcome(notifier: Notifier, user: User) -> None:
    notifier.send_welcome_email(user)
```

`Notifier` is a `Protocol` that knows about SMTP config. `User`
exposes `email_address()` (one accessor) and keeps everything else
private. `send_welcome` knows two friends: `Notifier` and `User`.

### Anti-pattern: train wrecks in the Django ORM

```python
# Five hops across four models
region_name = order.customer.addresses.first().city.country.name
```

This is the classic Django train wreck. Note that `select_related`
is **orthogonal** to LoD: `select_related` fixes the N+1 query
problem, but the *call site* is still coupled to the entire shape of
the model graph. The view module now depends on `Order`, `Customer`,
`Address`, `City`, `Country`. Add an intermediate `Region` model
between `City` and `Country` and every view breaks.

### Idiomatic fix: ask the aggregate root

```python
class Order(models.Model):
    def primary_region_name(self) -> str:
        # Knows how to traverse its own relations
        return self.customer.addresses.first().city.country.name

# Caller:
region_name = order.primary_region_name()
```

`Order` owns the traversal. If the schema changes, exactly one
method needs updating. Callers see a stable interface. Use
`select_related` *inside* the method to fix the query cost without
leaking the schema.

### Anti-pattern: Pydantic models that expose the whole graph

```python
class User(BaseModel):
    profile: Profile
    addresses: list[Address]
    preferences: UserPrefs

class Profile(BaseModel):
    contact: Contact

class Contact(BaseModel):
    email: Email
    phone: Phone

# Caller in some API handler:
local_part = user.profile.contact.email.address.split("@")[0]
```

Pydantic encourages this shape because nested models map directly
onto nested JSON. The convenience hides the coupling: the handler
now depends on the full nested model graph.

### Idiomatic fix: flat DTOs at the boundary

```python
@dataclass(frozen=True, slots=True)
class UserSummary:
    id: UserId
    email_local_part: str
    city_name: str
    is_admin: bool

def summarise(user: User) -> UserSummary:
    return UserSummary(
        id=user.id,
        email_local_part=user.email_local_part(),
        city_name=user.city_name(),
        is_admin=user.is_admin(),
    )
```

The internal Pydantic graph is rich; the *exposed* DTO is flat. The
nested models stay internal to the domain layer. The API/handler
layer sees `UserSummary` and never reaches in.

### Anti-pattern: returning a deep tree to extract a leaf

```python
def primary_address(order: Order) -> str:
    return order.customer().get_addresses()[0].postal_code.region.name
```

Five hops. Add one new layer between `Customer` and `Address` and
the function breaks.

### Idiomatic fix: ask for the leaf directly

```python
class Order:
    def primary_region(self) -> str:
        # Knows internals
        ...
```

`Order` owns the traversal; callers ask for what they need.

### Anti-pattern: returning internal mutable state

```python
class Cart:
    def items(self) -> list[Item]:
        return self._items

# Caller now has unrestricted access to internal state:
cart.items().append(weird_item)        # bypasses validation
cart.items().sort(key=lambda i: i.price)  # breaks invariants
```

`items()` returns a stranger. Once you hand the caller the live
list, they can do anything `list` allows — including violating
invariants that `Cart.add` was supposed to enforce. Returning a
mutable internal collection is the Python equivalent of Rust's
`&mut Vec<T>` leak.

### Idiomatic fix: expose operations, not the container

```python
class Cart:
    def __init__(self) -> None:
        self._items: list[Item] = []

    def add(self, item: Item) -> None:
        # validates
        ...

    def remove(self, item_id: ItemId) -> None:
        # validates
        ...

    def items(self) -> Iterator[Item]:
        # Read-only view
        return iter(self._items)
```

Callers work through the cart, not on its internals. Returning an
iterator (or a `tuple(...)` copy) gives read access without exposing
the mutable container.

### Anti-pattern: pass-through accessors

```python
class Order:
    @property
    def customer(self) -> Customer: return self._customer

class Customer:
    @property
    def contact(self) -> Contact: return self._contact

class Contact:
    @property
    def email(self) -> Email: return self._email

# Now any caller can:
e = order.customer.contact.email
```

You've exposed the full traversal. Each step is technically a
property access on `self`, but the caller has assembled a chain that
violates LoD's spirit. Properties are sugar over methods; the
Demeter rule still applies.

### Idiomatic fix: expose the operation, not the getter

```python
class Order:
    def customer_email(self) -> Email:
        return self._customer._contact._email
```

One accessor, one purpose. If `Customer` later separates work and
home contacts, the change happens in `Order.customer_email`, not at
every call site.

### Anti-pattern: the `__getattr__` flattening trick

```python
class FlatUser:
    def __init__(self, user: User) -> None:
        self._user = user

    def __getattr__(self, name: str) -> Any:
        # "Flatten" nested access by searching the graph
        for obj in (self._user, self._user.profile,
                    self._user.profile.contact):
            if hasattr(obj, name):
                return getattr(obj, name)
        raise AttributeError(name)

# Caller:
email = flat_user.email   # looks like one hop!
```

This *looks* like a fix — the caller only writes `flat_user.email`,
one dot. But it is LoD violation in disguise:

- The traversal logic still depends on the graph shape; it has just
  moved into `__getattr__`.
- Type checkers see `Any` and lose all signal.
- Renames silently break: `FlatUser` doesn't know that `Contact.email`
  was renamed; the attribute simply ceases to resolve.
- The dynamism hides the coupling from code review and from
  code-ranker's static graph.

The fix is not to flatten dynamically. It is to expose **named
operations** on the holder object — same as the non-dynamic case.

### Anti-pattern: attribute-bag classes

```python
@dataclass
class User:
    id: UserId
    email: Email
    roles: list[Role]
    created_at: datetime
    last_login: datetime | None
    preferences: UserPrefs
    addresses: list[Address]
```

Every field is public. Callers reach into any of them. This is
LoD's worst-case shape: no encapsulation. Renaming any field is a
breaking change for every caller.

### Idiomatic fix: private fields, narrow API

```python
@dataclass(frozen=True, slots=True)
class User:
    _id: UserId
    _email: Email
    _roles: tuple[Role, ...]
    # ...

    def id(self) -> UserId: return self._id
    def email(self) -> Email: return self._email
    def has_role(self, r: Role) -> bool: ...
    def is_admin(self) -> bool: ...
    def primary_address(self) -> Address | None: ...
```

`User` decides what to expose. Internals can be rearranged.
Underscore prefix is a convention, not enforcement — but combined
with `frozen=True` and `slots=True`, accidental misuse is rare.

## Iterator chains are not LoD violations

```python
total = sum(item.price() for item in order.items())
```

This is **not** a Demeter violation, even though it touches every
item. The comprehension operates on a single conceptual sequence
returned by `order.items()`. Demeter is about coupling to unrelated
objects, not about syntactic chain length.

A useful test: if the chain transforms a single conceptual entity
(a sequence), it is fine. If the chain hops across unrelated
entities (`workspace.config.notifications.email.smtp`), it is the
LoD-violating pattern.

## LoD at the package level

LoD generalises to packages. A module that reaches *deep* into
another package's submodules is the same anti-pattern:

```python
# Bad
from other_pkg.internals.storage.adapters.postgres.pool import Pool
```

The using module depends on five layers of `other_pkg`'s hierarchy.
Renaming any of `internals` / `storage` / `adapters` / `postgres` /
`pool` breaks downstream.

LoD-friendly version: `other_pkg` exposes a re-export at the
package root via `__init__.py`.

```python
from other_pkg import Pool
```

The path is one hop. Internals are free to evolve.

## How code-ranker detects LoD violations

Module-level LoD violations have a graph signature:

| Signal | LoD interpretation |
|---|---|
| `Import` edge from one package to a deeply-nested submodule of another package (path depth > 2) | Reaching too far into another package's hierarchy |
| Function fan-out: a single function calls into many sibling packages | Talking to many strangers; possible SRP overlap |
| Long chained attribute access (`a.b.c.d.e`) detected via AST | Function-level LoD violation |
| `@property` that returns a non-primitive owned by another module | Pass-through accessor; future rule |

A future rule **`cross-package-deep-reach`** could detect: import
of a name more than 2 path segments deep into a foreign package.
Severity low (often fine), confidence medium (real violation in
many but not all cases).

Another rule **`chained-attribute-depth`** could flag attribute
chains of depth > 3 in a single expression, since each dot is a
potential coupling point.

## Suggested recommendation template

> **LoD candidate**: module `api.handlers` imports
> `domain.internals.types.raw.User`. The import reaches four
> levels deep into `domain`'s package hierarchy, exposing the using
> module to renames at every level. Re-export `User` at `domain`'s
> root (in `domain/__init__.py`) and import via the shorter path.
> The Demeter principle (Lieberherr, 1989) extends to module
> traversal: depend on friends, not on strangers' internals.
>
> Reference: <https://www2.ccs.neu.edu/research/demeter/papers/law-of-demeter/oopsla88-law-of-demeter.pdf>

## Related principles

- [DIP](DIP.md) — DIP makes the friends
  `Protocol`-based, which limits how deep callers can reach.
- [Information Hiding](CoI.md) — LoD is
  the dynamic counterpart to "hide your attributes".
- [SRP](SRP.md) — when a method talks to too
  many strangers, it usually has too many responsibilities.

## References

1. Lieberherr, K. and Holland, I. "Assuring Good Style for
   Object-Oriented Programs". *IEEE Software*, 1989.
2. Lieberherr, K. "Programming with Propagation Patterns". 1989.
3. Holland, I. "Specifying Reusable Components Using Contracts".
   PhD thesis, Northeastern University, 1992.
4. Bock, D. "The Paperboy, The Wallet, and The Law of Demeter".
   <https://www2.ccs.neu.edu/research/demeter/demeter-method/LawOfDemeter/paper-boy/demeter.pdf>
5. Hunt, A. and Thomas, D. *The Pragmatic Programmer*. Topic 28.
6. Demeter Project home.
   <https://www.ccs.neu.edu/research/demeter/>
