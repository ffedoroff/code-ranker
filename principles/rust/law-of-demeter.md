# Law of Demeter — Principle of Least Knowledge (in Rust)

**TL;DR**: A method `f` of struct `T` should only call methods on:
(a) `T` itself, (b) `T`'s direct fields, (c) parameters passed to
`f`, (d) objects `f` constructs locally. In short: "talk to friends,
not to strangers". In Rust this maps to: avoid `x.foo().bar().baz()`
chains that traverse multiple objects; prefer narrow accessors that
expose exactly what the caller needs.

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
- Yoshua Wuyts, "Combinatorial purity":
  <https://blog.yoshuawuyts.com/combinatorial-purity/>

## The principle

The original Demeter Project formulation gives a method `M` of class
`C` permission to invoke methods only on:

1. The object `M` is a method of (`self` in Rust).
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

In Rust:

```rust
// Demeter violation: 3-level traversal
let username = order.customer.contact.email.local_part();
```

The function holding `order` is now coupled to the structure of
`Order`, `Customer`, `Contact`, and `Email`. Any rename in any of
them breaks this code.

LoD says: ask `Order` for the username (or whatever you actually
need), and let `Order` decide how to traverse:

```rust
let username = order.customer_email_local_part();
```

`Order` now talks to its `Customer`, which talks to its `Contact`,
each layer responsible for its own knowledge.

## Why it matters

LoD-violating chains are **change amplifiers**:

- Rename `Contact.email` → `Contact.email_address`. Every call site
  that wrote `order.customer.contact.email.local_part()` breaks.
- Change `Email` from a struct with `local_part` to an opaque type.
  Same.
- Add validation that some emails are non-public; the call site has
  bypassed the validation.

When chains run deep, the coupled call site has *transitively
guessed* the data model of types it should not know about. The
guess becomes a constraint.

LoD also enforces a form of [encapsulation](#information-hiding):
your code expresses **what you want**, not **how to reach it**. The
how is hidden behind the boundary of each type.

## In Rust

Rust has some natural enforcements:

- Field access requires the field to be `pub` (or in the same
  module). Cross-crate `a.b.c.d` paths require multiple `pub`
  fields, which is friction.
- Borrow checker rejects some chained expressions that would compile
  in Java/C# (you can't borrow `a.b` and `a.c.d` simultaneously
  unless the borrows are non-conflicting).
- Iterator chains (`.map().filter().collect()`) are NOT LoD
  violations — each call is on the immediate object the previous
  returned, but conceptually they form one expression. The friend
  vs stranger test still applies: each adapter is a "friend" of the
  iterator interface.

The Rust-idiomatic LoD discipline:

1. **Public fields are rare.** Prefer methods that name the
   operation. `order.total()` instead of `order.total`.
2. **Methods take what they need, not the kitchen sink.** Pass an
   `&Order` if you need the order, not `&Workspace`.
3. **Don't traverse into details you don't own.** If you need a
   user's email format check, ask the user — don't pull the email
   out and check it yourself.

## Violations and remedies

### Anti-pattern: deep traversal

```rust
fn send_welcome(workspace: &Workspace, user_id: UserId) {
    let user = workspace.users().get(user_id).expect("user");
    let email = user.profile.contact.email.address.clone();
    let smtp = workspace.config.notifications.email.smtp.clone();
    send_email_via(smtp, email, /* ... */);
}
```

`send_welcome` knows the full shape of `Workspace`, `User`, `Profile`,
`Contact`, `Email`, `Config`, `NotificationsConfig`, `EmailConfig`,
`SmtpConfig`. Touching any of them is a breaking change for
`send_welcome`.

### Idiomatic fix: pass what's needed; let the owner traverse

```rust
fn send_welcome(notifier: &Notifier, user: &User) -> Result<()> {
    notifier.send_welcome_email(user)
}
```

`Notifier` is a port (trait + impl) that knows about SMTP config.
`User` exposes `email_address(&self) -> &Email` (one accessor) and
keeps everything else private. `send_welcome` knows two friends:
`Notifier` and `User`.

### Anti-pattern: returning a deep tree just to extract a leaf

```rust
fn primary_address(o: &Order) -> &str {
    &o.customer().get_addresses()[0].postal_code.region.name
}
```

Five hops. Add one new layer between `Customer` and `Address` and
the function breaks.

### Idiomatic fix: ask for the leaf directly

```rust
impl Order {
    pub fn primary_region(&self) -> &str { /* knows internals */ }
}
```

`Order` owns the traversal; callers ask for what they need.

### Anti-pattern: returning internal mutable state

```rust
impl Cart {
    pub fn items_mut(&mut self) -> &mut Vec<Item> { &mut self.items }
}

// Caller now has unrestricted access to internal state:
cart.items_mut().push(weird_item);          // bypasses validation
cart.items_mut().sort_by_key(|i| i.price);  // breaks invariants
```

`items_mut()` returns a stranger. Once you hand the caller `&mut Vec<Item>`,
they can do anything Vec allows — including violating invariants
that `Cart::add_item` was supposed to enforce.

### Idiomatic fix: expose operations, not the container

```rust
impl Cart {
    pub fn add(&mut self, item: Item) -> Result<()> { /* validates */ }
    pub fn remove(&mut self, id: ItemId) -> Result<()> { /* validates */ }
    pub fn items(&self) -> impl Iterator<Item = &Item> { self.items.iter() }
}
```

Callers do work through the cart, not on its internals. Read-only
iterator access is OK; mutation goes through methods that enforce
invariants.

### Anti-pattern: pass-through accessor chains

```rust
impl Order {
    pub fn customer(&self) -> &Customer { &self.customer }
}
impl Customer {
    pub fn contact(&self) -> &Contact { &self.contact }
}
impl Contact {
    pub fn email(&self) -> &Email { &self.email }
}

// Now any caller can:
let e = order.customer().contact().email();
```

You've exposed the full traversal. Every step is technically a
"method call on `self`", but the caller has assembled a chain that
violates LoD's spirit.

### Idiomatic fix: don't add the accessors until they are necessary, and even then add the *operation* not the *getter*

```rust
impl Order {
    pub fn customer_email(&self) -> &Email { &self.customer.contact.email }
}
```

One accessor, one purpose. If `Customer` later separates work and
home contacts, the change happens in `Order::customer_email`, not at
every call site.

### Anti-pattern: getter for everything

```rust
#[derive(...)]
pub struct User {
    pub id: UserId,
    pub email: Email,
    pub roles: Vec<Role>,
    pub created_at: DateTime<Utc>,
    pub last_login: Option<DateTime<Utc>>,
    pub preferences: UserPrefs,
    pub addresses: Vec<Address>,
}
```

Every field is `pub`. Callers can reach into any of them. This is
LoD's worst-case shape: no encapsulation. Renaming any field is
breaking.

### Idiomatic fix: private fields, narrow API

```rust
pub struct User { /* private fields */ }
impl User {
    pub fn id(&self) -> UserId { self.id }
    pub fn email(&self) -> &Email { &self.email }
    pub fn has_role(&self, r: Role) -> bool { /* ... */ }
    pub fn is_admin(&self) -> bool { /* ... */ }
    pub fn primary_address(&self) -> Option<&Address> { /* ... */ }
}
```

`User` decides what to expose. Internals can be rearranged.

## Iterator chains are not LoD violations

```rust
let total: Money = order.items().map(|i| i.price()).sum();
```

This is **not** a Demeter violation, even though it chains three
calls. Each call is on the iterator interface, which is the same
"friend" throughout. The chain expresses *one* idea (sum of prices)
in one expression. Demeter is about coupling to unrelated objects,
not about syntactic chain length.

A useful test: if the chain transforms a single conceptual entity
(a sequence), it is fine. If the chain hops across unrelated
entities (`workspace.config.notifications.email.smtp`), it is the
LoD-violating pattern.

## LoD at the module level

LoD generalizes to modules. A module that reaches *deep* into
another module's submodules is the same anti-pattern:

```rust
// Bad
use other_crate::internals::storage::adapters::postgres::pool::Pool;
```

The using crate depends on three layers of `other_crate`'s
hierarchy. Renaming any of `internals`/`storage`/`adapters`/`postgres`/`pool`
breaks downstream.

LoD-friendly version: `other_crate` exposes a re-export at the
crate root.

```rust
use other_crate::Pool;
```

The path is one hop. Internals are free to evolve.

## How code-ranker detects LoD violations

Module-level LoD violations have a graph signature:

| Signal | LoD interpretation |
|---|---|
| `Uses` edge from one crate to a deeply-nested module of another crate (path depth > 2) | Reaching too far into another crate's hierarchy |
| Multiple call sites with very long callee paths (e.g. `a.b.c.d.e()`) | Function-level LoD violation; requires AST analysis |
| Public field on a struct that is read from another crate | Pure data exposure; future rule |

A future rule **`cross-crate-deep-reach`** could detect: import
of an item more than 2 path segments deep into a foreign crate.
Severity low (often fine), confidence medium (real violation in
many but not all cases).

## Suggested recommendation template

> **LoD candidate**: crate `api` imports
> `domain::internals::types::raw::User`. The import reaches four
> levels deep into `domain`'s module hierarchy, exposing the using
> crate to renames at every level. Add a `pub use` at `domain`'s
> root (`domain::User`) and import via the shorter path. The
> Demeter principle (Holland et al., 1988) extends to module
> traversal: depend on friends, not on strangers' internals.
>
> Reference: <https://www2.ccs.neu.edu/research/demeter/papers/law-of-demeter/oopsla88-law-of-demeter.pdf>

## Related principles

- [DIP](solid-dependency-inversion.md) — DIP makes the friends
  trait-based, which limits how deep callers can reach.
- [Information Hiding](composition-over-inheritance.md) — LoD is
  the dynamic counterpart to "hide your fields".
- [SRP](solid-single-responsibility.md) — when a method talks to too
  many strangers, it usually has too many responsibilities.

## References

1. Lieberherr, K. and Holland, I. "Assuring Good Style for
   Object-Oriented Programs". *IEEE Software*, 1989.
2. Holland, I. "Specifying Reusable Components Using Contracts".
   PhD thesis, Northeastern University, 1992.
3. Bock, D. "The Paperboy, The Wallet, and The Law of Demeter".
   <https://www2.ccs.neu.edu/research/demeter/demeter-method/LawOfDemeter/paper-boy/demeter.pdf>
4. Hunt, A. and Thomas, D. *The Pragmatic Programmer*. Topic 28.
5. Wuyts, Y. "Combinatorial purity".
   <https://blog.yoshuawuyts.com/combinatorial-purity/>
6. Demeter Project home.
   <https://www.ccs.neu.edu/research/demeter/>
