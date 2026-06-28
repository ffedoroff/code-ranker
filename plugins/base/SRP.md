# SRP — Single Responsibility Principle

**TL;DR**: A module, type, or function should have one reason to change.
In practice this most often means: a single interface or type should
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

The unit of "responsibility" most naturally maps to a package, then to
a module, then to a type or interface. Functions are usually too
fine-grained — splitting a 50-line function in two does not change
which actor causes it to evolve.

## Why it matters

A module shared by multiple actors becomes a coordination chokepoint:

- Every change must pass review by every actor's team.
- Tests proliferate because each actor's changes can break the others'.
- Refactoring is "expensive" because so many callers depend on the
  module's exact shape.
- Stack traces and commit history become hard to read: a single file
  with eight reasons to change has eight times the commit churn.

SRP is the principle that keeps a codebase **navigable**. When you
violate it, the symptom is not a runtime bug — it is the gradual
realization that nobody on the team feels comfortable touching certain
files.

## Granularities

SRP applies at multiple granularities:

| Unit | What "one responsibility" means |
|---|---|
| Project | The whole product / library family |
| Package | One bounded context (an "actor's worth" of code) |
| Module | One coherent concept inside a package |
| File | One implementation concern (e.g. a single interface + its support types) |
| Type/interface | One thing it represents |
| Function | One mental step the caller performs |

The most common SRP violation in real codebases is the
**"god module"**: a module imported by many siblings because it
collects unrelated re-exports, conversions, and helper types under one
name (often `service`, `helpers`, `util`, or just the package entry
point).

The second most common violation is the **"god type"**: a single
type named `*Service`, `*Manager`, or `*Client` with 30+ methods
spanning multiple concerns (CRUD, validation, business rules, audit
logging, metrics, retries).

## Violations and remedies

### Anti-pattern: god `Service` type

```
// Bad: one type, many actors.
type UserService {
    db: DbPool
    cache: Cache
    audit: AuditSink
    metrics: MetricsClient
    mailer: Mailer
}

UserService.create_user(...) -> Result<User>
UserService.deactivate_user(...) -> Result
UserService.record_login(...) -> Result
UserService.export_for_gdpr(...) -> Result<Bytes>
UserService.send_welcome_email(...) -> Result
UserService.rotate_password(...) -> Result
UserService.assign_role(...) -> Result
UserService.audit_admin_change(...) -> Result
// ... 30 more methods
```

Reasons to change: GDPR compliance (legal), email templates (marketing),
auth flow (security), RBAC (product), audit retention (ops). Five
different actors, one type.

### Idiomatic fix: split by actor

```
// One type per cohesive responsibility.
type UserRepository { db: DbPool }
UserRepository.create(...) -> Result<User>
UserRepository.deactivate(id) -> Result

type UserAuthService { repo: UserRepository, hasher: Hasher }
UserAuthService.rotate_password(...) -> Result
UserAuthService.record_login(...) -> Result

type UserComplianceService { repo: UserRepository, audit: AuditSink }
UserComplianceService.export_for_gdpr(...) -> Result<Bytes>

type UserNotifier { mailer: Mailer }
UserNotifier.welcome(...) -> Result
```

Each type now has one actor. Legal changes touch only
`UserComplianceService`; marketing touches only `UserNotifier`; etc.

### Anti-pattern: god module

```
// account/service (4000 LOC), the module entry point.
// Hosts: interface Service, type ServiceContext, type ServiceError,
// type aliases, re-exports of repositories, helpers like now_utc(),
// logging helpers, and miscellaneous extension types.
```

When this file is imported by 19 siblings (as observed in real
codebases), every sibling pays the cost of every change to any
unrelated item in the file. The graph signal is an exceptionally
high fan-in module that sits at the centre of an import cycle.

### Idiomatic fix: pull each concern to its own file

```
// service/contract  — the Service interface
// service/context   — ServiceContext (dependency carrier)
// service/error     — ServiceError + conversions
// service/time      — clock helpers
// service/logging   — logging helpers
// service/entry     — re-export only what is intentional API
```

Now a change to error formatting touches `error` alone; siblings
import what they need from a leaf module, not from a god file.

### Anti-pattern: function with mixed concerns

```
function process_payment(order, db) -> Result {
    // 1. Validate
    if order.total < 0 { return error(...) }
    if order.items.is_empty() { return error(...) }
    // 2. Persist
    db.execute("INSERT INTO orders ...")
    // 3. Charge
    let resp = payment_provider.charge(...)
    // 4. Notify
    notify_warehouse(order)
    notify_user(order)
    // 5. Audit
    audit.record(...)
    return ok
}
```

This function has five reasons to change. A new validation rule,
a payment-provider API bump, a warehouse-integration change, a
notification preference, and an audit-format adjustment all touch the
same body.

### Idiomatic fix: extract steps; orchestrator just sequences them

```
function process_payment(order, validator, repo, payment, notifier, audit) -> Result {
    validator.check(order)
    repo.persist(order)
    payment.charge(order)
    notifier.notify(order)
    audit.record_payment(order)
    return ok
}
```

The orchestrator now has one reason to change: the order of steps.
Each collaborator has its own actor.

## SRP at the package level

The same principle scales up. A package is "responsible to one actor"
when its changelog is intelligible: every released version answers a
single question — "what changed for `X`?". When a package has releases
labelled "add API registration, fix database reconnect, bump
serialization library, add token verification, format errors", it is
serving five actors and is a refactoring candidate.

In a multi-package project, the SRP-friendly layout looks like:

```
project/
├── libs/
│   ├── db/          # storage actor
│   ├── security/    # security actor
│   ├── http/        # transport actor
│   └── errors/      # error vocabulary actor
└── modules/
    ├── account/     # account product actor
    ├── billing/     # billing product actor
    └── notifications/  # notification product actor
```

Each package has one actor; cross-package dependencies are explicit.

## How code-ranker detects SRP violations

Code Ranker cannot read actors directly, but the graph signatures of an
SRP violation are unambiguous:

| Signal | SRP interpretation |
|---|---|
| Module with high fan-in × fan-out (god-module-coupling rule) | Module serves multiple unrelated siblings |
| File LOC and item-count breaching mega-file thresholds | Single file accumulating multiple concerns |
| Module composed mostly of re-exports + entangled in a cycle (prelude-sibling-cycle rule) | Module acts as both a facade and a participant in unrelated subsystems |
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
5. Cite Martin's clean-coder post.

Example body:

> **SRP violation candidate**: module `domain.service` has fan-in 19
> and fan-out 6. SRP (Martin 1996) prescribes one reason to change per
> module. Identify the actors driving recent commits to this module;
> if more than two are visible, split the module along those lines.
> Suggested first move: extract `domain.service.context` (the
> dependency carrier) and `domain.service.errors` (the error
> vocabulary) into leaf modules; have the orchestrator (`service`
> entry point) re-export only the intentional API.

## Related principles

- [Open/Closed Principle](OCP.md) — what to do once SRP
  has been applied: keep each unit closed to modification.
- [Interface Segregation Principle](ISP.md) —
  same idea applied to interface surface, not module surface.
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
