# SRP — Single Responsibility Principle (in Rust)

**TL;DR**: A module, struct, or function should have one reason to change.
In a Rust workspace, this most often means: a single trait or struct should
not accumulate responsibilities for unrelated callers; a single module
should not be both the wiring root and the type registry; a single file
should not host both domain logic and storage adapters.

## Canonical sources

- Robert C. Martin, "The Principles of OOD" (originally in *More C++ Gems*,
  1996; later in *Clean Architecture*, 2017): "A class should have only one
  reason to change." Source: <https://blog.cleancoder.com/uncle-bob/2014/05/08/SingleReponsibilityPrinciple.html>
- Robert C. Martin, *Clean Architecture*, Ch. 7: refines the principle to
  "A module should be responsible to one, and only one, actor."
- Mark Seemann, "The Single Responsibility Principle" (2011):
  <https://blog.ploeh.dk/2011/03/22/SOLIDinIntroductoryProgramming/>
- Rust API Guidelines, future-proofing chapter (C-STRUCT-PRIVATE):
  <https://rust-lang.github.io/api-guidelines/future-proofing.html>
- matklad, "Large Rust Workspaces": one crate per bounded responsibility.
  <https://matklad.github.io/2021/08/22/large-rust-workspaces.html>

## The principle

Martin's later formulation is the most useful: **a module is responsible
to one actor**. An "actor" here is any stakeholder whose needs drive
changes — a regulatory body, a product owner, a downstream team. When a
module serves two actors, changes requested by one are forced through
review by the other, and the module accumulates conflicting pressures.

The popular short form — "one reason to change" — is sometimes
misread as "one method per class" or "one function per file". That is
not the principle. The unit of responsibility is **change pressure**:
if two pieces of code consistently change for the same reason, they
belong together; if they change for different reasons, they belong
apart.

In Rust, the unit of "responsibility" most naturally maps to a crate,
then to a module, then to a struct or trait. Functions are usually too
fine-grained — splitting a 50-line function in two does not change
which actor causes it to evolve.

## Why it matters

A module shared by multiple actors becomes a coordination chokepoint:

- Every PR must pass review by every actor's team.
- Tests proliferate because each actor's changes can break the others'.
- Refactoring is "expensive" because so many callers depend on the
  module's exact shape.
- Stack traces and commit history become hard to read: a single file
  with eight reasons to change has eight times the commit churn.

SRP is the principle that keeps a workspace **navigable**. When you
violate it, the symptom is not a runtime bug — it is the gradual
realization that nobody on the team feels comfortable touching certain
files.

## In Rust

The Rust toolchain encourages SRP at multiple granularities:

| Unit | What "one responsibility" means |
|---|---|
| Workspace | The whole product / library family |
| Crate | One bounded context (an "actor's worth" of code) |
| Module | One coherent concept inside a crate |
| File | One implementation concern (e.g. a single trait + its support types) |
| Struct/trait | One thing it represents |
| Function | One mental step the caller performs |

The most common SRP violation in real Rust workspaces is the
**"god module"**: a module imported by many siblings because it
collects unrelated re-exports, conversions, and helper types under one
name (often `service`, `helpers`, `util`, or just `mod.rs`).

The second most common violation is the **"god struct"**: a single
struct named `*Service`, `*Manager`, or `*Client` with 30+ methods
spanning multiple concerns (CRUD, validation, business rules, audit
logging, metrics, retries).

## Violations and remedies

### Anti-pattern: god `Service` struct

```rust
// Bad: one struct, many actors.
pub struct UserService {
    db: PgPool,
    cache: Redis,
    audit: AuditSink,
    metrics: MetricsClient,
    mailer: Mailer,
}

impl UserService {
    pub async fn create_user(&self, ...) -> Result<User> { /* ... */ }
    pub async fn deactivate_user(&self, ...) -> Result<()> { /* ... */ }
    pub async fn record_login(&self, ...) -> Result<()> { /* ... */ }
    pub async fn export_for_gdpr(&self, ...) -> Result<Bytes> { /* ... */ }
    pub async fn send_welcome_email(&self, ...) -> Result<()> { /* ... */ }
    pub async fn rotate_password(&self, ...) -> Result<()> { /* ... */ }
    pub async fn assign_role(&self, ...) -> Result<()> { /* ... */ }
    pub async fn audit_admin_change(&self, ...) -> Result<()> { /* ... */ }
    // ... 30 more methods
}
```

Reasons to change: GDPR compliance (legal), email templates (marketing),
auth flow (security), RBAC (product), audit retention (ops). Five
different actors, one struct.

### Idiomatic fix: split by actor

```rust
// One struct per cohesive responsibility.
pub struct UserRepository { db: PgPool }
impl UserRepository {
    pub async fn create(&self, ...) -> Result<User> { /* ... */ }
    pub async fn deactivate(&self, id: UserId) -> Result<()> { /* ... */ }
}

pub struct UserAuthService { repo: Arc<UserRepository>, hasher: Argon2 }
impl UserAuthService {
    pub async fn rotate_password(&self, ...) -> Result<()> { /* ... */ }
    pub async fn record_login(&self, ...) -> Result<()> { /* ... */ }
}

pub struct UserComplianceService { repo: Arc<UserRepository>, audit: Arc<AuditSink> }
impl UserComplianceService {
    pub async fn export_for_gdpr(&self, ...) -> Result<Bytes> { /* ... */ }
}

pub struct UserNotifier { mailer: Mailer }
impl UserNotifier {
    pub async fn welcome(&self, ...) -> Result<()> { /* ... */ }
}
```

Each type now has one actor. Legal changes touch only
`UserComplianceService`; marketing touches only `UserNotifier`; etc.

### Anti-pattern: god module

```rust
// crates/account/src/service/mod.rs (4000 LOC)
// Hosts: trait Service, struct ServiceContext, struct ServiceError,
// type aliases, re-exports of repos, helpers like `now_utc()`,
// macro_rules! `service_log!`, and miscellaneous extension traits.
```

When this file is imported by 19 siblings (as observed in real
codebases), every sibling pays the cost of every change to any
unrelated item in the file. The graph signal is an exceptionally
high fan-in module that sits at the centre of an import SCC.

### Idiomatic fix: pull each concern to its own file

```rust
// service/trait.rs       — the Service trait
// service/context.rs     — ServiceContext (DI carrier)
// service/error.rs       — ServiceError + From impls
// service/time.rs        — clock helpers
// service/macros.rs      — service_log! etc.
// service/mod.rs         — pub use only what is intentional API
```

Now a change to error formatting touches `error.rs` alone; siblings
import what they need from a leaf module, not from a god file.

### Anti-pattern: function with mixed concerns

```rust
async fn process_payment(order: Order, db: &PgPool) -> Result<()> {
    // 1. Validate
    if order.total < Money::ZERO { return Err(...); }
    if order.items.is_empty() { return Err(...); }
    // 2. Persist
    sqlx::query!("INSERT INTO orders ...").execute(db).await?;
    // 3. Charge
    let stripe_resp = stripe::charge(...).await?;
    // 4. Notify
    notify_warehouse(&order).await?;
    notify_user(&order).await?;
    // 5. Audit
    audit::record(...).await?;
    Ok(())
}
```

This function has five reasons to change. A new validation rule,
a Stripe API bump, a warehouse-integration change, a notification
preference, and an audit-format adjustment all touch the same body.

### Idiomatic fix: extract steps; orchestrator just sequences them

```rust
async fn process_payment(
    order: Order,
    validator: &OrderValidator,
    repo: &OrderRepository,
    payment: &PaymentGateway,
    notifier: &Notifier,
    audit: &AuditSink,
) -> Result<()> {
    validator.check(&order)?;
    repo.persist(&order).await?;
    payment.charge(&order).await?;
    notifier.notify(&order).await?;
    audit.record_payment(&order).await?;
    Ok(())
}
```

The orchestrator now has one reason to change: the order of steps.
Each collaborator has its own actor.

## SRP at the crate level

The same principle scales up. A crate is "responsible to one actor"
when its changelog is intelligible: every released version answers a
single question — "what changed for `X`?". When a crate has releases
labelled "add OpenAPI registration, fix Postgres reconnect, bump
serde, add JWT verification, format errors", it is serving five
actors and is a refactoring candidate.

In a workspace, the SRP-friendly layout (matklad's
"Large Rust Workspaces") looks like:

```
workspace/
├── libs/
│   ├── modkit-db/          # storage actor
│   ├── modkit-security/    # security actor
│   ├── modkit-http/        # transport actor
│   └── modkit-errors/      # error vocabulary actor
└── modules/
    ├── account/            # account product actor
    ├── billing/            # billing product actor
    └── notifications/      # notification product actor
```

Each crate has one actor; cross-crate dependencies are explicit.

## How code-ranker detects SRP violations

Code Ranker cannot read actors directly, but the graph signatures of an
SRP violation are unambiguous:

| Signal | SRP interpretation |
|---|---|
| Module with high fan-in × fan-out (god-module-coupling rule) | Module serves multiple unrelated siblings |
| File LOC and item-count breaching mega-file thresholds | Single file accumulating multiple concerns |
| Module composed mostly of re-exports + entangled in an SCC (prelude-sibling-cycle rule) | Module acts as both a facade and a participant in unrelated subsystems |
| Public function with very high fan-in (high-fan-in-public-api rule) | Single API surface used by many unrelated actors — every change is a coordination event |

Cross-references in code-ranker's catalog:

- `god-module-coupling` directly maps to "module-serving-many-actors"
- `mega-file` maps to "file-with-too-many-reasons-to-change"
- `prelude-sibling-cycle` maps to "facade-module-conflated-with-participation"

## Suggested recommendation template

When code-ranker detects a candidate SRP violation, the Finding should:

1. Quote Martin's "one reason to change" / "one actor".
2. Pin the violation to the offending node (module or file).
3. Ask the user to enumerate the *actors* whose changes touch this
   module in the last N months (informally — this is qualitative).
4. Suggest a split along those actor lines.
5. Cite Martin's clean-coder post and matklad's large-workspaces post.

Example body:

> **SRP violation candidate**: module `domain::service` has fan-in 19
> and fan-out 6. SRP (Martin 1996) prescribes one reason to change per
> module. Identify the actors driving recent commits to this module;
> if more than two are visible, split the module along those lines.
> Suggested first move: extract `domain::service::context` (the DI
> carrier) and `domain::service::errors` (the error vocabulary) into
> leaf modules; have the orchestrator (`service::mod.rs`) re-export
> only the intentional API.

## Related principles

- [Open/Closed Principle](OCP.md) — what to do once SRP
  has been applied: keep each unit closed to modification.
- [Interface Segregation Principle](ISP.md) —
  same idea applied to trait surface, not module surface.
- [DRY](DRY.md) — distinct: SRP is about *why* code changes; DRY is
  about *whether* knowledge is duplicated.
- [High Cohesion / Low Coupling](CoI.md) —
  SRP is the cohesion lever; CoI is the coupling lever.

## References

1. Martin, R. C. "The Single Responsibility Principle". Clean Coder
   Blog, 2014. <https://blog.cleancoder.com/uncle-bob/2014/05/08/SingleReponsibilityPrinciple.html>
2. Martin, R. C. *Clean Architecture: A Craftsman's Guide to Software
   Structure and Design*. Prentice Hall, 2017. Ch. 7 — "SRP: The
   Single Responsibility Principle".
3. Seemann, M. "The Single Responsibility Principle". Ploeh blog, 2011.
   <https://blog.ploeh.dk/2011/03/22/SOLIDinIntroductoryProgramming/>
4. matklad. "Large Rust Workspaces". 2021.
   <https://matklad.github.io/2021/08/22/large-rust-workspaces.html>
5. Rust API Guidelines, future-proofing chapter.
   <https://rust-lang.github.io/api-guidelines/future-proofing.html>
