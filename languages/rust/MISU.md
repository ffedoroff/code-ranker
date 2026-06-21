# Make Invalid States Unrepresentable (in Rust)

**TL;DR**: Move correctness from runtime checks into the type system.
A `User` cannot have a missing email; a `Connection` cannot be queried
before being opened; a parsed JSON value cannot also be a parse error.
Rust's enums, lifetimes, and typestate make this principle exceptionally
strong — many invariants compile-error if violated.

## Canonical sources

- Yaron Minsky, "Effective ML: Make Illegal States Unrepresentable"
  (2010 Jane Street tech talk). The phrase originates here.
  <https://blog.janestreet.com/effective-ml-revisited/>
- Alexis King, "Parse, don't validate" (2019):
  <https://lexi-lambda.github.io/blog/2019/11/05/parse-don-t-validate/>
- Yoshua Wuyts, "From design to type system" (2021):
  <https://blog.yoshuawuyts.com/from-design-to-type-system/>
- Will Crichton, "Typestate in Rust":
  <https://will-crichton.net/notes/typestate-in-rust/>
- Pedro Cuenca, "Pretty State Machine Patterns in Rust" (2017):
  <https://hoverbear.org/blog/rust-state-machine-pattern/>

## The principle

Two designs of the same feature can differ dramatically in how many
runtime checks they require:

**Design A** (invalid states representable):

```rust
struct User {
    email: Option<String>,        // may be None
    age: Option<u8>,              // may be None
    role: String,                 // any string
}

fn send_birthday_email(u: &User) {
    let email = u.email.as_ref().expect("user without email?!");
    let age = u.age.expect("user without age?!");
    if u.role == "admin" || u.role == "Admin" || u.role == "ADMIN" {
        // role is a string, so every case must be checked
    }
    // ...
}
```

**Design B** (invalid states *unrepresentable*):

```rust
struct User {
    email: Email,            // always present, parsed at construction
    age: Age,                // u8 newtype, guaranteed ≤ 150
    role: Role,              // enum: Admin | Member | Guest
}

fn send_birthday_email(u: &User) {
    let email = &u.email;    // no Option
    let age = u.age;         // no Option, no range check
    if u.role == Role::Admin {
        // role is an enum, exhaustively matchable
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
it". Every `.unwrap()`, every `.expect("won't happen")`, every
defensive `if x.is_some()` is an invariant living in your head
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

Rust's type system is unusually powerful here. Sum types (`enum`)
make the "this is exactly one of N alternatives" pattern trivial;
the borrow checker prevents whole categories of state-machine
violations; affine types (`move`-only) prevent double-use.

## In Rust

The Rust tools for this principle:

### 1. Sum types instead of stringly-typed enums

```rust
// Bad
struct Request {
    method: String,    // "GET", "POST", "get", "POSt", etc.
    body: Option<Vec<u8>>,
}

// Good
enum Request {
    Get { url: Url },
    Post { url: Url, body: Vec<u8> },
    Delete { url: Url },
}
```

A `Request::Get` literally cannot have a body, because the variant
has no `body` field. The state "GET with a body" is unrepresentable.

### 2. Newtype with private constructor

```rust
pub struct Email(String);

impl Email {
    pub fn parse(raw: &str) -> Result<Self, ParseEmailError> {
        if raw.contains('@') && /* ... full validation ... */ {
            Ok(Email(raw.to_owned()))
        } else {
            Err(ParseEmailError::Invalid)
        }
    }
    pub fn as_str(&self) -> &str { &self.0 }
}
```

You cannot construct an `Email` without going through `parse`. Once
constructed, every downstream consumer can rely on it being
well-formed. No re-validation, no defensive checks. (Cross-reference:
[Newtype Pattern](CoI.md) section.)

### 3. Typestate for state machines

```rust
pub struct Connection<S> { /* state-specific fields */ _phantom: PhantomData<S> }

pub struct Closed;
pub struct Open;

impl Connection<Closed> {
    pub fn open(self) -> Result<Connection<Open>, ConnectError> { /* ... */ }
}

impl Connection<Open> {
    pub fn query(&self, sql: &str) -> Result<Rows> { /* ... */ }
    pub fn close(self) -> Connection<Closed> { /* ... */ }
}
```

`Connection<Closed>::query` does not exist. The compiler rejects
`query` on a closed connection. The state machine is encoded in
types, not in `if self.is_open { ... } else { panic!(); }`.

### 4. `NonZeroU32`, `NonEmpty<T>` for invariant-bearing containers

```rust
use std::num::NonZeroU32;

pub fn allocate(count: NonZeroU32) -> Vec<Slot> { /* ... */ }
```

`allocate(0)` does not type-check. The function does not need to
check at runtime.

### 5. Smart enums replacing booleans

```rust
// Bad
fn save(record: Record, force: bool) -> Result<()>;

// What does `force = true` mean?  When?

// Good
enum SaveBehaviour { ErrorIfExists, OverwriteIfExists }
fn save(record: Record, behaviour: SaveBehaviour) -> Result<()>;
```

Call sites become self-documenting: `save(r, SaveBehaviour::OverwriteIfExists)`
versus `save(r, true)`.

### 6. `Pin` for type-level invariants that the compiler enforces

Rare in user code, but exemplary: `Pin<P>` guarantees the pointee
will not move, which is required for self-referential structs. The
invariant is enforced by the type, not by documentation.

## Violations and remedies

### Anti-pattern: `Option<T>` for required fields

```rust
struct OrderRequest {
    customer_id: Option<CustomerId>,   // required, but Option for "easier deserialization"
    items: Option<Vec<Item>>,          // required
    total: Option<Money>,              // required
}

fn process(req: OrderRequest) -> Result<()> {
    let cid = req.customer_id.ok_or(Error::MissingCustomer)?;
    let items = req.items.ok_or(Error::MissingItems)?;
    let total = req.total.ok_or(Error::MissingTotal)?;
    // ...
}
```

Every consumer must unwrap. The `OrderRequest` struct is
semantically "an order, but maybe not really".

### Idiomatic fix: required fields, optional `OrderRequestRaw` for deserialization

```rust
// Wire-level (deserialization target)
#[derive(Deserialize)]
struct OrderRequestRaw {
    customer_id: Option<CustomerId>,
    items: Option<Vec<Item>>,
    total: Option<Money>,
}

// Domain-level (validated)
struct OrderRequest {
    customer_id: CustomerId,
    items: Vec<Item>,
    total: Money,
}

impl OrderRequestRaw {
    fn into_domain(self) -> Result<OrderRequest, RequestError> {
        Ok(OrderRequest {
            customer_id: self.customer_id.ok_or(RequestError::MissingCustomer)?,
            items: self.items.ok_or(RequestError::MissingItems)?,
            total: self.total.ok_or(RequestError::MissingTotal)?,
        })
    }
}
```

Validation happens once at the wire boundary. After that, `OrderRequest`
has no `Option`, and every downstream function can rely on the
fields being present.

This is King's "parse, don't validate" applied at the API boundary.

### Anti-pattern: state encoded in a flag

```rust
struct Connection {
    socket: TcpStream,
    is_open: bool,
}

impl Connection {
    pub fn query(&self, sql: &str) -> Result<Rows> {
        if !self.is_open { return Err(Error::Closed); }
        // ...
    }
    pub fn close(&mut self) { self.is_open = false; }
}
```

Every method needs the `is_open` check. The compiler cannot help.

### Idiomatic fix: typestate

```rust
struct Connection<S> { socket: TcpStream, _state: PhantomData<S> }
struct Open; struct Closed;

impl Connection<Closed> { pub fn open(...) -> Result<Connection<Open>>; }
impl Connection<Open> {
    pub fn query(&self, sql: &str) -> Result<Rows>;
    pub fn close(self) -> Connection<Closed>;
}
```

`query` on a `Connection<Closed>` does not compile.

### Anti-pattern: parallel collections that must stay in sync

```rust
struct Catalog {
    names: Vec<String>,
    prices: Vec<Money>,
    in_stock: Vec<bool>,
}
```

The invariant "lengths are equal" is unstated. A bug that pushes to
two vectors but not the third desynchronizes silently.

### Idiomatic fix: one struct per row

```rust
struct CatalogItem { name: String, price: Money, in_stock: bool }
struct Catalog { items: Vec<CatalogItem> }
```

The invariant is built in: there is exactly one of each field per
item.

### Anti-pattern: `String` for "kind-of typed" identifiers

```rust
fn deactivate(user_id: String, by: String) -> Result<()> { /* ... */ }
```

`deactivate(by, user_id)` (arguments swapped) compiles. Production
bug.

### Idiomatic fix: newtype

```rust
pub struct UserId(Uuid);
pub struct AdminId(Uuid);

fn deactivate(user: UserId, by: AdminId) -> Result<()> { /* ... */ }
```

Swapping arguments fails to compile.

### Anti-pattern: builder that allows `.build()` on incomplete state

```rust
struct UserBuilder { email: Option<String>, age: Option<u8> }
impl UserBuilder {
    fn email(mut self, e: String) -> Self { self.email = Some(e); self }
    fn age(mut self, a: u8) -> Self { self.age = Some(a); self }
    fn build(self) -> Result<User, BuildError> {
        Ok(User {
            email: self.email.ok_or(BuildError::MissingEmail)?,
            age: self.age.ok_or(BuildError::MissingAge)?,
        })
    }
}
```

Forgetting `.email()` is caught at runtime, not at compile time.

### Idiomatic fix: typestate builder

```rust
struct UserBuilder<E, A> { email: E, age: A }
struct NoEmail; struct NoAge;

impl UserBuilder<NoEmail, NoAge> {
    fn new() -> Self { Self { email: NoEmail, age: NoAge } }
}
impl<A> UserBuilder<NoEmail, A> {
    fn email(self, e: Email) -> UserBuilder<Email, A> { /* ... */ }
}
impl<E> UserBuilder<E, NoAge> {
    fn age(self, a: Age) -> UserBuilder<E, Age> { /* ... */ }
}
impl UserBuilder<Email, Age> {
    fn build(self) -> User { User { email: self.email, age: self.age } }
}
```

`build()` only exists on `UserBuilder<Email, Age>`. Forgetting either
step is a compile error.

(See [OCP](OCP.md) for the trade-off: adding a new
required field is breaking. Reserve typestate for genuinely required
fields.)

## When NOT to use this principle

The principle has limits. Encoding *every* invariant in types becomes
counter-productive:

- **Performance** — `NonEmpty<T>` has a non-trivial cost vs `Vec<T>`
  with a runtime check.
- **API ergonomics** — typestate is invasive and may force users into
  awkward conversion patterns.
- **Compilation time** — many phantom types and generic bounds
  multiply.
- **Reverse psychology** — sometimes the runtime check is genuinely
  small and the type-level proof is large.

A pragmatic heuristic: encode invariants that **multiple consumers**
need. A single-use invariant ("this function takes a slice that must
have an even number of elements") may be cheaper as a debug-assert.

<!-- doc:base "How code-ranker detects representable-invalid-state risk" -->

<!-- doc:base "Suggested recommendation template" -->

<!-- doc:base "Related principles" -->

## References

1. Minsky, Y. "Effective ML". Jane Street tech talk, 2010.
   <https://blog.janestreet.com/effective-ml-revisited/>
2. King, A. "Parse, don't validate". 2019.
   <https://lexi-lambda.github.io/blog/2019/11/05/parse-don-t-validate/>
3. Wuyts, Y. "From design to type system". 2021.
   <https://blog.yoshuawuyts.com/from-design-to-type-system/>
4. Crichton, W. "Typestate in Rust".
   <https://will-crichton.net/notes/typestate-in-rust/>
5. Cuenca, P. "Pretty State Machine Patterns in Rust". 2017.
   <https://hoverbear.org/blog/rust-state-machine-pattern/>
6. Rust API Guidelines, "Type safety".
   <https://rust-lang.github.io/api-guidelines/dependability.html>
