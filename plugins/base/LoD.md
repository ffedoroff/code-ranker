# Law of Demeter — Principle of Least Knowledge

**TL;DR**: A method `f` of type `T` should only call methods on:
(a) `T` itself, (b) `T`'s direct fields, (c) parameters passed to
`f`, (d) objects `f` constructs locally. In short: "talk to friends,
not to strangers". In practice this maps to: avoid
`x.foo().bar().baz()` chains that traverse multiple objects; prefer
narrow accessors that expose exactly what the caller needs.

## Canonical sources

- Ian Holland, Karl Lieberherr et al., "Object-Oriented Programming:
  An Objective Sense of Style" (1988): the formal statement of the
  Law of Demeter. <https://dl.acm.org/doi/10.1145/62083.62113>
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

1. The object `M` is a method of (the receiver, often called `self`
   or `this`).
2. Arguments of `M`.
3. Objects created by `M`.
4. Direct fields of the object `M` is a method of.
5. Global variables (in their sense) accessible to `C`.

**Not** allowed: invoking methods on objects returned from methods
of any of the above. That is: `a.b().c()` traverses *two* objects;
LoD says you've reached too far.

Bock's "Paperboy" metaphor: when the paperboy is collecting money,
he should not say "give me your wallet so I can take what you owe
me". He should say "you owe me $5". The customer manages their own
wallet. The paperboy talks to a friend (the customer), not to a
stranger (the wallet).

For example:

```
// Demeter violation: 3-level traversal
username = order.customer.contact.email.local_part()
```

The function holding `order` is now coupled to the structure of
`Order`, `Customer`, `Contact`, and `Email`. Any rename in any of
them breaks this code.

LoD says: ask `Order` for the username (or whatever you actually
need), and let `Order` decide how to traverse:

```
username = order.customer_email_local_part()
```

`Order` now talks to its `Customer`, which talks to its `Contact`,
each layer responsible for its own knowledge.

## Why it matters

LoD-violating chains are **change amplifiers**:

- Rename `Contact.email` → `Contact.email_address`. Every call site
  that wrote `order.customer.contact.email.local_part()` breaks.
- Change `Email` from a record with a `local_part` field to an
  opaque type. Same.
- Add validation that some emails are non-public; the call site has
  bypassed the validation.

When chains run deep, the coupled call site has *transitively
guessed* the data model of types it should not know about. The
guess becomes a constraint.

LoD also enforces a form of [encapsulation](#information-hiding):
your code expresses **what you want**, not **how to reach it**. The
how is hidden behind the boundary of each type.

## Applying the principle

Some language features encourage LoD naturally:

- Field access can require the field to be public (or in the same
  module). Reaching into `a.b.c.d` across a boundary requires
  multiple public fields, which is friction.
- Languages with strict aliasing or mutation rules reject some
  chained expressions that would compile elsewhere.
- Fluent transformation chains (`.map().filter().collect()`) are NOT
  LoD violations — each call is on the immediate object the previous
  returned, but conceptually they form one expression. The friend
  vs stranger test still applies: each adapter is a "friend" of the
  collection/iterator interface.

The LoD discipline in practice:

1. **Public fields are rare.** Prefer methods that name the
   operation. `order.total()` instead of `order.total`.
2. **Methods take what they need, not the kitchen sink.** Pass an
   `Order` if you need the order, not the whole `Workspace`.
3. **Don't traverse into details you don't own.** If you need a
   user's email format check, ask the user — don't pull the email
   out and check it yourself.

## Violations and remedies

### Anti-pattern: deep traversal

```
function send_welcome(workspace, user_id) {
    user = workspace.users().get(user_id)
    email = user.profile.contact.email.address
    smtp = workspace.config.notifications.email.smtp
    send_email_via(smtp, email, ...)
}
```

`send_welcome` knows the full shape of `Workspace`, `User`, `Profile`,
`Contact`, `Email`, `Config`, `NotificationsConfig`, `EmailConfig`,
`SmtpConfig`. Touching any of them is a breaking change for
`send_welcome`.

### Idiomatic fix: pass what's needed; let the owner traverse

```
function send_welcome(notifier, user) -> Result {
    return notifier.send_welcome_email(user)
}
```

`Notifier` is a port (interface plus implementation) that knows about
SMTP config. `User` exposes `email_address()` (one accessor) and keeps
everything else private. `send_welcome` knows two friends: `Notifier`
and `User`.

### Anti-pattern: returning a deep tree just to extract a leaf

```
function primary_address(o: Order) -> String {
    return o.customer().get_addresses()[0].postal_code.region.name
}
```

Five hops. Add one new layer between `Customer` and `Address` and
the function breaks.

### Idiomatic fix: ask for the leaf directly

```
class Order {
    primary_region() -> String { /* knows internals */ }
}
```

`Order` owns the traversal; callers ask for what they need.

### Anti-pattern: returning internal mutable state

```
class Cart {
    items_mut() -> MutableList<Item> { return this.items }
}

// Caller now has unrestricted access to internal state:
cart.items_mut().push(weird_item)              // bypasses validation
cart.items_mut().sort_by(i => i.price)         // breaks invariants
```

`items_mut()` returns a stranger. Once you hand the caller the raw
mutable list, they can do anything the list allows — including
violating invariants that `Cart.add_item` was supposed to enforce.

### Idiomatic fix: expose operations, not the container

```
class Cart {
    add(item) -> Result { /* validates */ }
    remove(id) -> Result { /* validates */ }
    items() -> ReadOnlySequence<Item> { return readonly(this.items) }
}
```

Callers do work through the cart, not on its internals. Read-only
access is OK; mutation goes through methods that enforce invariants.

### Anti-pattern: pass-through accessor chains

```
class Order   { customer() -> Customer { return this.customer } }
class Customer { contact()  -> Contact  { return this.contact } }
class Contact  { email()    -> Email    { return this.email } }

// Now any caller can:
e = order.customer().contact().email()
```

You've exposed the full traversal. Every step is technically a
"method call on the receiver", but the caller has assembled a chain
that violates LoD's spirit.

### Idiomatic fix: don't add the accessors until they are necessary, and even then add the *operation* not the *getter*

```
class Order {
    customer_email() -> Email { return this.customer.contact.email }
}
```

One accessor, one purpose. If `Customer` later separates work and
home contacts, the change happens in `Order.customer_email`, not at
every call site.

### Anti-pattern: getter for everything

```
type User {
    public id: UserId
    public email: Email
    public roles: List<Role>
    public created_at: Timestamp
    public last_login: Optional<Timestamp>
    public preferences: UserPrefs
    public addresses: List<Address>
}
```

Every field is public. Callers can reach into any of them. This is
LoD's worst-case shape: no encapsulation. Renaming any field is
breaking.

### Idiomatic fix: private fields, narrow API

```
class User {  // private fields
    id() -> UserId { return this.id }
    email() -> Email { return this.email }
    has_role(r) -> bool { /* ... */ }
    is_admin() -> bool { /* ... */ }
    primary_address() -> Optional<Address> { /* ... */ }
}
```

`User` decides what to expose. Internals can be rearranged.

## Fluent transformation chains are not LoD violations

```
total = order.items().map(i => i.price()).sum()
```

This is **not** a Demeter violation, even though it chains three
calls. Each call is on the collection/iterator interface, which is the
same "friend" throughout. The chain expresses *one* idea (sum of
prices) in one expression. Demeter is about coupling to unrelated
objects, not about syntactic chain length.

A useful test: if the chain transforms a single conceptual entity
(a sequence), it is fine. If the chain hops across unrelated
entities (`workspace.config.notifications.email.smtp`), it is the
LoD-violating pattern.

## LoD at the module level

LoD generalizes to modules. A module that reaches *deep* into
another module's submodules is the same anti-pattern:

```
// Bad
import other_package.internals.storage.adapters.postgres.pool.Pool
```

The using module depends on three layers of `other_package`'s
hierarchy. Renaming any of
`internals`/`storage`/`adapters`/`postgres`/`pool` breaks downstream.

LoD-friendly version: `other_package` exposes a re-export at its
public root.

```
import other_package.Pool
```

The path is one hop. Internals are free to evolve.

## How code-ranker detects LoD violations

Module-level LoD violations have a graph signature:

| Signal | LoD interpretation |
|---|---|
| `Uses` edge from one module to a deeply-nested submodule of another package (path depth > 2) | Reaching too far into another package's hierarchy |
| Multiple call sites with very long callee paths (e.g. `a.b.c.d.e()`) | Function-level LoD violation; requires AST analysis |
| Public field on a type that is read from another module | Pure data exposure; future rule |

A future rule **`cross-module-deep-reach`** could detect: import
of an item more than 2 path segments deep into a foreign package.
Severity low (often fine), confidence medium (real violation in
many but not all cases).

## Suggested recommendation template

> **LoD candidate**: module `api` imports
> `domain.internals.types.raw.User`. The import reaches four
> levels deep into `domain`'s module hierarchy, exposing the using
> module to renames at every level. Re-export the type at `domain`'s
> public root (`domain.User`) and import via the shorter path. The
> Demeter principle (Holland et al., 1988) extends to module
> traversal: depend on friends, not on strangers' internals.
>
> Reference: <https://www2.ccs.neu.edu/research/demeter/papers/law-of-demeter/oopsla88-law-of-demeter.pdf>

## Related principles

- [DIP](DIP.md) — DIP makes the friends
  interface-based, which limits how deep callers can reach.
- [Information Hiding](CoI.md) — LoD is
  the dynamic counterpart to "hide your fields".
- [SRP](SRP.md) — when a method talks to too
  many strangers, it usually has too many responsibilities.

## References

1. Lieberherr, K. and Holland, I. "Assuring Good Style for
   Object-Oriented Programs". *IEEE Software*, 1989.
2. Holland, I. "Specifying Reusable Components Using Contracts".
   PhD thesis, Northeastern University, 1992.
3. Bock, D. "The Paperboy, The Wallet, and The Law of Demeter".
   <https://www2.ccs.neu.edu/research/demeter/demeter-method/LawOfDemeter/paper-boy/demeter.pdf>
4. Hunt, A. and Thomas, D. *The Pragmatic Programmer*. Topic 28.
5. Demeter Project home.
   <https://www.ccs.neu.edu/research/demeter/>
