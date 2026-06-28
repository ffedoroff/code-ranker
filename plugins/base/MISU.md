# Make Invalid States Unrepresentable

**TL;DR**: Move correctness from runtime checks into the type system.
A `User` cannot have a missing email; a `Connection` cannot be queried
before being opened; a parsed value cannot also be a parse error.
Union types (tagged variants), distinct value types, and typestate
make this principle strong — many invariants become compile errors if
violated.

## Canonical sources

- Yaron Minsky, "Effective ML: Make Illegal States Unrepresentable"
  (2010 Jane Street tech talk). The phrase originates here.
  <https://blog.janestreet.com/effective-ml-revisited/>
- Alexis King, "Parse, don't validate" (2019):
  <https://lexi-lambda.github.io/blog/2019/11/05/parse-don-t-validate/>

## The principle

Two designs of the same feature can differ dramatically in how many
runtime checks they require:

**Design A** (invalid states representable):

```
type User {
    email: Optional<String>      // may be absent
    age: Optional<Int>           // may be absent
    role: String                 // any string
}

function send_birthday_email(u: User) {
    email = u.email.expect("user without email?!")
    age   = u.age.expect("user without age?!")
    if u.role == "admin" || u.role == "Admin" || u.role == "ADMIN" {
        // role is a string, so every case must be checked
    }
    // ...
}
```

**Design B** (invalid states *unrepresentable*):

```
type User {
    email: Email           // always present, parsed at construction
    age: Age               // bounded value type, guaranteed <= 150
    role: Role             // variant: Admin | Member | Guest
}

function send_birthday_email(u: User) {
    email = u.email        // no Optional
    age   = u.age          // no Optional, no range check
    if u.role == Role.Admin {
        // role is a variant, exhaustively matchable
    }
    // ...
}
```

Design A pushes correctness onto every caller. Design B pushes it
to `User`'s construction — once, in one place. After that, the
compiler enforces the invariants.

Minsky's principle: **make invalid states syntactically impossible**.
King's reformulation: **parse, don't validate** — convert raw data
into a type that carries the proof of validity, then never re-validate.

## Why it matters

Bugs cluster around "this case shouldn't happen but the code allows
it". Every forced unwrap, every "won't happen" assertion, every
defensive `if x is present` is an invariant living in your head
rather than in the code.

When you encode the invariant in a type:

- **The compiler enforces it** — every call site is checked.
- **The invariant is visible** — readers see `Email` and know it's
  validated, no need to trace back to a constructor.
- **Tests don't have to repeat it** — you don't write 50 tests
  asserting "email is well-formed at every public entry point",
  because every entry point's signature already says so.
- **Refactoring is safe** — extracting code that takes a `User`
  still has its invariants.

A type system with union types (tagged variants) is especially
powerful here. Variants make the "this is exactly one of N
alternatives" pattern trivial; some languages also offer move-only or
use-once semantics that prevent whole categories of state-machine
violations and double-use.

## Techniques

### 1. Union types instead of stringly-typed values

```
// Bad
type Request {
    method: String     // "GET", "POST", "get", "POSt", etc.
    body: Optional<Bytes>
}

// Good
union Request {
    Get    { url: Url }
    Post   { url: Url, body: Bytes }
    Delete { url: Url }
}
```

A `Request.Get` literally cannot have a body, because the variant
has no `body` field. The state "GET with a body" is unrepresentable.

### 2. Distinct value type with a private constructor

```
type Email { value: String }  // constructor is private

function Email.parse(raw: String) -> Result<Email, ParseEmailError> {
    if raw.contains("@") && /* ... full validation ... */ {
        return Ok(Email { value: raw })
    } else {
        return Err(ParseEmailError.Invalid)
    }
}

function Email.as_string() -> String { return this.value }
```

You cannot construct an `Email` without going through `parse`. Once
constructed, every downstream consumer can rely on it being
well-formed. No re-validation, no defensive checks. (Cross-reference:
[Distinct Value Types](CoI.md) section.)

### 3. Typestate for state machines

```
type Connection<S> { /* state-specific fields, tagged by S */ }

type Closed
type Open

function Connection<Closed>.open() -> Result<Connection<Open>, ConnectError> { /* ... */ }

function Connection<Open>.query(sql) -> Result<Rows> { /* ... */ }
function Connection<Open>.close() -> Connection<Closed> { /* ... */ }
```

`Connection<Closed>.query` does not exist. The compiler rejects
`query` on a closed connection. The state machine is encoded in
types, not in `if self.is_open { ... } else { fail }`.

### 4. Constrained numeric / container types

```
function allocate(count: PositiveInt) -> List<Slot> { /* ... */ }
```

`allocate(0)` does not type-check, because `PositiveInt` cannot hold
zero. The function does not need to check at runtime. A non-empty
collection type works the same way for "must have at least one
element".

### 5. Small variant types replacing booleans

```
// Bad
function save(record, force: bool) -> Result

// What does force = true mean?  When?

// Good
enum SaveBehaviour { ErrorIfExists, OverwriteIfExists }
function save(record, behaviour: SaveBehaviour) -> Result
```

Call sites become self-documenting:
`save(r, SaveBehaviour.OverwriteIfExists)` versus `save(r, true)`.

## Violations and remedies

### Anti-pattern: optional fields for required data

```
type OrderRequest {
    customer_id: Optional<CustomerId>   // required, but optional for "easier deserialization"
    items: Optional<List<Item>>         // required
    total: Optional<Money>              // required
}

function process(req: OrderRequest) -> Result {
    cid   = req.customer_id.or_error(Error.MissingCustomer)?
    items = req.items.or_error(Error.MissingItems)?
    total = req.total.or_error(Error.MissingTotal)?
    // ...
}
```

Every consumer must unwrap. The `OrderRequest` type is semantically
"an order, but maybe not really".

### Idiomatic fix: required fields, separate raw type for deserialization

```
// Wire-level (deserialization target)
type OrderRequestRaw {
    customer_id: Optional<CustomerId>
    items: Optional<List<Item>>
    total: Optional<Money>
}

// Domain-level (validated)
type OrderRequest {
    customer_id: CustomerId
    items: List<Item>
    total: Money
}

function OrderRequestRaw.into_domain() -> Result<OrderRequest, RequestError> {
    return Ok(OrderRequest {
        customer_id: this.customer_id.or_error(RequestError.MissingCustomer)?,
        items: this.items.or_error(RequestError.MissingItems)?,
        total: this.total.or_error(RequestError.MissingTotal)?,
    })
}
```

Validation happens once at the wire boundary. After that, `OrderRequest`
has no optional fields, and every downstream function can rely on the
fields being present.

This is King's "parse, don't validate" applied at the API boundary.

### Anti-pattern: state encoded in a flag

```
type Connection {
    socket: TcpStream
    is_open: bool
}

function Connection.query(sql) -> Result<Rows> {
    if !this.is_open { return Err(Error.Closed) }
    // ...
}
function Connection.close() { this.is_open = false }
```

Every method needs the `is_open` check. The compiler cannot help.

### Idiomatic fix: typestate

```
type Connection<S> { socket: TcpStream }  // tagged by state S
type Open
type Closed

function Connection<Closed>.open() -> Result<Connection<Open>>
function Connection<Open>.query(sql) -> Result<Rows>
function Connection<Open>.close() -> Connection<Closed>
```

`query` on a `Connection<Closed>` does not compile.

### Anti-pattern: parallel collections that must stay in sync

```
type Catalog {
    names: List<String>
    prices: List<Money>
    in_stock: List<bool>
}
```

The invariant "lengths are equal" is unstated. A bug that pushes to
two lists but not the third desynchronizes silently.

### Idiomatic fix: one record per row

```
type CatalogItem { name: String, price: Money, in_stock: bool }
type Catalog { items: List<CatalogItem> }
```

The invariant is built in: there is exactly one of each field per
item.

### Anti-pattern: plain strings for "kind-of typed" identifiers

```
function deactivate(user_id: String, by: String) -> Result { /* ... */ }
```

`deactivate(by, user_id)` (arguments swapped) compiles. Production
bug.

### Idiomatic fix: distinct value types

```
type UserId  { value: Uuid }
type AdminId { value: Uuid }

function deactivate(user: UserId, by: AdminId) -> Result { /* ... */ }
```

Swapping arguments fails to compile.

### Anti-pattern: builder that allows `.build()` on incomplete state

```
type UserBuilder { email: Optional<String>, age: Optional<Int> }

function UserBuilder.email(e) -> UserBuilder { this.email = Some(e); return this }
function UserBuilder.age(a)   -> UserBuilder { this.age = Some(a); return this }
function UserBuilder.build()  -> Result<User, BuildError> {
    return Ok(User {
        email: this.email.or_error(BuildError.MissingEmail)?,
        age:   this.age.or_error(BuildError.MissingAge)?,
    })
}
```

Forgetting `.email()` is caught at runtime, not at compile time.

### Idiomatic fix: typestate builder

```
type UserBuilder<E, A> { email: E, age: A }
type NoEmail
type NoAge

function UserBuilder.new() -> UserBuilder<NoEmail, NoAge>
function UserBuilder<NoEmail, A>.email(e: Email) -> UserBuilder<Email, A>
function UserBuilder<E, NoAge>.age(a: Age)        -> UserBuilder<E, Age>
function UserBuilder<Email, Age>.build()          -> User
```

`build()` only exists on `UserBuilder<Email, Age>`. Forgetting either
step is a compile error.

(See [OCP](OCP.md) for the trade-off: adding a new
required field is breaking. Reserve typestate for genuinely required
fields.)

## When NOT to use this principle

The principle has limits. Encoding *every* invariant in types becomes
counter-productive:

- **Performance** — a non-empty collection wrapper has a non-trivial
  cost versus a plain list with a runtime check.
- **API ergonomics** — typestate is invasive and may force users into
  awkward conversion patterns.
- **Compilation time** — many phantom/generic type parameters
  multiply.
- **Diminishing returns** — sometimes the runtime check is genuinely
  small and the type-level proof is large.

A pragmatic heuristic: encode invariants that **multiple consumers**
need. A single-use invariant ("this function takes a sequence that
must have an even number of elements") may be cheaper as a runtime
assertion.

## How code-ranker detects representable-invalid-state risk

Code Ranker's static graph cannot directly read invariants. It can flag
*structural risk*:

| Signal | Interpretation |
|---|---|
| Functions with many forced-unwrap / "won't happen" assertions on optional return values | Signals invariants in the head of the author, not in the types. Future AST rule. |
| Public type with many optional fields | Possibly invalid-state-representable. Check whether construction goes through a parse-style constructor. |
| String-typed identifiers across many call sites | Distinct-value-type candidates. Detectable from AST. |
| Functions taking same-type arguments without naming | Swapping risk. AST analysis. |

Code Ranker's current rule set does not catch these directly. The
**LLM-verification** prompt mode (see
`cpt-code-ranker-fr-prompt-composer`) can ask an LLM reading the code
to flag these patterns.

## Suggested recommendation template

> **Make-Invalid-States-Unrepresentable candidate**: type
> `OrderRequest` has 5 optional fields, all of which downstream
> code unwraps. This is a "parse, don't validate" candidate: split
> `OrderRequest` into `OrderRequestRaw` (wire-level, all optional)
> and `OrderRequest` (domain-level, all required), with a single
> `into_domain` parse-step at the boundary.
>
> Source: King, "Parse, don't validate" (2019); Minsky, "Effective
> ML" (2010).

## Related principles

- [LSP](LSP.md) — types that encode
  invariants make LSP contracts implicit (no documentation needed for
  "email must be valid" — the type says so).
- [Distinct Value Types](CoI.md) — the
  workhorse technique for this principle.
- [KISS](KISS.md) — encoding too many invariants in types can
  violate KISS. Pick your battles.

## References

1. Minsky, Y. "Effective ML". Jane Street tech talk, 2010.
   <https://blog.janestreet.com/effective-ml-revisited/>
2. King, A. "Parse, don't validate". 2019.
   <https://lexi-lambda.github.io/blog/2019/11/05/parse-don-t-validate/>
