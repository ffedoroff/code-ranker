# DRY — Don't Repeat Yourself (in Rust)

**TL;DR**: Every piece of knowledge must have a single, unambiguous,
authoritative representation within a system. DRY is about **knowledge
duplication**, not **code duplication** — copy-pasted lines that
encode different decisions are not DRY violations; one line in two
different modules that means "the maximum retry count" is.

## Canonical sources

- Andy Hunt and Dave Thomas, *The Pragmatic Programmer* (1999,
  Addison-Wesley): the source of the principle's name. Topic 9 in
  the 20th-anniversary edition: <https://pragprog.com/titles/tpp20/>
- Andy Hunt blog, "DRY is About Knowledge" (2014):
  <https://blog.codinghorror.com/dry-not-just-about-code/> (Atwood
  citing Hunt)
- matklad, "Three Levels of Repetition" (2024):
  <https://matklad.github.io/2024/02/02/three-levels-of-repetition.html>
- Dan Abramov, "The WET Codebase":
  <https://overreacted.io/the-wet-codebase/> (counterpoint:
  premature DRY is worse than duplication)
- Sandi Metz, "The Wrong Abstraction" (2016):
  <https://sandimetz.com/blog/2016/1/20/the-wrong-abstraction>

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

The skill is distinguishing knowledge duplication (which DRY targets)
from accidental similarity (which DRY does not).

## In Rust

Rust has several mechanisms that make true DRY clean and several
that make false DRY tempting. Use the first set; resist the second.

### Mechanisms for genuine DRY

**Constants and statics**:

```rust
pub const MIN_ALCOHOL_AGE: u8 = 18;
pub const MAX_USERNAME_LEN: usize = 64;
pub const PASSWORD_RESET_TTL: Duration = Duration::from_secs(60 * 15);
```

One canonical place. The compiler will not let you misspell
`MIN_ALCOHOL_AGE` — typos become compile errors.

**Functions that name a calculation**:

```rust
pub fn effective_tax_rate(subtotal: Money, jurisdiction: &Jurisdiction) -> Rate {
    base_rate(jurisdiction) + surcharge_for(subtotal)
}
```

The formula has one expression. If the regulation changes, you
change one place.

**Generic functions for true polymorphism**:

```rust
pub fn parse_id<T: From<Uuid>>(s: &str) -> Result<T> {
    Uuid::parse_str(s).map(T::from).map_err(...)
}
```

Used to derive `UserId`, `OrderId`, `TransactionId` from the same
parsing logic — *which is genuinely the same knowledge*.

**Macros for textual repetition with knowledge content**:

```rust
macro_rules! impl_id_newtype {
    ($name:ident) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
        pub struct $name(Uuid);
        impl $name {
            pub fn new() -> Self { Self(Uuid::new_v4()) }
            pub fn as_uuid(&self) -> &Uuid { &self.0 }
        }
        impl From<Uuid> for $name { fn from(u: Uuid) -> Self { Self(u) } }
    };
}

impl_id_newtype!(UserId);
impl_id_newtype!(OrderId);
impl_id_newtype!(TransactionId);
```

The macro encodes the **decision** "all IDs are UUIDs with this
exact shape". If the decision changes (say, to ULIDs), one
modification updates all newtypes.

**Derive macros for cross-cutting concerns**:

`#[derive(Debug, Clone, Serialize)]` codifies "every domain type
gets these capabilities" once, in `serde_derive`. You do not
re-implement `Debug` for every struct.

**Type aliases for shared shapes**:

```rust
pub type ConfigResult<T> = Result<T, ConfigError>;
```

The fact that "config operations return `Result<_, ConfigError>`"
appears once.

### Mechanisms that *tempt* false DRY

**Over-eager helper extraction**:

```rust
fn validate_user_input(s: &str) -> bool { s.len() > 0 && s.len() < 100 && !s.contains('\0') }
fn validate_order_note(s: &str) -> bool { s.len() > 0 && s.len() < 100 && !s.contains('\0') }
```

Tempting to extract `fn validate_short_text(s: &str) -> bool`. But
the two validations *happen* to coincide today. Tomorrow the order
note rule changes to "≤ 500 chars" and now the helper grows a
parameter, a boolean flag, two enum variants, etc.

Better: leave them duplicated until the third copy appears. Hunt:
"Rule of Three" — abstract when you have *three* concrete instances
proving the abstraction is real, not two.

**Premature shared crate**:

Workspaces accumulate a `crates/common/` or `crates/utils/` that
becomes a junk drawer of weakly-related helpers. The crate's "DRY"
benefit is illusory — the helpers were never the same knowledge,
just the same shape.

Better: leave the local helpers local. If three crates genuinely
need the same calculation, extract *that calculation*, not "stuff
the three crates might share".

**Forcing identical APIs onto different abstractions**:

```rust
trait Storage {
    fn put(&self, k: &str, v: &[u8]);
    fn get(&self, k: &str) -> Option<Vec<u8>>;
}
impl Storage for HashMap<String, Vec<u8>> { /* ... */ }
impl Storage for S3Client { /* ... */ }
```

Memory and S3 do not share a contract (see
[LSP](solid-liskov-substitution.md)). The shared trait is a
DRY-shaped illusion masking incompatible behaviours.

## Violations and remedies

### Anti-pattern: magic numbers duplicated

```rust
// crates/api/src/handlers/auth.rs
if username.len() > 64 { return Err(ApiError::TooLong); }

// crates/domain/src/user.rs
if request.name.len() > 64 { return Err(DomainError::Invalid); }

// crates/admin/src/forms.rs
fn validate(s: &str) -> bool { s.len() <= 64 }
```

If the limit changes, three places must be edited and someone will
miss the third.

### Idiomatic fix: single source of truth in a domain crate

```rust
// crates/domain/src/limits.rs
pub const MAX_USERNAME_LEN: usize = 64;
```

```rust
// everywhere else
use domain::limits::MAX_USERNAME_LEN;
if username.len() > MAX_USERNAME_LEN { ... }
```

### Anti-pattern: duplicated SQL schema knowledge

```rust
// crates/repo/src/users.rs
const COLS: &str = "id, email, name, created_at, deleted_at";

fn fetch(...) -> ... {
    sqlx::query("SELECT id, email, name, created_at, deleted_at FROM users WHERE id = $1")...
}
fn insert(...) -> ... {
    sqlx::query("INSERT INTO users (id, email, name, created_at) VALUES ...")...
}
```

The column list appears three times (in `COLS`, in the SELECT, in
the INSERT). Adding a column requires updating each.

### Idiomatic fix: an ORM-style entity or a `sqlx::FromRow` derive

```rust
#[derive(sqlx::FromRow)]
struct UserRow {
    id: Uuid,
    email: String,
    name: String,
    created_at: DateTime<Utc>,
    deleted_at: Option<DateTime<Utc>>,
}

const COLS: &str = "id, email, name, created_at, deleted_at";

fn fetch(id: Uuid) -> Result<UserRow> {
    sqlx::query_as::<_, UserRow>(&format!("SELECT {COLS} FROM users WHERE id = $1"))
        .bind(id).fetch_one(&pool).await
}
```

Adding a column means: add a field to `UserRow`, add a name to
`COLS`. Two edits, both in the same file.

### Anti-pattern: parallel validation in API and domain

```rust
// crates/api/src/handlers/orders.rs
pub fn create_order_handler(req: CreateOrderRequest) -> impl Reply {
    if req.items.is_empty() { return reject("no items"); }
    if req.total < Money::ZERO { return reject("negative total"); }
    // ... 12 more checks ...
}

// crates/domain/src/order.rs
impl Order {
    pub fn new(items: Vec<Item>, total: Money) -> Result<Self> {
        if items.is_empty() { return Err(...); }
        if total < Money::ZERO { return Err(...); }
        // ... 12 more checks ...
    }
}
```

Every validation rule exists twice. They drift.

### Idiomatic fix: validation lives in the domain; API delegates

```rust
// crates/domain/src/order.rs
impl Order {
    pub fn new(items: Vec<Item>, total: Money) -> Result<Self, DomainError> {
        if items.is_empty() { return Err(DomainError::NoItems); }
        if total < Money::ZERO { return Err(DomainError::NegativeTotal); }
        // ...
    }
}
```

```rust
// crates/api/src/handlers/orders.rs
pub fn create_order_handler(req: CreateOrderRequest) -> impl Reply {
    let order = match Order::new(req.items, req.total) {
        Ok(o) => o,
        Err(e) => return reject(e),
    };
    // ...
}
```

The API performs *no business validation*. It translates errors. A
new rule is added in one place — in the domain.

### Anti-pattern: copy-pasted code that ISN'T DRY

```rust
fn calculate_tax_us(amount: Money) -> Money { amount * 0.07 }
fn calculate_tax_eu(amount: Money) -> Money { amount * 0.21 }
fn calculate_tax_uk(amount: Money) -> Money { amount * 0.20 }
```

It would be tempting to extract `fn calculate_tax(rate: f32, amount: Money)`.
Should you?

**No** — for two reasons:

1. The three tax rates are not the same knowledge. They are
   independent regulations. If the EU rate changes, the US rate is
   unaffected.
2. The functions communicate intent. `calculate_tax_us(amount)` reads
   better at the call site than `calculate_tax(0.07, amount)`.

When VAT rates split by region into 27 individual values that vary
together (per EU directive), THEN extract. The Rule of Three applies.

### Idiomatic fix: leave as-is

Resist the urge. Three lookups in a table is fine. (See Sandi Metz,
"The Wrong Abstraction".)

## DRY at the crate level

Cross-crate DRY shows up as:

- A constant duplicated in multiple `Cargo.toml`s (e.g. `version`).
  Fix: workspace inheritance (`version.workspace = true`).
- A type duplicated across crates because both need "the same" struct
  shape. Fix: one defining crate, the others depend on it. (Or
  *don't fix* if the two structs happen to look alike but mean
  different things.)
- A dependency version pinned in multiple `Cargo.toml`s. Fix:
  `[workspace.dependencies]` declares once.

## How code-ranker detects DRY violations

DRY is the hardest principle to detect automatically — knowledge
duplication does not have a graph signature. Code Ranker can flag
*candidates*:

| Signal | DRY interpretation |
|---|---|
| Identical function names across multiple modules (e.g. `validate`, `parse`, `format`) | Possible knowledge duplication. Requires fn-name overlap analysis. |
| Public constants with identical *values* across multiple crates | Strong DRY-violation candidate. Requires AST inspection. |
| Multiple crates with similar Cargo.toml dependency lists | Possibly the same domain repeated. |
| Repeated string-literal regex patterns | Regex literals appearing in N source files is a textbook DRY violation. |

Code Ranker's static graph cannot tell you whether two functions
*encode the same knowledge* — that requires understanding the
function bodies. A future rule could flag literal duplication and
let the LLM-verification step (see `cpt-code-ranker-fr-prompt-composer`)
decide.

## Suggested recommendation template

> **DRY candidate** (low confidence): the constant `64` appears as a
> max-length check in 5 places across the workspace (api/auth.rs,
> domain/user.rs, admin/forms.rs, infra/email/templates.rs,
> shared/limits.rs). If these are encoding the same business rule
> ("usernames must be ≤ 64 chars"), consolidate to a single
> `domain::limits::MAX_USERNAME_LEN`. If they are independent (a
> column width, an email subject limit, a UI hint), keep them
> separate.
>
> Code Ranker cannot tell which case applies. See *Pragmatic Programmer*
> Topic 9 and matklad's "Three Levels of Repetition" for guidance
> on the call.

## Related principles

- [KISS](kiss.md) — DRY can violate KISS when premature abstraction
  introduces a more complex shape than the duplication.
- [YAGNI](yagni.md) — don't DRY for a hypothetical second instance
  that may never appear.
- [SRP](solid-single-responsibility.md) — SRP is the discipline that
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
