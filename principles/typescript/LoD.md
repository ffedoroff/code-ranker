# Law of Demeter — Principle of Least Knowledge (in TypeScript)

**TL;DR**: A method `f` of class/object `T` should only call methods
on: (a) `T` itself, (b) `T`'s direct fields, (c) parameters passed to
`f`, (d) objects `f` constructs locally. In short: "talk to friends,
not to strangers". In TypeScript this maps to: avoid
`x.foo.bar.baz.qux` attribute chains and `obj.a().b().c()` method
chains that traverse multiple unrelated objects; prefer narrow
accessors that expose exactly what the caller needs. Optional
chaining (`?.`) makes train wrecks ergonomic — that is a trap, not a
fix.

## Canonical sources

- Ian Holland, Karl Lieberherr et al., "Object-Oriented Programming:
  An Objective Sense of Style" (1988): the formal statement of the
  Law of Demeter. <https://dl.acm.org/doi/10.1145/62083.62113>
- Lieberherr, K. and Holland, I., "Assuring Good Style for
  Object-Oriented Programs", *IEEE Software*, 1989.
- David Bock, "The Paperboy, The Wallet, and The Law of Demeter"
  (2001): the canonical metaphor.
  <https://www2.ccs.neu.edu/research/demeter/demeter-method/LawOfDemeter/paper-boy/demeter.pdf>
- Hunt and Thomas, *The Pragmatic Programmer*, Topic 28 "Coupling
  and the Law of Demeter".
- Sandi Metz, *Practical Object-Oriented Design in Ruby* (POODR),
  ch. 4 — "Creating Flexible Interfaces", and her talks on Demeter
  as a coupling smell.
- Kent C. Dodds, "Prop Drilling".
  <https://kentcdodds.com/blog/prop-drilling>

## The principle

A method `M` of class `C` may invoke methods only on:

1. The object `M` is a method of (`this`).
2. Arguments of `M`.
3. Objects created by `M`.
4. Direct fields of the object `M` is a method of.
5. Module-scoped (global, in their sense) values accessible to `C`.

**Not** allowed: invoking methods on objects *returned* from methods
of any of the above. `a.b().c()` traverses two objects; LoD says
you've reached too far.

Bock's "Paperboy": when collecting money, the paperboy should not
say "give me your wallet". He says "you owe me $5". The customer
manages their own wallet. The paperboy talks to a friend (the
customer), not to a stranger (the wallet).

In TypeScript:

```ts
// Demeter violation: 4-level traversal
const city = user.profile.address.city.country.name;
```

The function holding `user` is now coupled to the shape of `User`,
`Profile`, `Address`, `City`, and `Country`. Renaming any of these
fields, or wrapping any in an optional, breaks this call site.

LoD says: ask `user` for what you actually need, and let `user`
decide how to traverse:

```ts
const country = user.countryName();
```

## Why it matters

LoD-violating chains are **change amplifiers**:

- Rename `Address.city` → `Address.cityRef`. Every call site that
  wrote `user.profile.address.city.country.name` breaks.
- Make `profile` lazy-loaded (now `Promise<Profile>`). Every chain
  breaks at the type level.
- Add a `null` somewhere in the middle. Every chain either breaks
  or quietly adopts `?.` and starts returning `undefined`.

When chains run deep, the call site has *transitively guessed* the
data model of types it should not know about. The guess becomes a
constraint.

## Optional chaining hides train wrecks

TypeScript's `?.` is wonderful for safety and dangerous for
discipline:

```ts
const name = user?.profile?.address?.city?.country?.name ?? "unknown";
```

Type-checks. Runs without throwing. Looks "defensive". It is still
the same Demeter violation as before, just with the failure mode
papered over. Every layer of `?.` is one more piece of structure the
caller is silently asserting it understands.

A useful tell: **if your call site contains more than one `?.` in a
single expression, it almost certainly knows too much about the
shape of something it doesn't own.** Move the traversal behind a
method on the type that owns the first link.

```ts
class User {
  countryName(): string { /* one place that knows the shape */ }
}
```

## Type narrowing through chains is a smell

```ts
if (order.customer && order.customer.contact && order.customer.contact.email) {
  send(order.customer.contact.email.address);
}
```

The narrowing tells you the caller is reconstructing the invariants
of `Customer`, `Contact`, and `Email` at the call site. Each `&&` is
an assertion about a foreign type's structure. Ask `Order` for what
you need; let `Order` enforce its own invariants.

## React prop drilling is a Demeter violation

Prop drilling is LoD's exact UI shape. A parent reaches *through*
intermediate components into a grandchild's needs:

```tsx
function App({ user }: { user: User }) {
  return <Page user={user} />;
}
function Page({ user }: { user: User }) {
  return <Header user={user} />;        // Page doesn't use `user`
}
function Header({ user }: { user: User }) {
  return <Avatar user={user} />;        // Header doesn't use `user` either
}
function Avatar({ user }: { user: User }) {
  return <img src={user.profile.avatarUrl} />;
}
```

`Page` and `Header` are couriers. They know about `User` only to
forward it. Any change to `User`'s shape ripples through every
courier's prop types. This is the "talking to strangers" anti-pattern
played out across a component tree.

Remedies (Dodds): hoist via context (`UserContext`), inject what's
needed at the leaf (compound components), or pass `children` so
intermediates need not know prop shapes at all.

## ORMs and lazy graphs

`user.profile.address` on a Prisma/Drizzle/TypeORM entity is not
"free attribute access". Depending on how the query was constructed,
each hop may trigger an additional query, throw, or return
`undefined`:

```ts
const user = await prisma.user.findUnique({ where: { id } });
// user.profile is undefined here — relation not included
user.profile.address; // runtime error or N+1
```

The LoD fix is **also** the performance fix: at the query boundary,
ask for exactly the shape the caller needs:

```ts
const u = await prisma.user.findUnique({
  where: { id },
  select: { id: true, countryName: true },
});
// u.countryName is the only field this caller touches.
```

A flat DTO returned at the boundary is LoD-friendly: the caller
knows one type, not a graph. Drizzle and TypeORM follow the same
shape — `with`/`relations` + `select` columns. The discipline:
**select what you'll use; project to a DTO; do not pass live
entities into components or use-case code.**

## GraphQL: queries are LoD made visible

A GraphQL query *is* a precise statement of "what I will touch":

```graphql
query { user(id: $id) { countryName } }
```

If your component then accesses `user.profile.address.city`, the
type system catches it because you didn't ask for it. GraphQL pushes
the discipline into the schema: callers declare their dependency on
shape up front, and the resolver layer owns traversal. This is LoD
at the API boundary.

## API client chains: fluent vs. train wreck

```ts
client.users.byId(id).posts.list();
```

When is this OK? When `client.users`, `byId(...)`, `.posts`,
`.list()` are all methods on **one logical friend** — a fluent
builder that returns specialised views of *itself*. SDKs like Stripe
or the official Octokit do this; each `.something` is a
sub-resource of the same `Client`, not a foreign object.

When is it a violation? When the chain hops across *domain*
objects: `order.customer().wallet().balance().amount()` — different
entities, each with their own invariants. The test: are you
navigating a single API surface (friend), or assembling a path
through your domain model (strangers)?

## Computed property access bypasses static checks

```ts
const v = (obj as any)[key][nested][leaf];
```

The compiler can't see the chain. The runtime risk is identical to
the static version, and the LoD violation is worse: you have given
up the one tool that flags the coupling. Avoid; if dynamic access
is necessary, restrict it to one layer and validate the result
(e.g. with a zod schema) before doing anything else.

## Violations and remedies

### Anti-pattern: deep attribute traversal

```ts
function sendWelcome(workspace: Workspace, userId: string) {
  const user = workspace.users.byId(userId)!;
  const email = user.profile.contact.email.address;
  const smtp = workspace.config.notifications.email.smtp;
  sendEmailVia(smtp, email);
}
```

`sendWelcome` knows the full shape of nine types. Any rename is a
breaking change.

### Idiomatic fix: a friend that owns the traversal

```ts
function sendWelcome(notifier: Notifier, user: User): Promise<void> {
  return notifier.sendWelcomeEmail(user);
}
```

`Notifier` knows SMTP. `User` has one accessor for the email.
`sendWelcome` knows two friends.

### Anti-pattern: getter for everything

```ts
interface User {
  id: string;
  email: string;
  roles: Role[];
  createdAt: Date;
  lastLogin: Date | null;
  preferences: UserPrefs;
  addresses: Address[];
}
```

Every field is public. Every caller can reach into any of them.
This is LoD's worst case. (TypeScript interfaces have no notion of
private — everything is exposed by default.)

### Idiomatic fix: a narrow class with operations

```ts
class User {
  #data: UserData;
  constructor(d: UserData) { this.#data = d; }
  id(): string { return this.#data.id; }
  email(): string { return this.#data.email; }
  hasRole(r: Role): boolean { /* ... */ }
  isAdmin(): boolean { /* ... */ }
  primaryAddress(): Address | undefined { /* ... */ }
}
```

Use `#private` fields for real encapsulation (not just TS `private`,
which is erased). The class chooses what to expose.

### Anti-pattern: pass-through accessor chains

```ts
class Order   { customer() { return this.c; } }
class Customer{ contact()  { return this.k; } }
class Contact { email()    { return this.e; } }

// Callers assemble:
const e = order.customer().contact().email();
```

Every step is "a method on the previous object", so it passes a
narrow reading of LoD — but the *call site* has assembled a chain
through three unrelated types. This is the textbook violation.

### Idiomatic fix: operation, not getter

```ts
class Order {
  customerEmail(): Email { return this.c.contact().email(); }
}
```

One accessor, one purpose. If `Customer` later splits work/home
contacts, the change lives in `Order.customerEmail`.

### Anti-pattern: returning internal mutable state

```ts
class Cart {
  items(): Item[] { return this._items; }   // live array!
}

cart.items().push(weirdItem);        // bypasses validation
cart.items().sort((a, b) => a.p - b.p); // breaks invariants
```

### Idiomatic fix: expose operations; return readonly views

```ts
class Cart {
  add(item: Item): void { /* validates, mutates */ }
  remove(id: string): void { /* validates */ }
  items(): readonly Item[] { return this._items; }
}
```

`readonly` is type-level only, but it makes mutation intent
explicit. For runtime safety, return a copy or freeze.

## Method chains that are *not* LoD violations

```ts
const total = order.items().map(i => i.price()).reduce((a, b) => a + b, 0);
```

Each call is on the same conceptual friend — the array/iterator
interface. The chain expresses *one* idea (sum of prices). LoD is
about coupling to unrelated objects, not about chain length.

The test (Sandi Metz's phrasing): if every link in the chain
returns the **same kind of thing** (an iterator, a query builder, a
promise), it's a fluent pipeline. If each link returns a
**different domain object**, it's a train wreck.

## LoD at the module level

```ts
// Bad
import { Pool } from "other-pkg/dist/internals/storage/adapters/postgres/pool";
```

Three layers deep into another package's hierarchy. Any internal
rename breaks the importer. LoD-friendly:

```ts
import { Pool } from "other-pkg";
```

The package's `index.ts` re-exports `Pool`. Internals are free to
evolve.

## How code-ranker detects LoD violations

| Signal | LoD interpretation |
|---|---|
| Import from `pkg/internals/...` or beyond `pkg/src/index` | Reaching into another module's hierarchy |
| Call sites with attribute paths of depth > 3 | Likely train wreck; AST analysis required |
| Components forwarding props they never read | Prop drilling; likely LoD smell |
| `?.` chains with > 1 link in one expression | Optional-chained train wreck |
| Public mutable arrays/maps returned from class methods | Stranger-handed-out internals |

## Suggested recommendation template

> **LoD candidate**: `notifier.ts` reads
> `workspace.config.notifications.email.smtp.host`. The chain
> traverses five types and couples the notifier to the full
> configuration tree. Expose a single accessor
> (`workspace.smtpConfig()`) or inject `SmtpConfig` directly. The
> Demeter principle (Lieberherr & Holland, 1989) extends to nested
> data: depend on friends, not on strangers' internals.

## Related principles

- [DIP](DIP.md) — DIP makes the friends
  interface-typed, which limits how deep callers can reach.
- [Information Hiding](CoI.md) — LoD is
  the dynamic counterpart to "hide your fields".
- [SRP](SRP.md) — when a method talks to
  too many strangers, it usually has too many responsibilities.
- [ISP](ISP.md) — narrow interfaces
  naturally limit the reach of any one caller.

## References

1. Lieberherr, K. and Holland, I. "Assuring Good Style for
   Object-Oriented Programs". *IEEE Software*, 1989.
2. Holland, I. "Specifying Reusable Components Using Contracts".
   PhD thesis, Northeastern University, 1992.
3. Bock, D. "The Paperboy, The Wallet, and The Law of Demeter".
   <https://www2.ccs.neu.edu/research/demeter/demeter-method/LawOfDemeter/paper-boy/demeter.pdf>
4. Hunt, A. and Thomas, D. *The Pragmatic Programmer*. Topic 28.
5. Metz, S. *Practical Object-Oriented Design in Ruby* (POODR),
   ch. 4.
6. Dodds, K. C. "Prop Drilling".
   <https://kentcdodds.com/blog/prop-drilling>
7. Demeter Project home.
   <https://www.ccs.neu.edu/research/demeter/>
