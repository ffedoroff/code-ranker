# Law of Demeter — Principle of Least Knowledge (in Rust)

**TL;DR**: A method `f` of struct `T` should only call methods on:
(a) `T` itself, (b) `T`'s direct fields, (c) parameters passed to
`f`, (d) objects `f` constructs locally. In short: "talk to friends,
not to strangers". In Rust this maps to: avoid `x.foo().bar().baz()`
chains that traverse multiple objects; prefer narrow accessors that
expose exactly what the caller needs.

<!-- doc:base "Canonical sources" -->

<!-- doc:base "The principle" -->

<!-- doc:base "Why it matters" -->

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

<!-- doc:base "Related principles" -->

<!-- doc:base "References" -->
